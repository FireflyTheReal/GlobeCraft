//! Buildings from Overture Maps (roadmap 8.6): their outlines, courtyards and heights,
//! for drawing as blocks in close views. Overture merges OpenStreetMap (first) with
//! Microsoft's and Google's machine-traced footprints, Esri community maps and some
//! national data, and publishes the result each month as one PMTiles archive. No
//! account or key. Data © OpenStreetMap contributors and Overture Maps Foundation,
//! under the Open Database License (ODbL).
//!
//! Only zoom 14 carries heights, floor counts and colours (coarser levels are bare
//! outlines), so only zoom 14 is read, and only close to the ground.
use crate::boundaries::osm_tiles::content::TileKey;
use crate::mvt::{self, GeomType};
use crate::vector_tiles::{Snapshot, Source, Spec, TileFade, TileStream, View};
use std::sync::Arc;

/// Above this the buildings are hidden: a zoom-14 tile is 1–2.4 km wide, and higher
/// views would need more of them (each 140–880 KiB) than they show.
pub const MAX_ALTITUDE_KM: f64 = 4.0;
/// The level read: the only one with heights.
const ZOOM: u8 = 14;
/// A storey, for a building whose height is given only as a number of floors.
const FLOOR_HEIGHT_M: f32 = 3.0;
/// A building with neither a height nor floors: two storeys.
const DEFAULT_HEIGHT_M: f32 = 2.0 * FLOOR_HEIGHT_M;
/// Overture leaves out anything taller; a value above it is an error in the data.
const MAX_HEIGHT_M: f32 = 900.0;
/// Points closer than this (tile units, of 4,096) to the tile's edge are on it: the
/// building goes on in the next tile, and the edge between two such points is no wall.
const EDGE_UNITS: f64 = 0.5;
/// Rings smaller than this (square tile units) are slivers left by clipping.
const MIN_RING_AREA: f64 = 0.5;

/// Where the archives are: the bucket Overture publishes its tiles in, one folder per
/// release (`tiles/2026-10-21.0/buildings.pmtiles`). Only the last two or three
/// releases are kept, so the newest is looked up rather than written down.
const OVERTURE_TILES: &str = "https://overturemaps-extras-us-west-2.s3.us-west-2.amazonaws.com";
/// The bucket's folder list is a few hundred bytes.
const MAX_LISTING_BYTES: u64 = 64 * 1024;

/// How a building's height is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightSource {
    /// Measured or mapped in metres.
    Given,
    /// Its number of floors, at three metres each.
    Floors,
    /// Neither: two storeys.
    Assumed,
}

/// A building, or a part of one (a tower on a podium), as a block.
#[derive(Clone, Debug)]
pub struct Building {
    /// The outline and then its courtyards, as (latitude, longitude); each ring open
    /// (its first point is not repeated).
    pub rings: Vec<Vec<(f64, f64)>>,
    /// Per ring, whether the edge from each point to the next is a wall. An edge
    /// along the tile's edge is none: the building goes on in the next tile.
    pub walls: Vec<Vec<bool>>,
    /// The roof as triangles: indices into the rings' points, counted ring after ring.
    pub roof: Vec<u32>,
    /// Bottom and top above the ground, metres.
    pub base_m: f32,
    pub top_m: f32,
    pub height: HeightSource,
    /// Colours mapped in OpenStreetMap, when given as `#rrggbb` or `#rgb`.
    pub roof_colour: Option<[u8; 3]>,
    pub wall_colour: Option<[u8; 3]>,
    /// The fade of the tile it came from.
    pub fade: TileFade,
}

/// One tile's buildings.
#[derive(Debug, Default)]
pub struct BuildingTile {
    pub buildings: Vec<Building>,
}

static SPEC: Spec<BuildingTile> = Spec {
    set: "buildings",
    source: Source::Overture("buildings"),
    bands: &[],
    min_zoom: ZOOM,
    max_zoom: ZOOM,
    skip_zoom: None,
    tiles_across_view: 3.0,
    // The view's tiles nearest first; at 4 km a straight-down view needs about 9,
    // a tilted one more, and city tiles are large.
    max_tiles: 25,
    max_floor_tiles: 25,
    max_cached_tiles: 60,
    max_cached_bytes: 2 * crate::vector_tiles::MAX_VECTOR_CACHE_BYTES,
    size_bytes,
    decode,
};

/// Estimated heap size of a decoded tile, for the stream's byte budget.
fn size_bytes(tile: &BuildingTile) -> usize {
    let buildings: usize = tile
        .buildings
        .iter()
        .map(|building| {
            let rings: usize = building
                .rings
                .iter()
                .map(|ring| ring.capacity() * std::mem::size_of::<(f64, f64)>())
                .sum();
            let walls: usize = building.walls.iter().map(Vec::capacity).sum();
            std::mem::size_of::<Building>() + rings + walls + building.roof.capacity() * 4
        })
        .sum();
    std::mem::size_of::<BuildingTile>() + buildings
}

/// The building layer.
#[derive(Default)]
pub struct BuildingTiles {
    stream: Option<TileStream<BuildingTile>>,
    /// The last list: from this snapshot, these buildings.
    merged: Option<(usize, Arc<[Building]>)>,
}

impl BuildingTiles {
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        enabled: bool,
        view: crate::ViewQuery,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        let stream = self.stream.get_or_insert_with(|| TileStream::new(&SPEC));
        if !enabled || view.altitude_km > MAX_ALTITUDE_KM {
            stream.hide();
            return;
        }
        stream.update(
            runtime,
            View {
                at: view,
                floor: ZOOM,
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

    /// The buildings to draw. The same list is returned while the tiles do not
    /// change, so its projection is reused.
    pub fn buildings(&mut self) -> Option<Arc<[Building]>> {
        let snapshot: Option<Arc<Snapshot<BuildingTile>>> =
            self.stream.as_ref().and_then(TileStream::shared_snapshot);
        let key = snapshot.as_ref().map_or(0, |s| Arc::as_ptr(s) as usize);
        if let Some((built, buildings)) = &self.merged
            && *built == key
        {
            return (!buildings.is_empty()).then(|| buildings.clone());
        }
        let buildings: Arc<[Building]> = snapshot
            .iter()
            .flat_map(|snapshot| {
                snapshot
                    .tiles
                    .iter()
                    .enumerate()
                    .map(|(index, tile)| (tile, snapshot.fade(index)))
            })
            .flat_map(|(tile, fade)| {
                tile.buildings.iter().map(move |building| Building {
                    fade,
                    ..building.clone()
                })
            })
            .collect::<Vec<_>>()
            .into();
        self.merged = Some((key, buildings.clone()));
        (!buildings.is_empty()).then_some(buildings)
    }
}

/// The `pmtiles:` template of Overture's newest published `theme` archive.
pub(crate) async fn overture(theme: &str) -> Result<String, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let listing = crate::http::body(
        client.get(format!(
            "{OVERTURE_TILES}/?list-type=2&prefix=tiles/&delimiter=/"
        )),
        MAX_LISTING_BYTES,
    )
    .await?;
    let listing = String::from_utf8_lossy(&listing);
    let release = newest_release(&listing)
        .ok_or_else(|| crate::Error::msg("Overture's tile bucket lists no release"))?;
    Ok(format!(
        "{}{OVERTURE_TILES}/tiles/{release}/{theme}.pmtiles",
        crate::map_data::PMTILES_PREFIX
    ))
}

/// The newest release folder in an S3 folder listing (`<Prefix>tiles/2026-10-21.0/
/// </Prefix>`): releases are named by date and a counter, so the largest name is
/// the newest.
fn newest_release(listing: &str) -> Option<&str> {
    listing
        .split("<Prefix>tiles/")
        .skip(1)
        .filter_map(|rest| rest.split_once("/</Prefix>").map(|(name, _)| name))
        .filter(|name| {
            let bytes = name.as_bytes();
            bytes.len() >= 12
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes[10] == b'.'
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'.')
        })
        .max()
}

/// A colour mapped as `#rrggbb` or `#rgb` (names are left to the default colours).
fn colour(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    let digit = |index: usize| u8::from_str_radix(hex.get(index..index + 1)?, 16).ok();
    match hex.len() {
        6 => {
            let pair = |index: usize| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok();
            Some([pair(0)?, pair(2)?, pair(4)?])
        }
        3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
        _ => None,
    }
}

/// Twice a ring's signed area in tile units (y downward): positive for the
/// specification's outer rings, which run clockwise on screen.
fn signed_area(ring: &[(f64, f64)]) -> f64 {
    let mut sum = 0.0;
    for (index, &(x0, y0)) in ring.iter().enumerate() {
        let (x1, y1) = ring[(index + 1) % ring.len()];
        sum += x0 * y1 - x1 * y0;
    }
    sum
}

/// `ring` (open) cut to the square `0..=extent` (Sutherland–Hodgman, one side at a
/// time; a concave ring may keep zero-width bridges along the edge, which carry no
/// wall and no roof area).
fn clip_to_tile(ring: &[(f64, f64)], extent: f64) -> Vec<(f64, f64)> {
    type Inside = fn((f64, f64), f64) -> bool;
    type Cross = fn((f64, f64), (f64, f64), f64) -> (f64, f64);
    let along_x = |a: (f64, f64), b: (f64, f64), x: f64| {
        let t = (x - a.0) / (b.0 - a.0);
        (x, a.1 + (b.1 - a.1) * t)
    };
    let along_y = |a: (f64, f64), b: (f64, f64), y: f64| {
        let t = (y - a.1) / (b.1 - a.1);
        (a.0 + (b.0 - a.0) * t, y)
    };
    let sides: [(Inside, Cross, f64); 4] = [
        (|p, edge| p.0 >= edge, along_x, 0.0),
        (|p, edge| p.0 <= edge, along_x, extent),
        (|p, edge| p.1 >= edge, along_y, 0.0),
        (|p, edge| p.1 <= edge, along_y, extent),
    ];
    let mut points = ring.to_vec();
    for (inside, cross, edge) in sides {
        let input = std::mem::take(&mut points);
        for (index, &current) in input.iter().enumerate() {
            let previous = input[(index + input.len() - 1) % input.len()];
            match (inside(previous, edge), inside(current, edge)) {
                (true, true) => points.push(current),
                (true, false) => points.push(cross(previous, current, edge)),
                (false, true) => {
                    points.push(cross(previous, current, edge));
                    points.push(current);
                }
                (false, false) => {}
            }
        }
        if points.is_empty() {
            break;
        }
    }
    points
}

/// Whether the edge from `a` to `b` runs along the tile's edge.
fn on_tile_edge(a: (f64, f64), b: (f64, f64), extent: f64) -> bool {
    let near = |value: f64, edge: f64| (value - edge).abs() <= EDGE_UNITS;
    (near(a.0, 0.0) && near(b.0, 0.0))
        || (near(a.0, extent) && near(b.0, extent))
        || (near(a.1, 0.0) && near(b.1, 0.0))
        || (near(a.1, extent) && near(b.1, extent))
}

/// A polygon in tile units: its outline and courtyards, each open.
type TilePolygon = Vec<Vec<(f64, f64)>>;

/// The polygons of a feature's rings: an outer ring starts one, the inner rings
/// after it are its courtyards. Each ring is cut to the tile.
fn polygons(parts: &[Vec<(f64, f64)>], extent: f64) -> Vec<TilePolygon> {
    let mut polygons: Vec<TilePolygon> = Vec::new();
    for part in parts {
        // Rings repeat their first point (ClosePath).
        let open = match part.split_last() {
            Some((last, rest)) if Some(last) == part.first() => rest,
            _ => &part[..],
        };
        if open.len() < 3 {
            continue;
        }
        let outer = signed_area(open) > 0.0;
        let ring = clip_to_tile(open, extent);
        let area = signed_area(&ring).abs();
        if outer {
            // A clipped-away outline drops its courtyards too.
            polygons.push(if area >= MIN_RING_AREA {
                vec![ring]
            } else {
                Vec::new()
            });
        } else if area >= MIN_RING_AREA
            && let Some(polygon) = polygons.last_mut().filter(|polygon| !polygon.is_empty())
        {
            polygon.push(ring);
        }
    }
    polygons.retain(|polygon| !polygon.is_empty());
    polygons
}

pub(crate) fn decode(key: TileKey, bytes: &[u8]) -> Option<BuildingTile> {
    let layers = mvt::decode(bytes, |name| matches!(name, "building" | "building_part"))?;
    let mut tile = BuildingTile::default();
    let mut triangulator = earcut::Earcut::new();
    let mut triangles: Vec<u32> = Vec::new();
    for layer in &layers {
        let extent = f64::from(layer.extent.max(1));
        for feature in &layer.features {
            if feature.kind != GeomType::Polygon {
                continue;
            }
            let number = |tag: &str| {
                layer
                    .tag(feature, tag)
                    .and_then(mvt::Value::as_number)
                    .filter(|value| value.is_finite())
                    .map(|value| value as f32)
            };
            let text = |tag: &str| layer.tag(feature, tag).and_then(mvt::Value::as_text);
            // A building with parts is drawn as its parts; nothing underground shows.
            if number("is_underground").is_some_and(|flag| flag != 0.0)
                || (layer.name == "building" && number("has_parts").is_some_and(|flag| flag != 0.0))
            {
                continue;
            }
            let (top_m, height) = match (number("height"), number("num_floors")) {
                (Some(height), _) if height > 0.0 => (height, HeightSource::Given),
                (_, Some(floors)) if floors > 0.0 => {
                    (floors * FLOOR_HEIGHT_M, HeightSource::Floors)
                }
                _ => (DEFAULT_HEIGHT_M, HeightSource::Assumed),
            };
            let base_m = number("min_height")
                .or_else(|| number("min_floor").map(|floor| floor * FLOOR_HEIGHT_M))
                .unwrap_or(0.0)
                .max(0.0);
            if top_m > MAX_HEIGHT_M || top_m <= base_m {
                continue;
            }
            let (roof_colour, wall_colour) = (
                text("roof_color").and_then(colour),
                text("facade_color").and_then(colour),
            );
            for polygon in polygons(&feature.parts, extent) {
                let mut holes: Vec<u32> = Vec::new();
                let mut count = 0;
                for ring in &polygon[..polygon.len() - 1] {
                    count += ring.len() as u32;
                    holes.push(count);
                }
                triangulator.earcut(
                    polygon.iter().flatten().map(|&(x, y)| [x, y]),
                    &holes,
                    &mut triangles,
                );
                if triangles.is_empty() {
                    continue;
                }
                let walls = polygon
                    .iter()
                    .map(|ring| {
                        (0..ring.len())
                            .map(|index| {
                                !on_tile_edge(ring[index], ring[(index + 1) % ring.len()], extent)
                            })
                            .collect()
                    })
                    .collect();
                let rings = polygon
                    .iter()
                    .map(|ring| {
                        ring.iter()
                            .map(|&(x, y)| key.point(x / extent, y / extent))
                            .collect()
                    })
                    .collect();
                tile.buildings.push(Building {
                    rings,
                    walls,
                    roof: triangles.clone(),
                    base_m,
                    top_m,
                    height,
                    roof_colour,
                    wall_colour,
                    fade: TileFade::default(),
                });
            }
        }
    }
    Some(tile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mvt::encode::{TestFeature, TestValue, layer, tile};

    const KEY: TileKey = TileKey {
        z: ZOOM,
        x: 8718,
        y: 5685,
    };

    fn square(x: i32, y: i32, size: i32) -> Vec<(i32, i32)> {
        // Clockwise on screen (y down): an outer ring.
        vec![(x, y), (x + size, y), (x + size, y + size), (x, y + size)]
    }

    #[test]
    fn heights_come_from_metres_then_floors_then_two_storeys() {
        let features = [
            TestFeature {
                tags: vec![
                    ("height", TestValue::Int(42)),
                    ("num_floors", TestValue::Int(3)),
                ],
                kind: 3,
                parts: vec![square(100, 100, 50)],
            },
            TestFeature {
                tags: vec![
                    ("num_floors", TestValue::Int(5)),
                    ("min_height", TestValue::Int(3)),
                ],
                kind: 3,
                parts: vec![square(300, 100, 50)],
            },
            TestFeature {
                tags: vec![("roof_color", TestValue::Text("#c33"))],
                kind: 3,
                parts: vec![square(500, 100, 50)],
            },
            // A building drawn as its parts, and one underground: neither shows.
            TestFeature {
                tags: vec![("has_parts", TestValue::Int(1))],
                kind: 3,
                parts: vec![square(700, 100, 50)],
            },
            TestFeature {
                tags: vec![("is_underground", TestValue::Int(1))],
                kind: 3,
                parts: vec![square(900, 100, 50)],
            },
        ];
        let tile = decode(KEY, &tile(&[layer("building", 4096, &features)])).unwrap();
        let shown: Vec<_> = tile
            .buildings
            .iter()
            .map(|b| (b.base_m, b.top_m, b.height, b.roof_colour))
            .collect();
        assert_eq!(
            shown,
            [
                (0.0, 42.0, HeightSource::Given, None),
                (3.0, 15.0, HeightSource::Floors, None),
                (0.0, 6.0, HeightSource::Assumed, Some([0xcc, 0x33, 0x33])),
            ]
        );
        // A square's roof is two triangles, and all its edges are walls.
        assert_eq!(tile.buildings[0].roof.len(), 6);
        assert!(tile.buildings[0].walls[0].iter().all(|&wall| wall));
        // Points are on the tile, in latitude and longitude.
        let (lat, lon) = tile.buildings[0].rings[0][0];
        let (north, west) = KEY.point(0.0, 0.0);
        let (south, east) = KEY.point(1.0, 1.0);
        assert!((south..north).contains(&lat) && (west..east).contains(&lon));
    }

    #[test]
    fn courtyards_stay_open_and_tile_edges_are_no_walls() {
        // A block with a courtyard, and one that runs off the tile's west edge.
        let mut courtyard = square(150, 150, 100);
        courtyard.reverse();
        let features = [
            TestFeature {
                tags: vec![],
                kind: 3,
                parts: vec![square(100, 100, 200), courtyard],
            },
            TestFeature {
                tags: vec![],
                kind: 3,
                parts: vec![square(-50, 1000, 100)],
            },
        ];
        let tile = decode(KEY, &tile(&[layer("building", 4096, &features)])).unwrap();
        let block = &tile.buildings[0];
        assert_eq!(block.rings.len(), 2);
        // A square ring around a square hole: eight triangles, none over the hole.
        assert_eq!(block.roof.len(), 8 * 3);
        let cut = &tile.buildings[1];
        assert_eq!(cut.rings[0].len(), 4);
        // Exactly one edge lies on the tile's edge (x = 0) and is no wall.
        assert_eq!(cut.walls[0].iter().filter(|&&wall| !wall).count(), 1);
    }

    #[test]
    fn the_newest_release_is_read_from_the_bucket_listing() {
        let listing = "<ListBucketResult><CommonPrefixes><Prefix>tiles/2026-09-23.0/</Prefix>\
            </CommonPrefixes><CommonPrefixes><Prefix>tiles/2026-10-21.0/</Prefix></CommonPrefixes>\
            <CommonPrefixes><Prefix>tiles/2026-09-23.1/</Prefix></CommonPrefixes>\
            <CommonPrefixes><Prefix>tiles/latest/</Prefix></CommonPrefixes></ListBucketResult>";
        assert_eq!(newest_release(listing), Some("2026-10-21.0"));
        assert_eq!(newest_release("<ListBucketResult/>"), None);
    }

    /// Real tiles decode, and quickly: `GLOBECRAFT_BUILDING_TILES` names a folder of
    /// unpacked Overture tiles named `<place>-14-<x>-<y>.mvt`.
    #[test]
    #[ignore = "needs GLOBECRAFT_BUILDING_TILES"]
    fn overture_tiles_decode() {
        let folder = std::env::var_os("GLOBECRAFT_BUILDING_TILES").unwrap();
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let parts: Vec<&str> = name.split('-').collect();
            let [_, z, x, y] = parts[..] else { continue };
            let key = TileKey {
                z: z.parse().unwrap(),
                x: x.parse().unwrap(),
                y: y.parse().unwrap(),
            };
            let bytes = std::fs::read(&path).unwrap();
            let started = std::time::Instant::now();
            let tile = decode(key, &bytes).unwrap();
            let count =
                |source: HeightSource| tile.buildings.iter().filter(|b| b.height == source).count();
            println!(
                "{name}: {} blocks, {} roof triangles, {} given, {} floors, {} assumed, {:.0} KiB, {:?}",
                tile.buildings.len(),
                tile.buildings
                    .iter()
                    .map(|b| b.roof.len() / 3)
                    .sum::<usize>(),
                count(HeightSource::Given),
                count(HeightSource::Floors),
                count(HeightSource::Assumed),
                size_bytes(&tile) as f64 / 1024.0,
                started.elapsed()
            );
            assert!(!tile.buildings.is_empty());
        }
    }

    #[test]
    fn colours_read_hex_and_leave_names() {
        assert_eq!(colour("#A0b1C2"), Some([0xa0, 0xb1, 0xc2]));
        assert_eq!(colour(" #fff "), Some([255, 255, 255]));
        assert_eq!(colour("red"), None);
        assert_eq!(colour("#12345"), None);
        assert_eq!(colour("#gg0000"), None);
    }
}
