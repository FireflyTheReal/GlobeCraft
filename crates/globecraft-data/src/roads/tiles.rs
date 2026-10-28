//! Roads from GlobeCraft's own road tile sets (OpenMapTiles `transportation` motorway
//! to service and their roadworks, and `transportation_name` for the names;
//! `crate::map_data`): `roads` holds zoom 0–12, `roads13` and `roads14` the closest
//! levels. Streamed for the view like the transport layer.
use super::Road;
use crate::boundaries::osm_tiles::content::{TileBounds, TileKey, clip};
use crate::mvt::{self, GeomType};
use crate::vector_tiles::{Snapshot, Spec, TileStream, View};
use std::sync::Arc;

/// What the published tiles hold (read around Munich, 2026-10-28): motorways from
/// zoom 4, trunk roads from 6, primary from 8, secondary and the links from 10,
/// tertiary from 11, residential streets at 12. Names come with motorways and trunk
/// roads from zoom 6 and with most roads at zoom 12.
///
/// Zoom 13 adds service roads and the remaining residential streets, zoom 14 every
/// street (the bands are their own runs; see `map-data.yml`).
///
/// Above this roads are hidden: only motorways remain, lost in the imagery.
pub const MAX_ALTITUDE_KM: f64 = 1_000.0;
/// A name line this close (tile units, of 4,096) to a road's middle names it.
const NAME_MATCH_UNITS: f64 = 6.0;

static SPEC: Spec<RoadTile> = Spec {
    set: "roads",
    bands: &["roads13", "roads14"],
    min_zoom: 4,
    max_zoom: 14,
    skip_zoom: None,
    tiles_across_view: 3.0,
    max_tiles: 64,
    max_floor_tiles: 64,
    max_cached_tiles: 500,
    decode,
};

/// One tile's roads, in latitude/longitude, clipped to the tile.
#[derive(Debug, Default)]
pub struct RoadTile {
    pub roads: Vec<Road>,
}

/// The road tile layer.
#[derive(Default)]
pub struct RoadTiles {
    stream: Option<TileStream<RoadTile>>,
    /// The last list: from this snapshot, these roads.
    merged: Option<(usize, Arc<[Road]>)>,
}

impl RoadTiles {
    /// `view`: latitude, longitude, altitude (km) and tilt (degrees).
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        enabled: bool,
        view: (f64, f64, f64, f64),
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        let stream = self.stream.get_or_insert_with(|| TileStream::new(&SPEC));
        let (latitude, longitude, altitude_km, tilt_deg) = view;
        if !enabled || altitude_km > MAX_ALTITUDE_KM {
            stream.hide();
            return;
        }
        stream.update(
            runtime,
            View {
                latitude,
                longitude,
                altitude_km,
                tilt_deg,
                floor: SPEC.min_zoom,
            },
            repaint,
        );
    }

    pub fn error(&self) -> Option<String> {
        self.stream.as_ref()?.error()
    }

    pub fn loading(&self) -> bool {
        self.stream.as_ref().is_some_and(TileStream::loading)
    }

    /// The roads to draw. The same list is returned while the tiles do not change,
    /// so its projection is reused.
    pub fn roads(&mut self) -> Option<Arc<[Road]>> {
        let snapshot: Option<Arc<Snapshot<RoadTile>>> =
            self.stream.as_ref().and_then(TileStream::shared_snapshot);
        let key = snapshot.as_ref().map_or(0, |s| Arc::as_ptr(s) as usize);
        if let Some((built, roads)) = &self.merged
            && *built == key
        {
            return (!roads.is_empty()).then(|| roads.clone());
        }
        let roads: Arc<[Road]> = snapshot
            .iter()
            .flat_map(|snapshot| snapshot.tiles.iter())
            .flat_map(|tile| tile.roads.iter().cloned())
            .collect::<Vec<_>>()
            .into();
        self.merged = Some((key, roads.clone()));
        (!roads.is_empty()).then_some(roads)
    }
}

/// OpenStreetMap's `highway` value for an OpenMapTiles class (roadworks drop their
/// `_construction`), and whether the road is under construction.
fn kind(class: &str, ramp: bool) -> (String, bool) {
    let (class, construction) = match class.strip_suffix("_construction") {
        Some(class) => (class, true),
        None => (class, false),
    };
    let base = match class {
        "minor" => "residential",
        other => other,
    };
    if ramp
        && matches!(
            base,
            "motorway" | "trunk" | "primary" | "secondary" | "tertiary"
        )
    {
        (format!("{base}_link"), construction)
    } else {
        (base.to_owned(), construction)
    }
}

/// A line in tile units.
type Points = Vec<(f64, f64)>;
/// A road or name line of a tile, in tile units.
type TileLine = (String, Points);

fn decode(key: TileKey, bytes: &[u8]) -> Option<RoadTile> {
    let layers = mvt::decode(bytes, |name| {
        matches!(name, "transportation" | "transportation_name")
    })?;
    // Geometry from `transportation`; names from `transportation_name`'s lines.
    // (kind, under construction) and the line.
    let mut lines: Vec<((String, bool), Points)> = Vec::new();
    let mut names: Vec<(String, TileLine)> = Vec::new();
    let mut extent = 4096.0;
    for layer in &layers {
        extent = f64::from(layer.extent.max(1));
        for feature in &layer.features {
            if feature.kind != GeomType::Line {
                continue;
            }
            let text = |tag: &str| {
                layer
                    .tag(feature, tag)
                    .and_then(mvt::Value::as_text)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            let Some(class) = text("class") else {
                continue;
            };
            let ramp = layer
                .tag(feature, "ramp")
                .and_then(mvt::Value::as_number)
                .is_some_and(|ramp| ramp != 0.0);
            let (kind, construction) = kind(&class, ramp);
            for part in &feature.parts {
                if layer.name == "transportation" {
                    lines.push(((kind.clone(), construction), part.clone()));
                } else if let Some(name) = text("name_en")
                    .or_else(|| text("name:latin"))
                    .or_else(|| text("name"))
                    .or_else(|| text("ref"))
                {
                    names.push((name, (kind.clone(), part.clone())));
                }
            }
        }
    }
    let inside = TileBounds {
        south: 0.0,
        north: extent,
        west: 0.0,
        east: extent,
    };
    let mut tile = RoadTile::default();
    for ((kind, construction), points) in lines {
        if points.len() < 2 {
            continue;
        }
        let name = name_for(&kind, &points, &names).unwrap_or_default();
        // `clip` works on (lat, lon) pairs; tile units are (x, y).
        let swapped: Vec<(f64, f64)> = points.iter().map(|&(x, y)| (y, x)).collect();
        for run in clip(&swapped, inside) {
            if run.len() < 2 {
                continue;
            }
            tile.roads.push(Road {
                name: name.clone(),
                kind: kind.clone(),
                restricted: construction,
                points: run
                    .into_iter()
                    .map(|(y, x)| key.point(x / extent, y / extent))
                    .collect(),
            });
        }
    }
    Some(tile)
}

/// The name of the name line of the same kind running through the middle of `points`.
fn name_for(kind: &str, points: &[(f64, f64)], names: &[(String, TileLine)]) -> Option<String> {
    let middle = {
        let a = points[(points.len() - 1) / 2];
        let b = points[points.len() / 2];
        ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
    };
    let mut best: Option<(f64, &str)> = None;
    for (name, (name_kind, line)) in names {
        if name_kind != kind {
            continue;
        }
        for pair in line.windows(2) {
            let distance = segment_distance(middle, pair[0], pair[1]);
            if distance <= NAME_MATCH_UNITS && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, name));
            }
        }
    }
    best.map(|(_, name)| name.to_owned())
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a.0 + dx * t, a.1 + dy * t);
    ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mvt::encode::{TestFeature, TestValue, layer, tile};

    #[test]
    fn roadworks_keep_their_road_kind() {
        assert_eq!(
            kind("motorway_construction", false),
            ("motorway".into(), true)
        );
        assert_eq!(kind("minor", false), ("residential".into(), false));
        assert_eq!(kind("trunk", true), ("trunk_link".into(), false));
    }

    #[test]
    fn roads_take_the_name_of_the_name_line_along_them() {
        let feature = |tags: Vec<(&'static str, TestValue<'static>)>, parts| TestFeature {
            tags,
            kind: 2,
            parts,
        };
        let bytes = tile(&[
            layer(
                "transportation",
                4096,
                &[
                    feature(
                        vec![("class", TestValue::Text("motorway"))],
                        vec![vec![(0, 1000), (4096, 1000)]],
                    ),
                    feature(
                        vec![
                            ("class", TestValue::Text("primary")),
                            ("ramp", TestValue::Int(1)),
                        ],
                        vec![vec![(100, 3000), (300, 3200)]],
                    ),
                    feature(
                        vec![("class", TestValue::Text("minor"))],
                        vec![vec![(0, 2000), (4096, 2000)]],
                    ),
                ],
            ),
            layer(
                "transportation_name",
                4096,
                &[
                    feature(
                        vec![
                            ("class", TestValue::Text("motorway")),
                            ("name", TestValue::Text("A 9")),
                        ],
                        vec![vec![(0, 1002), (4096, 1002)]],
                    ),
                    // Same place, other class: not this road's name.
                    feature(
                        vec![
                            ("class", TestValue::Text("primary")),
                            ("name", TestValue::Text("Somewhere")),
                        ],
                        vec![vec![(0, 2001), (4096, 2001)]],
                    ),
                ],
            ),
        ]);
        let tile = decode(
            TileKey {
                z: 12,
                x: 2176,
                y: 1420,
            },
            &bytes,
        )
        .unwrap();
        let roads: Vec<_> = tile
            .roads
            .iter()
            .map(|road| (road.kind.as_str(), road.name.as_str()))
            .collect();
        assert_eq!(
            roads,
            [
                ("motorway", "A 9"),
                ("primary_link", ""),
                ("residential", "")
            ]
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The published road tiles around Munich, read as the app reads them: which road
    /// kinds each level holds, and the motorways' names.
    #[tokio::test]
    #[ignore = "network: reads GlobeCraft's published road tiles"]
    async fn published_road_tiles_hold_named_roads() {
        let template = crate::map_data::banded_template(SPEC.set, SPEC.bands)
            .await
            .unwrap();
        let mut motorway_names = BTreeMap::new();
        for z in [4, 6, 8, 10, 11, 12, 13, 14] {
            let key = TileKey::at(z, 48.2, 11.6);
            let tile = crate::vector_tiles::fetch(&SPEC, key, &template)
                .await
                .unwrap();
            let mut kinds: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for road in &tile.roads {
                let entry = kinds.entry(road.kind.as_str()).or_default();
                entry.0 += 1;
                entry.1 += usize::from(!road.name.is_empty());
                if road.kind == "motorway" && !road.name.is_empty() {
                    *motorway_names.entry(road.name.clone()).or_insert(0) += 1;
                }
            }
            println!("z{z}: (count, named) {kinds:?}");
        }
        println!("motorway names: {motorway_names:?}");
        assert!(motorway_names.keys().any(|name| name.contains('9')));
    }
}
