//! Roads from GlobeCraft's own `roads` tile set (OpenMapTiles `transportation`
//! motorway to service, and `transportation_name` for the names, zoom 0–12;
//! `crate::map_data`), streamed for the view like the transport layer. Closer views
//! add the minor streets zoom 12 leaves out from Overpass (see `super::LiveRoads`).
use super::Road;
use crate::boundaries::osm_tiles::content::{TileBounds, TileKey, clip};
use crate::mvt::{self, GeomType};
use crate::vector_tiles::{Snapshot, Spec, TileStream, View};
use std::sync::Arc;

/// Above this roads are hidden: only motorways remain, lost in the imagery.
pub const MAX_ALTITUDE_KM: f64 = 1_000.0;
/// A name line this close (tile units, of 4,096) to a road's middle names it.
const NAME_MATCH_UNITS: f64 = 6.0;

static SPEC: Spec<RoadTile> = Spec {
    set: "roads",
    min_zoom: 4,
    max_zoom: 12,
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

/// The road tile layer, merged with the Overpass streets of close views.
#[derive(Default)]
pub struct RoadTiles {
    stream: Option<TileStream<RoadTile>>,
    /// The last merge: from these inputs, this list.
    merged: Option<(usize, usize, Arc<[Road]>)>,
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

    /// The roads to draw: the tiles' and `streets` (Overpass, close views). The same
    /// list is returned while neither changes, so its projection is reused.
    pub fn merged(&mut self, streets: Option<Arc<[Road]>>) -> Option<Arc<[Road]>> {
        let snapshot: Option<Arc<Snapshot<RoadTile>>> =
            self.stream.as_ref().and_then(TileStream::shared_snapshot);
        let key = (
            snapshot.as_ref().map_or(0, |s| Arc::as_ptr(s) as usize),
            streets
                .as_ref()
                .map_or(0, |s| Arc::as_ptr(s) as *const u8 as usize),
        );
        if let Some((tiles, streets, roads)) = &self.merged
            && (*tiles, *streets) == key
        {
            return (!roads.is_empty()).then(|| roads.clone());
        }
        let mut roads: Vec<Road> = snapshot
            .iter()
            .flat_map(|snapshot| snapshot.tiles.iter())
            .flat_map(|tile| tile.roads.iter().cloned())
            .collect();
        roads.extend(streets.iter().flat_map(|streets| streets.iter().cloned()));
        let roads: Arc<[Road]> = roads.into();
        self.merged = Some((key.0, key.1, roads.clone()));
        (!roads.is_empty()).then_some(roads)
    }
}

/// OpenStreetMap's `highway` value for an OpenMapTiles class, as Overpass roads use.
fn kind(class: &str, ramp: bool) -> String {
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
        format!("{base}_link")
    } else {
        base.to_owned()
    }
}

/// A road or name line of a tile, in tile units.
type TileLine = (String, Vec<(f64, f64)>);

fn decode(key: TileKey, bytes: &[u8]) -> Option<RoadTile> {
    let layers = mvt::decode(bytes, |name| {
        matches!(name, "transportation" | "transportation_name")
    })?;
    // Geometry from `transportation`; names from `transportation_name`'s lines.
    let mut lines: Vec<TileLine> = Vec::new();
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
            let kind = kind(&class, ramp);
            for part in &feature.parts {
                if layer.name == "transportation" {
                    lines.push((kind.clone(), part.clone()));
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
    for (kind, points) in lines {
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
                restricted: false,
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
