//! geoBoundaries gbOpen fallback: one simplified per-country administrative file.
use super::Detail;
use super::geojson::{collect_geojson_lines, feature_name, label_center};
use super::{LabelKind, MapLabel, municipal_label_rank};
use crate::http::fetch_json;

const API: &str = "https://www.geoboundaries.org/api/current/gbOpen";
const NAME_KEYS: &[&str] = &["shapeName", "name", "NAME"];

// The municipal level of each country, from a review of geoBoundaries' metadata
// (2026-10-22, docs/MUNICIPAL_COVERAGE.md): the deepest level whose units are
// municipalities rather than parcels, villages or wards, whose simplified file stays
// under about 40 MB, and which refines the level above it. Countries with a national
// source (DEU, ESP, FRA, USA, CAN, BRA) are not listed; neither are territories
// without any subdivision, which fall back to OSM cells.

/// Countries whose municipalities are geoBoundaries ADM1.
const ADM1_COUNTRIES: &str = "\
    AND ARE ATG BHR BHS BRB DMA GRD GRL KNA LBY LCA LIE MDA MDV MHL MLT MNE MUS NIU \
    NRU SMR SYC TTO TUV VCT XKX";

/// Countries whose municipalities are geoBoundaries ADM2.
const ADM2_COUNTRIES: &str = "\
    AFG ARG ARM AUS AZE BDI BGR BLR BLZ BRN BTN BWA COD COG COL CPV CUB CYP DJI DNK \
    DOM ECU EGY ERI EST FSM GAB GBR GEO GHA GNB GNQ GTM GUY HND HRV IDN IND IRL IRQ \
    ISL ISR JAM JOR JPN KAZ KGZ KIR KOR KWT LAO LBN LBR LSO LUX LVA MAR MEX MKD MNG \
    MRT NAM NGA NIC NLD NOR OMN PER PLW PRK PRY PSE QAT ROU RUS SAU SDN SGP SLV SOM \
    SRB SSD STP SUR SVN SWZ TCD TGO THA TJK TKM TLS TON TUR TWN URY UZB VEN VNM WSM \
    YEM ZMB";

/// Countries whose municipalities are geoBoundaries ADM3.
const ADM3_COUNTRIES: &str = "\
    AGO ALB AUT BEN BFA BIH BOL CAF CHE CHL CHN CIV CMR COM CRI CZE DZA ETH FIN FJI \
    GIN GMB GRC HTI HUN KEN KHM LKA LTU MDG MLI MMR MOZ MWI MYS NER NPL NZL PAK PAN \
    PHL PNG POL PRT RWA SEN SLE SVK SWE SYR TUN TZA UKR VUT ZAF ZWE";

/// Countries whose municipalities are geoBoundaries ADM4.
const ADM4_COUNTRIES: &str = "\
    BEL BGD IRN ITA SLB UGA";

/// geoBoundaries' code for a Natural Earth country code where the two differ.
fn geoboundaries_iso(iso3: &str) -> &str {
    match iso3 {
        "PSX" => "PSE",
        "SDS" => "SSD",
        "KOS" => "XKX",
        "SAH" => "ESH",
        other => other,
    }
}

/// The geoBoundaries level holding a country's municipalities, if it has one.
pub(super) fn level(iso3: &str) -> Option<&'static str> {
    let iso3 = geoboundaries_iso(iso3);
    let listed = |countries: &str| countries.split_whitespace().any(|code| code == iso3);
    [
        ("ADM1", ADM1_COUNTRIES),
        ("ADM2", ADM2_COUNTRIES),
        ("ADM3", ADM3_COUNTRIES),
        ("ADM4", ADM4_COUNTRIES),
    ]
    .into_iter()
    .find(|(_, countries)| listed(countries))
    .map(|(level, _)| level)
}

pub(super) fn metadata_url(iso3: &str, level: &str) -> String {
    format!("{API}/{iso3}/{level}/")
}

/// Natural Earth's states where they are not geoBoundaries ADM1: the level whose unit
/// count matches Natural Earth's admin-1 count (Italy's 110 provinces are ADM3, for
/// example). Municipal borders replace exactly these lines.
const ADM2_STATES: &str = "BEL BFA BIH GIN GRC LKA MWI PHL";
const ADM3_STATES: &str = "ITA UGA";

/// The geoBoundaries level of a country's states, or `None` when its municipalities
/// are ADM1 (each municipality is its own state; every border between two is drawn).
pub(super) fn state_level(iso3: &str) -> Option<&'static str> {
    let municipal = level(iso3)?;
    let iso3 = geoboundaries_iso(iso3);
    let listed = |countries: &str| countries.split_whitespace().any(|code| code == iso3);
    let state = if listed(ADM3_STATES) {
        "ADM3"
    } else if listed(ADM2_STATES) {
        "ADM2"
    } else {
        "ADM1"
    };
    (state != municipal).then_some(state)
}

/// A municipal file as read: the outlines and names, and for each municipality the
/// lines it added and a point inside it.
#[derive(Default)]
pub(super) struct MunicipalFile {
    detail: Detail,
    municipalities: Vec<Municipality>,
}

/// One municipality of a file: the lines it added, and a point inside it.
struct Municipality {
    lines: std::ops::Range<usize>,
    inside: Option<(f64, f64)>,
}

pub(super) fn read_municipality(feature: &serde_json::Value, out: &mut MunicipalFile) {
    out.detail.features += 1;
    let Some(geometry) = feature.get("geometry").filter(|value| !value.is_null()) else {
        return;
    };
    let start = out.detail.lines.len();
    if let Some(coords) = geometry.get("coordinates") {
        collect_geojson_lines(coords, &mut out.detail.lines);
    }
    let lines = start..out.detail.lines.len();
    let inside = super::parents::interior_point(&out.detail.lines[lines.clone()]);
    out.municipalities.push(Municipality { lines, inside });
    let Some(name) = feature_name(feature, NAME_KEYS) else {
        return;
    };
    let Some((lat, lon)) = inside.or_else(|| label_center(geometry)) else {
        return;
    };
    out.detail.labels.push(MapLabel {
        lat,
        lon,
        en: name.clone(),
        pt: name,
        kind: LabelKind::City,
        rank: municipal_label_rank(feature.get("properties")),
        big: false,
    });
}

/// One state's rings.
pub(super) fn read_state(feature: &serde_json::Value, out: &mut Vec<Vec<super::Line>>) {
    let mut rings = Vec::new();
    if let Some(coords) = feature.pointer("/geometry/coordinates") {
        collect_geojson_lines(coords, &mut rings);
    }
    out.push(rings);
}

/// The municipal detail with each line's state: the state containing the municipality,
/// or the municipality itself when it is the country's top level (`states` is `None`).
pub(super) fn with_parents(file: MunicipalFile, states: Option<&super::parents::States>) -> Detail {
    let MunicipalFile {
        mut detail,
        municipalities,
    } = file;
    detail.parents = vec![None; detail.lines.len()];
    for (index, Municipality { lines, inside }) in municipalities.into_iter().enumerate() {
        let parent = match states {
            Some(states) => inside.and_then(|point| states.containing(point)),
            None => Some(index),
        };
        for line in lines {
            detail.parents[line] = parent.map(|parent| parent as u64);
        }
    }
    detail
}

async fn file_url(iso3: &str, level: &str) -> Result<String, crate::Error> {
    let metadata = fetch_json(&metadata_url(iso3, level)).await?;
    metadata
        .get("simplifiedGeometryGeoJSON")
        .or_else(|| metadata.get("gjDownloadURL"))
        .and_then(|value| value.as_str())
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| crate::Error::from(format!("geoBoundaries {iso3} {level}: no GeoJSON URL")))
}

pub(super) async fn load(iso3: &str) -> Result<Detail, crate::Error> {
    let level = level(iso3)
        .ok_or_else(|| crate::Error::from(format!("geoBoundaries {iso3}: no municipal level")))?;
    let state_level = state_level(iso3);
    let iso3 = geoboundaries_iso(iso3);
    let file = crate::features::fetch(&file_url(iso3, level).await?, read_municipality).await?;
    let states = match state_level {
        Some(state_level) => {
            let rings: Vec<Vec<super::Line>> =
                crate::features::fetch(&file_url(iso3, state_level).await?, read_state).await?;
            Some(rings)
        }
        None => None,
    };
    crate::cpu::run(crate::cpu::Service::Features, move || {
        let states = states.map(super::parents::States::new);
        Ok(with_parents(file, states.as_ref()))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_country_uses_its_municipal_level() {
        assert_eq!(level("ITA"), Some("ADM4"), "comuni");
        assert_eq!(
            level("AUT"),
            Some("ADM3"),
            "Gemeinden, not cadastral parcels"
        );
        assert_eq!(level("POL"), Some("ADM3"), "gminas");
        assert_eq!(level("NLD"), Some("ADM2"), "gemeenten");
        assert_eq!(
            level("MDA"),
            Some("ADM1"),
            "raions are the only subdivision"
        );
        // Natural Earth codes that differ from geoBoundaries'.
        assert_eq!(level("SDS"), level("SSD"));
        assert_eq!(level("KOS"), Some("ADM1"));
        // National sources and undivided territories are not listed.
        assert_eq!(level("DEU"), None);
        assert_eq!(level("REU"), None);
        let all = [
            ADM1_COUNTRIES,
            ADM2_COUNTRIES,
            ADM3_COUNTRIES,
            ADM4_COUNTRIES,
        ];
        let codes: Vec<&str> = all.iter().flat_map(|c| c.split_whitespace()).collect();
        assert!(codes.iter().all(|c| c.len() == 3), "{codes:?}");
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "a country is listed twice");
    }

    #[test]
    fn states_follow_natural_earth_and_single_level_countries_are_their_own_states() {
        assert_eq!(state_level("POL"), Some("ADM1"));
        assert_eq!(
            state_level("ITA"),
            Some("ADM3"),
            "Natural Earth draws 110 provinces"
        );
        assert_eq!(state_level("BEL"), Some("ADM2"));
        assert_eq!(
            state_level("MDA"),
            None,
            "raions are municipalities and states at once"
        );
        assert_eq!(state_level("DEU"), None, "national source");
    }

    #[test]
    fn municipalities_take_the_state_that_contains_them() {
        let square = |lat: f64, lon: f64, size: f64| {
            serde_json::json!({"type": "Feature", "properties": {"shapeName": "M"},
                "geometry": {"type": "Polygon", "coordinates": [[
                    [lon, lat], [lon + size, lat], [lon + size, lat + size],
                    [lon, lat + size], [lon, lat]]]}})
        };
        let mut file = MunicipalFile::default();
        for lon in [0.0, 1.0, 2.0, 3.0] {
            read_municipality(&square(0.0, lon, 1.0), &mut file);
        }
        let mut states = Vec::new();
        read_state(&square(0.0, 0.0, 2.0), &mut states);
        read_state(&square(0.0, 2.0, 2.0), &mut states);
        let states = super::super::parents::States::new(states);
        let detail = with_parents(file, Some(&states));
        assert_eq!(detail.parents, [Some(0), Some(0), Some(1), Some(1)]);
        assert_eq!(detail.labels.len(), 4);
    }

    #[test]
    fn metadata_endpoint_uses_the_documented_gbopen_path() {
        assert_eq!(
            metadata_url("BRA", "ADM2"),
            "https://www.geoboundaries.org/api/current/gbOpen/BRA/ADM2/"
        );
    }
}
