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

pub(super) async fn load(iso3: &str) -> Result<Detail, crate::Error> {
    let level = level(iso3)
        .ok_or_else(|| crate::Error::from(format!("geoBoundaries {iso3}: no municipal level")))?;
    let iso3 = geoboundaries_iso(iso3);
    let metadata = fetch_json(&metadata_url(iso3, level)).await?;
    let url = metadata
        .get("simplifiedGeometryGeoJSON")
        .or_else(|| metadata.get("gjDownloadURL"))
        .and_then(|value| value.as_str())
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            crate::Error::from(format!("geoBoundaries {iso3} {level}: no GeoJSON URL"))
        })?;
    crate::features::fetch(url, |feature: &serde_json::Value, out: &mut Detail| {
        out.features += 1;
        let Some(geometry) = feature.get("geometry").filter(|value| !value.is_null()) else {
            return;
        };
        if let Some(coords) = geometry.get("coordinates") {
            collect_geojson_lines(coords, &mut out.lines);
        }
        let Some(name) = feature_name(feature, NAME_KEYS) else {
            return;
        };
        let Some((lat, lon)) = label_center(geometry) else {
            return;
        };
        out.labels.push(MapLabel {
            lat,
            lon,
            en: name.clone(),
            pt: name,
            kind: LabelKind::City,
            rank: municipal_label_rank(feature.get("properties")),
            big: false,
        });
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
    fn metadata_endpoint_uses_the_documented_gbopen_path() {
        assert_eq!(
            metadata_url("BRA", "ADM2"),
            "https://www.geoboundaries.org/api/current/gbOpen/BRA/ADM2/"
        );
    }
}
