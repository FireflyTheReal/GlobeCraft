//! National municipal boundary services with keyless GeoJSON/ArcGIS access.
//! The table is deliberately small: countries without a stable JSON endpoint use geoBoundaries.
use super::geojson::{collect_geojson_lines, feature_name, label_center};
use super::municipal::GeoBox;
use super::{Detail, LabelKind, MapLabel, municipal_label_rank};
use crate::http::fetch_json;
use std::sync::Arc;

const PAGE_SIZE: usize = 1000;
const MAX_PAGES: usize = 20;
const WFS_COUNT: usize = 1000;

/// Grid cell size per national source. A cell is one request with a stable
/// URL. Sizes follow measurements from 2026-10-21 (Berlin/Paris/Madrid): a 1.5°
/// box cost 1.6 MB (DE) and 0.33 MB (ES); France's full commune polygons cost
/// 5.4 MB / 464 features for a 0.6° box and hit the WFS cap of 1,000 features at
/// 1.0°, so its cells are a quarter degree.
pub(super) fn cell_size_deg(iso3: &str) -> Option<f64> {
    match iso3 {
        "FRA" => Some(0.25),
        "USA" | "CAN" | "DEU" | "ESP" => Some(1.0),
        _ => None,
    }
}

const US_URL: &str =
    "https://tigerweb.geo.census.gov/arcgis/rest/services/TIGERweb/State_County/MapServer/1/query";
const CANADA_URL: &str = "https://geo.statcan.gc.ca/geo_wa/rest/services/2021/Cartographic_boundary_files/MapServer/4/query";
const FRANCE_URL: &str = "https://data.geopf.fr/wfs/ows";
const GERMANY_URL: &str = "https://sgx.geodatenzentrum.de/wfs_vg250";
const SPAIN_URL: &str = "https://services1.arcgis.com/nCKYwcSONQTkPA4K/arcgis/rest/services/Municipios/FeatureServer/0/query";

const US_NAMES: &[&str] = &["NAME"];
const CANADA_NAMES: &[&str] = &["CDNAME"];
const FRANCE_NAMES: &[&str] = &["nom_officiel"];
const GERMANY_NAMES: &[&str] = &["gen"];
const SPAIN_NAMES: &[&str] = &["NAMEUNIT"];

/// How far from the view centre a country's cells are requested. Dense, heavy
/// sources stay close; light ones (US counties, Canadian divisions) cover more.
pub(super) fn max_radius_deg(iso3: &str) -> f64 {
    match iso3 {
        "FRA" => 0.75,
        "DEU" => 1.5,
        "ESP" => 2.0,
        _ => 4.0,
    }
}

pub(super) fn source_label(iso3: &str) -> Option<&'static str> {
    match iso3 {
        "USA" => Some("US Census TIGERweb"),
        "CAN" => Some("Statistics Canada"),
        "FRA" => Some("IGN ADMIN EXPRESS"),
        "DEU" => Some("BKG VG250"),
        "ESP" => Some("IGN municipios"),
        _ => None,
    }
}

fn query_url(
    base: &str,
    params: impl IntoIterator<Item = (&'static str, String)>,
) -> Result<String, crate::Error> {
    let mut url =
        reqwest::Url::parse(base).map_err(|error| crate::Error::from(error.to_string()))?;
    url.query_pairs_mut().extend_pairs(params);
    Ok(url.into())
}

fn city_label(feature: &serde_json::Value, name_keys: &[&str]) -> Option<MapLabel> {
    let geometry = feature.get("geometry").filter(|value| !value.is_null())?;
    let (lat, lon) = label_center(geometry)?;
    let name = feature_name(feature, name_keys)?;
    Some(MapLabel {
        lat,
        lon,
        en: name.clone(),
        pt: name,
        kind: LabelKind::City,
        rank: municipal_label_rank(feature.get("properties")),
        big: false,
    })
}

/// Fields naming a municipality's state (département in France, province in Spain and
/// Canada), with how many leading characters of the value identify it. France's IGN
/// ADMIN EXPRESS renamed its fields in version 4 (2025) to the full BD TOPO names, so
/// both spellings are listed, then the commune's INSEE code, whose first two characters
/// are the metropolitan département (2A and 2B in Corsica).
const PARENT_KEYS: [(&str, usize); 8] = [
    ("code_insee_du_departement", usize::MAX),
    ("insee_dep", usize::MAX),
    ("code_insee", 2),
    ("sn_l", usize::MAX),
    ("SN_L", usize::MAX),
    ("STATE", usize::MAX),
    ("CDUID", 2),
    ("CODIGOINE", 2),
];

/// An opaque code for the state a municipal feature belongs to.
fn parent_code(props: Option<&serde_json::Value>) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let props = props?;
    PARENT_KEYS.iter().find_map(|(key, prefix)| {
        let value = match props.get(*key)? {
            serde_json::Value::String(text) => text.trim().to_owned(),
            // Numeric INE codes lose their leading zero ("08019" → 8019).
            serde_json::Value::Number(number) => format!("{:05}", number.as_u64()?),
            _ => return None,
        };
        let code: String = value.chars().take(*prefix).collect();
        (!code.is_empty()).then(|| {
            let mut hasher = std::hash::DefaultHasher::new();
            (*key, code).hash(&mut hasher);
            hasher.finish()
        })
    })
}

/// Records `parent` for the lines added since `before`.
fn tag_parents(out: &mut Detail, before: usize, parent: Option<u64>) {
    out.parents.resize(before, None);
    out.parents.resize(out.lines.len(), parent);
}

fn geojson_consumer(
    name_keys: &'static [&'static str],
) -> impl Fn(&serde_json::Value, &mut Detail) + Clone + Send + 'static {
    move |feature, out| {
        out.features += 1;
        let Some(geometry) = feature.get("geometry").filter(|value| !value.is_null()) else {
            return;
        };
        if let Some(coords) = geometry.get("coordinates") {
            let before = out.lines.len();
            collect_geojson_lines(coords, &mut out.lines);
            tag_parents(out, before, parent_code(feature.get("properties")));
        }
        if let Some(label) = city_label(feature, name_keys) {
            out.labels.push(label);
        }
    }
}

fn bbox_params(area: GeoBox) -> String {
    let GeoBox {
        min_lat,
        min_lon,
        max_lat,
        max_lon,
    } = area.clamped();
    format!("{min_lon:.4},{min_lat:.4},{max_lon:.4},{max_lat:.4},EPSG:4326")
}

fn envelope(area: GeoBox, decimals: usize) -> String {
    let GeoBox {
        min_lat,
        min_lon,
        max_lat,
        max_lon,
    } = area.clamped();
    format!("{min_lon:.decimals$},{min_lat:.decimals$},{max_lon:.decimals$},{max_lat:.decimals$}")
}

async fn arcgis_geojson(
    base: &str,
    params: Vec<(&'static str, String)>,
    names: &'static [&'static str],
    paginate: bool,
    cache: bool,
) -> Result<Detail, crate::Error> {
    let mut out = Detail::default();
    for page in 0..MAX_PAGES {
        let mut page_params = params.clone();
        page_params.push(("resultOffset", (page * PAGE_SIZE).to_string()));
        page_params.push(("resultRecordCount", PAGE_SIZE.to_string()));
        let url = query_url(base, page_params)?;
        let batch = if cache {
            crate::features::fetch(&url, geojson_consumer(names)).await?
        } else {
            crate::features::fetch_ephemeral(&url, geojson_consumer(names)).await?
        };
        let count = batch.features;
        let before = out.lines.len();
        out.parents.resize(before, None);
        out.parents.extend(batch.parents);
        out.lines.extend(batch.lines);
        out.labels.extend(batch.labels);
        out.features += count;
        if !paginate || count < PAGE_SIZE {
            break;
        }
    }
    Ok(out)
}

fn esri_center(rings: &[serde_json::Value]) -> Option<(f64, f64)> {
    let ring = rings
        .iter()
        .max_by_key(|ring| ring.as_array().map_or(0, Vec::len))?;
    let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);
    let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
    for point in ring.as_array()? {
        let pair = point.as_array()?;
        let (lon, lat) = (pair.first()?.as_f64()?, pair.get(1)?.as_f64()?);
        if lat.is_finite() && lon.is_finite() {
            min_lat = min_lat.min(lat);
            max_lat = max_lat.max(lat);
            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
        }
    }
    (min_lat <= max_lat).then_some(((min_lat + max_lat) / 2.0, (min_lon + max_lon) / 2.0))
}

fn parse_esri(value: &serde_json::Value, name_keys: &[&str]) -> Detail {
    let mut out = Detail::default();
    for feature in value
        .get("features")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        out.features += 1;
        let Some(rings) = feature
            .pointer("/geometry/rings")
            .and_then(|v| v.as_array())
        else {
            continue;
        };
        let before = out.lines.len();
        for ring in rings {
            let line: Vec<(f64, f64)> = ring
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|point| {
                    let pair = point.as_array()?;
                    let lon = pair.first()?.as_f64()?;
                    let lat = pair.get(1)?.as_f64()?;
                    (lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0)
                        .then_some((lat, lon))
                })
                .collect();
            if line.len() >= 2 {
                out.lines.push(Arc::new(line));
            }
        }
        tag_parents(&mut out, before, parent_code(feature.get("attributes")));
        if let Some((lat, lon)) = esri_center(rings)
            && let Some(name) = feature
                .get("attributes")
                .and_then(|attributes| {
                    name_keys
                        .iter()
                        .find_map(|key| attributes.get(*key).and_then(|v| v.as_str()))
                })
                .filter(|name| !name.trim().is_empty())
        {
            out.labels.push(MapLabel {
                lat,
                lon,
                en: name.to_owned(),
                pt: name.to_owned(),
                kind: LabelKind::City,
                rank: municipal_label_rank(feature.get("attributes")),
                big: false,
            });
        }
    }
    out
}

async fn arcgis_esri(
    base: &str,
    params: Vec<(&'static str, String)>,
    names: &'static [&'static str],
) -> Result<Detail, crate::Error> {
    let mut out = Detail::default();
    for page in 0..MAX_PAGES {
        let mut page_params = params.clone();
        page_params.push(("resultOffset", (page * PAGE_SIZE).to_string()));
        page_params.push(("resultRecordCount", PAGE_SIZE.to_string()));
        let url = query_url(base, page_params)?;
        let value = fetch_json(&url).await?;
        let batch = crate::cpu::run(crate::cpu::Service::Features, move || {
            Ok(parse_esri(&value, names))
        })
        .await?;
        let count = batch.features;
        let before = out.lines.len();
        out.parents.resize(before, None);
        out.parents.extend(batch.parents);
        out.lines.extend(batch.lines);
        out.labels.extend(batch.labels);
        out.features += count;
        if count < PAGE_SIZE {
            break;
        }
    }
    Ok(out)
}

async fn united_states(area: GeoBox) -> Result<Detail, crate::Error> {
    arcgis_geojson(
        US_URL,
        vec![
            ("where", "1=1".into()),
            ("geometry", envelope(area, 4)),
            ("geometryType", "esriGeometryEnvelope".into()),
            ("inSR", "4326".into()),
            ("spatialRel", "esriSpatialRelIntersects".into()),
            ("outFields", "NAME,STATE,COUNTY,GEOID".into()),
            ("returnGeometry", "true".into()),
            ("outSR", "4326".into()),
            ("maxAllowableOffset", "0.005".into()),
            ("geometryPrecision", "4".into()),
            ("orderByFields", "GEOID".into()),
            ("f", "geojson".into()),
        ],
        US_NAMES,
        true,
        false,
    )
    .await
}

async fn canada(area: GeoBox) -> Result<Detail, crate::Error> {
    arcgis_esri(
        CANADA_URL,
        vec![
            ("where", "1=1".into()),
            ("geometry", envelope(area, 3)),
            ("geometryType", "esriGeometryEnvelope".into()),
            ("inSR", "4326".into()),
            ("spatialRel", "esriSpatialRelIntersects".into()),
            ("outFields", "CDUID,CDNAME".into()),
            ("returnGeometry", "true".into()),
            ("outSR", "4326".into()),
            ("maxAllowableOffset", "0.005".into()),
            ("geometryPrecision", "3".into()),
            ("orderByFields", "CDUID".into()),
            ("f", "json".into()),
        ],
        CANADA_NAMES,
    )
    .await
}

async fn france(area: GeoBox) -> Result<Detail, crate::Error> {
    let url = query_url(
        FRANCE_URL,
        vec![
            ("SERVICE", "WFS".into()),
            ("VERSION", "2.0.0".into()),
            ("REQUEST", "GetFeature".into()),
            (
                "TYPENAMES",
                "LIMITES_ADMINISTRATIVES_EXPRESS.LATEST:commune".into(),
            ),
            ("BBOX", bbox_params(area)),
            ("COUNT", WFS_COUNT.to_string()),
            ("SRSNAME", "EPSG:4326".into()),
            ("outputFormat", "application/json".into()),
        ],
    )?;
    crate::features::fetch_ephemeral(&url, geojson_consumer(FRANCE_NAMES)).await
}

async fn germany(area: GeoBox) -> Result<Detail, crate::Error> {
    let url = query_url(
        GERMANY_URL,
        vec![
            ("SERVICE", "WFS".into()),
            ("VERSION", "2.0.0".into()),
            ("REQUEST", "GetFeature".into()),
            ("TYPENAMES", "vg250:vg250_gem".into()),
            ("BBOX", bbox_params(area)),
            ("COUNT", WFS_COUNT.to_string()),
            ("SRSNAME", "EPSG:4326".into()),
            ("outputFormat", "application/json".into()),
        ],
    )?;
    crate::features::fetch_ephemeral(&url, geojson_consumer(GERMANY_NAMES)).await
}

async fn spain(area: GeoBox) -> Result<Detail, crate::Error> {
    arcgis_geojson(
        SPAIN_URL,
        vec![
            ("where", "1=1".into()),
            ("geometry", envelope(area, 4)),
            ("geometryType", "esriGeometryEnvelope".into()),
            ("inSR", "4326".into()),
            ("spatialRel", "esriSpatialRelIntersects".into()),
            ("outFields", "NAMEUNIT,CODIGOINE,OBJECTID".into()),
            ("returnGeometry", "true".into()),
            ("outSR", "4326".into()),
            ("maxAllowableOffset", "0.002".into()),
            ("orderByFields", "OBJECTID".into()),
            ("f", "geojson".into()),
        ],
        SPAIN_NAMES,
        true,
        false,
    )
    .await
}

/// One grid cell from the country's national service, or `None` when the
/// country has no viewport-capable national source.
pub(super) async fn load_cell(iso3: &str, area: GeoBox) -> Option<Result<Detail, crate::Error>> {
    match iso3 {
        "USA" => Some(united_states(area).await),
        "CAN" => Some(canada(area).await),
        "FRA" => Some(france(area).await),
        "DEU" => Some(germany(area).await),
        "ESP" => Some(spain(area).await),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_uses_wfs_longitude_latitude_axis_order() {
        let cell = GeoBox {
            min_lat: 48.5,
            min_lon: 2.0,
            max_lat: 49.0,
            max_lon: 2.5,
        };
        assert_eq!(bbox_params(cell), "2.0000,48.5000,2.5000,49.0000,EPSG:4326");
        assert_eq!(envelope(cell, 3), "2.000,48.500,2.500,49.000");
        assert_eq!(cell_size_deg("FRA"), Some(0.25));
        assert_eq!(cell_size_deg("DEU"), Some(1.0));
        assert_eq!(cell_size_deg("MEX"), None);
    }

    #[test]
    fn french_communes_name_their_departement_in_every_admin_express_version() {
        let parent = |props: serde_json::Value| parent_code(Some(&props));
        let v4 =
            parent(serde_json::json!({"code_insee": "75056", "code_insee_du_departement": "75"}));
        let v4_other =
            parent(serde_json::json!({"code_insee": "92012", "code_insee_du_departement": "92"}));
        assert!(v4.is_some());
        assert_ne!(
            v4, v4_other,
            "Paris and Hauts-de-Seine are different départements"
        );
        assert!(parent(serde_json::json!({"insee_dep": "75"})).is_some());
        // Without a département field, the commune code still separates départements.
        let by_code = |code: &str| parent(serde_json::json!({"code_insee": code}));
        assert_eq!(by_code("2A004"), by_code("2A041"));
        assert_ne!(by_code("2A004"), by_code("2B033"));
        assert_eq!(parent(serde_json::json!({"nom_officiel": "Paris"})), None);
    }

    #[test]
    fn esri_rings_are_projected_and_named() {
        let value = serde_json::json!({
            "features": [{
                "attributes": {"CDNAME": "Division No. 1"},
                "geometry": {"rings": [[[-60.0, 50.0], [-60.0, 51.0], [-59.0, 51.0], [-60.0, 50.0]]]}
            }]
        });
        let detail = parse_esri(&value, CANADA_NAMES);
        assert_eq!(detail.features, 1);
        assert_eq!(detail.lines.len(), 1);
        assert_eq!(detail.labels[0].en, "Division No. 1");
    }

    #[test]
    fn arcgis_query_encodes_pagination() {
        let url = query_url(
            US_URL,
            vec![
                ("where", "1=1".into()),
                ("resultOffset", "2000".into()),
                ("f", "geojson".into()),
            ],
        )
        .unwrap();
        assert!(url.contains("resultOffset=2000"));
        assert!(url.contains("f=geojson"));
    }
}
