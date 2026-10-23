use super::{CountryRing, LabelKind, Line, MapLabel};
use globecraft_core::language::fold_name;
use std::collections::HashMap;
use std::sync::Arc;
pub(super) fn feature_name(feature: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let props = feature.get("properties")?;
    keys.iter().find_map(|key| {
        props
            .get(*key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_owned())
    })
}

/// Label anchor: bounding-box centre of the geometry's largest outer ring (so countries with
/// far-away islands are labelled on the mainland). Returns (lat, lon).
pub(super) fn label_center(geometry: &serde_json::Value) -> Option<(f64, f64)> {
    let coords = geometry.get("coordinates")?;
    let rings: Vec<&serde_json::Value> = match geometry.get("type").and_then(|t| t.as_str())? {
        "Polygon" => coords.as_array()?.first().into_iter().collect(),
        "MultiPolygon" => coords
            .as_array()?
            .iter()
            .filter_map(|p| p.as_array()?.first())
            .collect(),
        _ => return None,
    };
    let ring = rings
        .into_iter()
        .max_by_key(|r| r.as_array().map_or(0, |a| a.len()))?;
    let (mut min_lat, mut max_lat, mut min_lon, mut max_lon) =
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in ring.as_array()? {
        let Some((lat, lon)) = crate::json_fields::coordinates(p) else {
            continue;
        };
        min_lat = min_lat.min(lat);
        max_lat = max_lat.max(lat);
        min_lon = min_lon.min(lon);
        max_lon = max_lon.max(lon);
    }
    (min_lat <= max_lat).then_some(((min_lat + max_lat) / 2.0, (min_lon + max_lon) / 2.0))
}

/// Brazilian cities exist both in the world populated-places set and in the IBGE municipality set;
/// keep only the most prominent entry (lowest rank) per name and ~0.5° cell.
pub fn dedupe_city_labels(labels: &mut Vec<MapLabel>) {
    let mut best: HashMap<(String, i32, i32), usize> = HashMap::new();
    let mut drop = vec![false; labels.len()];
    for (i, l) in labels.iter().enumerate() {
        // Place names come and go with the view; the renderer skips the ones another
        // label already shows (see `project_labels`).
        if l.kind != LabelKind::City || l.is_place() {
            continue;
        }
        for (dla, dlo) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let key = (
                fold_name(&l.pt),
                (l.lat * 2.0).floor() as i32 + dla,
                (l.lon * 2.0).floor() as i32 + dlo,
            );
            if let Some(&j) = best.get(&key) {
                if labels[j].rank <= l.rank {
                    drop[i] = true;
                } else {
                    drop[j] = true;
                }
                break;
            }
        }
        if !drop[i] {
            best.insert(
                (
                    fold_name(&l.pt),
                    (l.lat * 2.0).floor() as i32,
                    (l.lon * 2.0).floor() as i32,
                ),
                i,
            );
        }
    }
    let mut it = drop.into_iter();
    labels.retain(|_| !it.next().unwrap_or(false));
}

fn line_from_points(value: &serde_json::Value) -> Option<Line> {
    let line: Vec<(f64, f64)> = value
        .as_array()?
        .iter()
        .filter_map(crate::json_fields::coordinates)
        .collect();
    (line.len() >= 2).then(|| Arc::new(line))
}

// Recursively extracts GeoJSON coordinate sequences. GeoJSON order is [longitude, latitude].
pub(super) fn collect_geojson_lines(value: &serde_json::Value, out: &mut Vec<Line>) {
    let Some(items) = value.as_array() else {
        return;
    };
    let is_line = items
        .first()
        .and_then(|v| v.as_array())
        .is_some_and(|pair| pair.len() >= 2 && pair[0].is_number() && pair[1].is_number());
    if is_line {
        if let Some(line) = line_from_points(value) {
            out.push(line);
        }
    } else {
        for child in items {
            collect_geojson_lines(child, out);
        }
    }
}

/// Country outlines plus their outer rings. Holes stay in `lines` for drawing but are not
/// used for country lookup, so a point on an inland lake still belongs to its country.
pub(super) fn collect_country_geometry(
    geometry: &serde_json::Value,
) -> (Vec<Line>, Vec<CountryRing>) {
    let mut lines = Vec::new();
    let mut rings = Vec::new();
    let Some(coords) = geometry.get("coordinates") else {
        return (lines, rings);
    };
    let polygons: Vec<&serde_json::Value> = match geometry.get("type").and_then(|t| t.as_str()) {
        Some("Polygon") => vec![coords],
        Some("MultiPolygon") => coords.as_array().into_iter().flatten().collect(),
        _ => {
            collect_geojson_lines(coords, &mut lines);
            return (lines, rings);
        }
    };
    for polygon in polygons {
        for (index, ring) in polygon.as_array().into_iter().flatten().enumerate() {
            let Some(line) = line_from_points(ring) else {
                continue;
            };
            if index == 0
                && let Some(ring) = CountryRing::new(line.clone())
            {
                rings.push(ring);
            }
            lines.push(line);
        }
    }
    (lines, rings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn geojson_nested_rings_keep_latitude_longitude_order() {
        let value = json!([[[10.0, 20.0], [11.0, 21.0], [181.0, 30.0]], [[0.0, 0.0]]]);
        let mut lines = Vec::new();
        collect_geojson_lines(&value, &mut lines);
        assert_eq!(lines, vec![Arc::new(vec![(20.0, 10.0), (21.0, 11.0)])]);
    }

    #[test]
    fn country_geometry_keeps_outer_rings_separate_from_holes() {
        let geometry = json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0], [0.0, 0.0]],
                [[2.0, 2.0], [2.0, 3.0], [3.0, 3.0], [2.0, 2.0]]
            ]
        });
        let (lines, rings) = collect_country_geometry(&geometry);
        assert_eq!(lines.len(), 2);
        assert_eq!(rings.len(), 1);
    }

    #[test]
    fn duplicate_city_labels_keep_the_more_prominent_entry() {
        let city = |name: &str, rank| MapLabel {
            lat: -23.5,
            lon: -46.6,
            en: name.into(),
            pt: name.into(),
            kind: LabelKind::City,
            rank,
            big: false,
        };
        let mut labels = vec![city("São Paulo", 9), city("Sao Paulo", 1)];
        dedupe_city_labels(&mut labels);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].rank, 1);
    }
}
