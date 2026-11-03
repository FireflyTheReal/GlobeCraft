//! Places of interest, peaks, rivers and canals, water names and parks from
//! GlobeCraft's own `poi` tile set (OpenMapTiles `poi`, `mountain_peak`, `waterway`,
//! `water_name` and `park`, zoom 0–14; `crate::map_data`), streamed for the view like
//! the transport layer.
use crate::boundaries::osm_tiles::content::{TileBounds, TileKey, clip};
use crate::mvt::{self, GeomType};
use crate::vector_tiles::{Snapshot, Spec, TileStream, View};
use std::sync::Arc;

/// Above this the layers are hidden (sea and ocean names show up to here).
pub const MAX_ALTITUDE_KM: f64 = 3_000.0;

static SPEC: Spec<PoiTile> = Spec {
    set: "poi",
    bands: &[],
    min_zoom: 4,
    max_zoom: 14,
    skip_zoom: None,
    tiles_across_view: 3.0,
    max_tiles: 64,
    max_floor_tiles: 64,
    max_cached_tiles: 600,
    max_cached_bytes: crate::vector_tiles::MAX_VECTOR_CACHE_BYTES,
    size_bytes,
    decode,
};

/// What a labelled point is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointKind {
    /// A place of interest: a shop, museum, station, hospital…
    Poi,
    Peak,
    /// The name of a sea, lake, bay or strait.
    Water,
    Park,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoiPoint {
    pub kind: PointKind,
    pub lat: f64,
    pub lon: f64,
    pub name: String,
    /// OpenMapTiles `class` (for places of interest: shop, museum, railway…).
    pub class: String,
    /// OpenMapTiles `subclass`, the OpenStreetMap value the class groups (books,
    /// marina, bus_stop…); empty when absent.
    pub subclass: String,
    /// OpenMapTiles `rank`: 1 most important (0 when absent).
    pub rank: u32,
    /// Peaks: height in metres.
    pub elevation_m: Option<i32>,
}

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PoiLineKind {
    River,
    Canal,
    /// The outline of a park, nature reserve or protected area.
    Park,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoiLine {
    pub kind: PoiLineKind,
    pub name: Option<String>,
    /// Latitude/longitude, clipped to the tile.
    pub points: Vec<(f64, f64)>,
}

/// One tile's content.
#[derive(Debug, Default)]
pub struct PoiTile {
    pub points: Vec<PoiPoint>,
    pub lines: Vec<PoiLine>,
}

/// Estimated heap size of a decoded POI tile, for the stream's byte budget.
fn size_bytes(tile: &PoiTile) -> usize {
    let points: usize = tile
        .points
        .iter()
        .map(|point| {
            std::mem::size_of::<PoiPoint>()
                + point.name.capacity()
                + point.class.capacity()
                + point.subclass.capacity()
        })
        .sum();
    let lines: usize = tile
        .lines
        .iter()
        .map(|line| {
            std::mem::size_of::<PoiLine>()
                + line.name.as_ref().map_or(0, String::capacity)
                + line.points.capacity() * std::mem::size_of::<(f64, f64)>()
        })
        .sum();
    std::mem::size_of::<PoiTile>()
        + tile.points.capacity() * std::mem::size_of::<PoiPoint>()
        + tile.lines.capacity() * std::mem::size_of::<PoiLine>()
        + points
        + lines
}

pub type PoiSnapshot = Snapshot<PoiTile>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoiQuery {
    pub enabled: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_km: f64,
    pub tilt_deg: f64,
}

/// The tile layer behind the places of interest, peaks, waters and parks layers.
pub struct PoiTiles {
    stream: TileStream<PoiTile>,
}

impl Default for PoiTiles {
    fn default() -> Self {
        Self {
            stream: TileStream::new(&SPEC),
        }
    }
}

impl PoiTiles {
    pub fn shared_snapshot(&self) -> Option<Arc<PoiSnapshot>> {
        self.stream.shared_snapshot()
    }

    pub fn error(&self) -> Option<String> {
        self.stream.error()
    }

    pub fn loading(&self) -> bool {
        self.stream.loading()
    }

    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        query: PoiQuery,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        if !query.enabled || query.altitude_km > MAX_ALTITUDE_KM {
            self.stream.hide();
            return;
        }
        let view = View {
            latitude: query.latitude,
            longitude: query.longitude,
            altitude_km: query.altitude_km,
            tilt_deg: query.tilt_deg,
            floor: SPEC.min_zoom,
        };
        self.stream.update(runtime, view, repaint);
    }
}

fn decode(key: TileKey, bytes: &[u8]) -> Option<PoiTile> {
    let layers = mvt::decode(bytes, |name| {
        matches!(
            name,
            "poi" | "mountain_peak" | "waterway" | "water_name" | "park"
        )
    })?;
    let mut tile = PoiTile::default();
    for layer in &layers {
        let extent = f64::from(layer.extent.max(1));
        let to_degrees = |(x, y): (f64, f64)| key.point(x / extent, y / extent);
        let inside_tile =
            |&(x, y): &(f64, f64)| (0.0..extent).contains(&x) && (0.0..extent).contains(&y);
        let bounds = TileBounds {
            south: 0.0,
            north: extent,
            west: 0.0,
            east: extent,
        };
        for feature in &layer.features {
            let text = |tag: &str| {
                layer
                    .tag(feature, tag)
                    .and_then(mvt::Value::as_text)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            let number = |tag: &str| layer.tag(feature, tag).and_then(mvt::Value::as_number);
            let name = text("name_en")
                .or_else(|| text("name:latin"))
                .or_else(|| text("name"));
            let class = text("class").unwrap_or_default();
            let subclass = text("subclass").unwrap_or_default();
            let point_kind = match layer.name.as_str() {
                "poi" => Some(PointKind::Poi),
                "mountain_peak" => Some(PointKind::Peak),
                "water_name" => Some(PointKind::Water),
                "park" => Some(PointKind::Park),
                _ => None,
            };
            match feature.kind {
                GeomType::Point => {
                    let (Some(kind), Some(name)) = (point_kind, name) else {
                        continue;
                    };
                    for point in feature.parts.iter().flatten().filter(|p| inside_tile(p)) {
                        let (lat, lon) = to_degrees(*point);
                        tile.points.push(PoiPoint {
                            kind,
                            lat,
                            lon,
                            name: name.clone(),
                            class: class.clone(),
                            subclass: subclass.clone(),
                            rank: number("rank").map_or(0, |rank| rank.max(0.0) as u32),
                            elevation_m: number("ele").map(|ele| ele.round() as i32),
                        });
                    }
                }
                GeomType::Line | GeomType::Polygon => {
                    let line_kind = match (layer.name.as_str(), class.as_str()) {
                        ("waterway", "river") => PoiLineKind::River,
                        ("waterway", "canal") => PoiLineKind::Canal,
                        ("park", _) => PoiLineKind::Park,
                        ("water_name", _) => {
                            // Lake names may come as a line along the lake: the
                            // middle of it carries the name.
                            if let (Some(name), Some(part)) = (name, feature.parts.first())
                                && let Some(middle) = part.get(part.len() / 2)
                                && inside_tile(middle)
                            {
                                let (lat, lon) = to_degrees(*middle);
                                tile.points.push(PoiPoint {
                                    kind: PointKind::Water,
                                    lat,
                                    lon,
                                    name,
                                    class,
                                    subclass,
                                    rank: 0,
                                    elevation_m: None,
                                });
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    for part in &feature.parts {
                        // `clip` works on (lat, lon) pairs; tile units are (x, y).
                        let swapped: Vec<(f64, f64)> = part.iter().map(|&(x, y)| (y, x)).collect();
                        for run in clip(&swapped, bounds) {
                            if run.len() < 2 {
                                continue;
                            }
                            tile.lines.push(PoiLine {
                                kind: line_kind,
                                name: name.clone(),
                                points: run.into_iter().map(|(y, x)| to_degrees((x, y))).collect(),
                            });
                        }
                    }
                }
                GeomType::Unknown => {}
            }
        }
    }
    Some(tile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mvt::encode::{TestFeature, TestValue, layer, tile};

    #[test]
    fn tiles_yield_points_of_interest_peaks_waters_and_parks() {
        let feature = |tags: Vec<(&'static str, TestValue<'static>)>, kind, parts| TestFeature {
            tags,
            kind,
            parts,
        };
        let bytes = tile(&[
            layer(
                "poi",
                4096,
                &[
                    feature(
                        vec![
                            ("name", TestValue::Text("Deutsches Museum")),
                            ("class", TestValue::Text("museum")),
                            ("subclass", TestValue::Text("museum")),
                            ("rank", TestValue::Int(3)),
                        ],
                        1,
                        vec![vec![(1000, 1000)]],
                    ),
                    // Unnamed: no label to show.
                    feature(
                        vec![("class", TestValue::Text("shop"))],
                        1,
                        vec![vec![(1200, 1000)]],
                    ),
                ],
            ),
            layer(
                "mountain_peak",
                4096,
                &[feature(
                    vec![
                        ("name", TestValue::Text("Zugspitze")),
                        ("ele", TestValue::Int(2962)),
                    ],
                    1,
                    vec![vec![(2000, 2000)]],
                )],
            ),
            layer(
                "waterway",
                4096,
                &[feature(
                    vec![
                        ("class", TestValue::Text("river")),
                        ("name", TestValue::Text("Isar")),
                    ],
                    2,
                    vec![vec![(0, 3000), (4096, 3100)]],
                )],
            ),
            layer(
                "water_name",
                4096,
                &[feature(
                    vec![
                        ("class", TestValue::Text("lake")),
                        ("name", TestValue::Text("Starnberger See")),
                    ],
                    2,
                    vec![vec![(500, 500), (600, 600), (700, 700)]],
                )],
            ),
        ]);
        let tile = decode(
            TileKey {
                z: 12,
                x: 2180,
                y: 1420,
            },
            &bytes,
        )
        .unwrap();
        let points: Vec<_> = tile
            .points
            .iter()
            .map(|point| (point.kind, point.name.as_str(), point.elevation_m))
            .collect();
        assert_eq!(
            points,
            [
                (PointKind::Poi, "Deutsches Museum", None),
                (PointKind::Peak, "Zugspitze", Some(2962)),
                (PointKind::Water, "Starnberger See", None),
            ]
        );
        assert_eq!(tile.points[0].rank, 3);
        assert_eq!(tile.points[0].subclass, "museum");
        assert_eq!(tile.points[1].subclass, "", "peaks without a subclass");
        assert_eq!(tile.lines.len(), 1);
        assert_eq!(tile.lines[0].kind, PoiLineKind::River);
        assert_eq!(tile.lines[0].name.as_deref(), Some("Isar"));
    }
}

#[cfg(test)]
mod size_probe {
    /// Which layer fills the published `poi` tiles at zoom 14, in a city, farmland,
    /// forest, desert and on the sea: the numbers behind trimming the set.
    #[tokio::test]
    #[ignore = "network: reads GlobeCraft's published places tiles"]
    async fn published_poi_layer_sizes() {
        let template = crate::map_data::template("poi").await.unwrap();
        let archives = template
            .strip_prefix(crate::map_data::PMTILES_PREFIX)
            .unwrap()
            .to_owned();
        let places = [
            ("Munich", 48.14, 11.56),
            ("Bavarian farmland", 48.6, 12.4),
            ("Black Forest", 48.0, 8.2),
            ("Amazon", -3.5, -62.0),
            ("Sahara", 23.0, 12.0),
            ("Siberia", 62.0, 100.0),
            ("Kansas", 38.5, -98.0),
            ("Yellowstone", 44.6, -110.5),
            ("North Atlantic", 45.0, -35.0),
        ];
        for z in [12u8, 14] {
            let keys: Vec<_> = places
                .iter()
                .map(|&(_, lat, lon)| {
                    let key = crate::boundaries::osm_tiles::content::TileKey::at(z, lat, lon);
                    (key.z, key.x, key.y)
                })
                .collect();
            let parts = crate::map_data::tile_parts_many(&keys, &archives)
                .await
                .unwrap();
            for ((name, _, _), parts) in places.iter().zip(parts) {
                let compressed: usize = parts.iter().map(Vec::len).sum();
                let mut layers = std::collections::BTreeMap::new();
                for part in parts {
                    let bytes = crate::map_data::gunzip_if_needed(part).unwrap();
                    let mut reader = crate::mvt::Reader::new(&bytes);
                    while !reader.done() {
                        let key = reader.varint().unwrap();
                        if key >> 3 == 3 && key & 7 == 2 {
                            let layer = reader.bytes().unwrap();
                            let mut inner = crate::mvt::Reader::new(layer);
                            let mut name = String::new();
                            while !inner.done() {
                                let key = inner.varint().unwrap();
                                match key & 7 {
                                    2 => {
                                        let value = inner.bytes().unwrap();
                                        if key >> 3 == 1 {
                                            name = String::from_utf8_lossy(value).into_owned();
                                        }
                                    }
                                    0 => {
                                        inner.varint();
                                    }
                                    5 => {
                                        inner.take(4);
                                    }
                                    _ => {
                                        inner.take(8);
                                    }
                                }
                            }
                            *layers.entry(name).or_insert(0usize) += layer.len();
                        } else {
                            match key & 7 {
                                2 => {
                                    reader.bytes();
                                }
                                0 => {
                                    reader.varint();
                                }
                                _ => break,
                            }
                        }
                    }
                }
                println!("z{z} {name}: {compressed} B gzip, raw by layer {layers:?}");
            }
        }
    }
}
