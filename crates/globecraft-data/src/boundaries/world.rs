//! Municipal borders around the current camera anywhere in the world, from OSM.
//! An intentionally small, throttled bounding box avoids abusive Overpass calls.
use super::Line;
use super::municipal::GeoBox;
use std::sync::Arc;
const MAX_RESPONSE_BYTES: u64 = 15_000_000;
const MAX_ELEMENTS: usize = 3000;
const LINE_ADMISSION_THRESHOLD: usize = 6000;

/// OSM municipal outlines (admin levels 8 and 9) inside one grid cell.
pub(super) async fn load_osm_cell(area: GeoBox) -> Result<Vec<Line>, crate::Error> {
    // On-demand bounded geography, not a false promise of a global exhaustive file.
    let GeoBox {
        min_lat: lat0,
        min_lon: lon0,
        max_lat: lat1,
        max_lon: lon1,
    } = area.clamped();
    let query = format!(
        "[out:json][timeout:20];(way[\"boundary\"=\"administrative\"][\"admin_level\"~\"^(8|9)$\"]({lat0:.4},{lon0:.4},{lat1:.4},{lon1:.4});relation[\"boundary\"=\"administrative\"][\"admin_level\"~\"^(8|9)$\"]({lat0:.4},{lon0:.4},{lat1:.4},{lon1:.4}););out geom;"
    );
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(
        client
            .post("https://overpass.kumi.systems/api/interpreter")
            .form(&[("data", query)]),
        MAX_RESPONSE_BYTES,
    )
    .await?;
    let v = crate::http::parse_json(bytes).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || {
        Ok(parse_overpass(&v))
    })
    .await
}
fn parse_overpass(v: &serde_json::Value) -> Vec<Line> {
    let mut out = Vec::new();
    for object in v["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_ELEMENTS)
    {
        let kind = object["type"].as_str().unwrap_or("");
        if kind == "way" {
            push_geom(&object["geometry"], &mut out);
        } else if kind == "relation" {
            for member in object["members"].as_array().into_iter().flatten() {
                if member["type"] == "way"
                    && member["role"]
                        .as_str()
                        .is_some_and(|r| r == "outer" || r == "inner" || r.is_empty())
                {
                    push_geom(&member["geometry"], &mut out);
                }
            }
        }
    }
    out
}
fn push_geom(v: &serde_json::Value, out: &mut Vec<Line>) {
    if out.len() > LINE_ADMISSION_THRESHOLD {
        return;
    }
    let pts: Vec<(f64, f64)> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let lat = p["lat"].as_f64()?;
            let lon = p["lon"].as_f64()?;
            (lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0)
                .then_some((lat, lon))
        })
        .collect();
    if pts.len() >= 2 {
        out.push(Arc::new(pts));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn municipal_boundary_can_be_read_from_any_country() {
        let v = serde_json::json!({"elements":[{"type":"way","geometry":[{"lat":48.8,"lon":2.3},{"lat":48.9,"lon":2.4}]}]});
        assert_eq!(parse_overpass(&v)[0][0], (48.8, 2.3));
    }
}
