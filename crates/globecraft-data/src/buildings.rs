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
use crate::vector_tiles::{Snapshot, Source, Spec, TileStream, View};
use std::sync::Arc;

mod roof;
pub use roof::{ShapedRoof, WallBend};

/// Above this the buildings are hidden: a zoom-14 tile is 1–2.4 km wide, and higher
/// views would need more of them (each 140–880 KiB) than they show.
pub const MAX_ALTITUDE_KM: f64 = 6.0;
/// The level read: the only one with heights.
const ZOOM: u8 = 14;
/// Building tiles are wanted at least this far around the view (km), however low
/// the camera: about two tiles of level 14 in mid-latitudes.
const MIN_RADIUS_KM: f64 = 2.5;
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
/// The cells of a tile's height grid (`Heights`) are about this wide (metres).
const HEIGHT_CELL_M: f64 = 6.0;
/// A height grid's heights are kept in steps of this (metres).
const HEIGHT_STEP_M: f32 = 0.1;

/// Where the archives are: the bucket Overture publishes its tiles in, one folder per
/// release (`tiles/2026-10-21.0/buildings.pmtiles`). Only the last two or three
/// releases are kept, so the newest is looked up rather than written down.
const OVERTURE_TILES: &str = "https://overturemaps-extras-us-west-2.s3.us-west-2.amazonaws.com";
/// The bucket's folder list is a few hundred bytes.
const MAX_LISTING_BYTES: u64 = 64 * 1024;

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
    /// Colours mapped in OpenStreetMap, when given as `#rrggbb` or `#rgb`.
    pub roof_colour: Option<[u8; 3]>,
    pub wall_colour: Option<[u8; 3]>,
    /// A roof that is not flat (D3); `roof` is then empty.
    pub shaped: Option<Box<ShapedRoof>>,
}

/// One tile's buildings.
#[derive(Debug, Default)]
pub struct BuildingTile {
    pub buildings: Vec<Building>,
    /// The tallest block over each spot, for finding which walls and roofs other
    /// blocks hide from the sun.
    pub heights: Heights,
}

/// The tallest block over each cell of a tile (its top above its ground, metres): a
/// surface model the renderer marches toward the sun, so the light climbs the walls at
/// sunset and arrives on the tops first at sunrise (roadmap C3b). Cells are even in
/// latitude and longitude across the tile; one is about `HEIGHT_CELL_M` wide.
#[derive(Debug, Default)]
pub struct Heights {
    north: f64,
    west: f64,
    /// Cells per degree of latitude (south) and longitude (east).
    rows_per_deg: f64,
    cols_per_deg: f64,
    rows: usize,
    cols: usize,
    /// Row after row from the north-west corner, in `HEIGHT_STEP_M`.
    cells: Vec<u16>,
    /// The tallest block of the tile (metres).
    pub tallest_m: f32,
}

impl Heights {
    /// The tallest block over (`lat`, `lon`) (metres, 0 between blocks); none outside
    /// the tile.
    pub fn at(&self, lat: f64, lon: f64) -> Option<f32> {
        let row = (self.north - lat) * self.rows_per_deg;
        let col = (lon - self.west) * self.cols_per_deg;
        if !(0.0..self.rows as f64).contains(&row) || !(0.0..self.cols as f64).contains(&col) {
            return None;
        }
        let cell = self.cells[row as usize * self.cols + col as usize];
        Some(f32::from(cell) * HEIGHT_STEP_M)
    }

    /// The grid over the box from `north`-`west` to `south`-`east` (degrees) of
    /// `blocks`' outlines (their rings' courtyards left open).
    pub fn of<'a>(
        (north, west): (f64, f64),
        (south, east): (f64, f64),
        blocks: impl IntoIterator<Item = &'a Building>,
    ) -> Self {
        let middle = (0.5 * (north + south)).to_radians();
        let degree_m = globecraft_core::EARTH_RADIUS_M * std::f64::consts::PI / 180.0;
        let side = |span_m: f64| (span_m / HEIGHT_CELL_M).ceil().clamp(1.0, 1024.0) as usize;
        let (rows, cols) = (
            side((north - south) * degree_m),
            side((east - west) * degree_m * middle.cos()),
        );
        let (rows_per_deg, cols_per_deg) =
            (rows as f64 / (north - south), cols as f64 / (east - west));
        let mut cells = vec![0u16; rows * cols];
        let mut tallest_m = 0.0f32;
        for block in blocks {
            let Some(outline) = block.rings.first() else {
                continue;
            };
            tallest_m = tallest_m.max(block.top_m);
            let height = (block.top_m / HEIGHT_STEP_M)
                .round()
                .min(f32::from(u16::MAX)) as u16;
            let (mut south_most, mut north_most) = (f64::MAX, f64::MIN);
            let (mut west_most, mut east_most) = (f64::MAX, f64::MIN);
            for &(lat, lon) in outline {
                (south_most, north_most) = (south_most.min(lat), north_most.max(lat));
                (west_most, east_most) = (west_most.min(lon), east_most.max(lon));
            }
            // The cells whose middles fall within `low..high` (in cells from the edge).
            let within = |low: f64, high: f64, count: usize| {
                let first = (low - 0.5).ceil().max(0.0) as usize;
                let last = ((high - 0.5).floor() + 1.0).clamp(0.0, count as f64) as usize;
                first..last
            };
            let row_span = within(
                (north - north_most) * rows_per_deg,
                (north - south_most) * rows_per_deg,
                rows,
            );
            for row in row_span {
                let lat = north - (row as f64 + 0.5) / rows_per_deg;
                let col_span = within(
                    (west_most - west) * cols_per_deg,
                    (east_most - west) * cols_per_deg,
                    cols,
                );
                for col in col_span {
                    let lon = west + (col as f64 + 0.5) / cols_per_deg;
                    // Inside the outline and outside its courtyards (even-odd).
                    let crossings = block
                        .rings
                        .iter()
                        .filter(|ring| crosses_odd(ring, lon, lat))
                        .count();
                    if crossings % 2 == 1 {
                        let cell = &mut cells[row * cols + col];
                        *cell = (*cell).max(height);
                    }
                }
            }
        }
        Self {
            north,
            west,
            rows_per_deg,
            cols_per_deg,
            rows,
            cols,
            cells,
            tallest_m,
        }
    }
}

/// Whether a ray from (`x`, `y`) toward +x crosses `ring` an odd number of times;
/// `ring` holds (y, x) pairs, as (latitude, longitude).
fn crosses_odd(ring: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut odd = false;
    for (index, &(ay, ax)) in ring.iter().enumerate() {
        let (by, bx) = ring[(index + 1) % ring.len()];
        if (ay > y) != (by > y) && x < ax + (y - ay) / (by - ay) * (bx - ax) {
            odd = !odd;
        }
    }
    odd
}

static SPEC: Spec<BuildingTile> = Spec {
    set: "buildings",
    source: Source::Overture("buildings"),
    bands: &[],
    min_zoom: ZOOM,
    max_zoom: ZOOM,
    skip_zoom: None,
    tiles_across_view: 3.0,
    // Low down the view's own area holds a tile or two; the blocks around stay
    // loaded instead of fading away (user, 2026-11-17: below about 700 m).
    min_radius_km: MIN_RADIUS_KM,
    // The view's tiles nearest first; at 6 km a straight-down view needs about 36
    // in mid-latitudes, and city tiles are large.
    max_tiles: 36,
    max_floor_tiles: 36,
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
            let shaped = building.shaped.as_ref().map_or(0, |roof| {
                std::mem::size_of::<ShapedRoof>()
                    + roof.tops_m.capacity() * 4
                    + roof.bends.capacity() * std::mem::size_of::<WallBend>()
                    + roof.points.capacity() * std::mem::size_of::<[f32; 3]>()
                    + roof.triangles.capacity() * 4
            });
            std::mem::size_of::<Building>() + rings + walls + building.roof.capacity() * 4 + shaped
        })
        .sum();
    std::mem::size_of::<BuildingTile>() + buildings + tile.heights.cells.capacity() * 2
}

/// The building tiles around the view, as the projection reads them.
pub type BuildingSnapshot = Snapshot<BuildingTile>;

/// The building layer.
#[derive(Default)]
pub struct BuildingTiles {
    stream: Option<TileStream<BuildingTile>>,
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

    /// The tiles to draw. The same snapshot is returned while the tiles do not
    /// change, so its projection is reused.
    pub fn shared_snapshot(&self) -> Option<Arc<BuildingSnapshot>> {
        self.stream.as_ref().and_then(TileStream::shared_snapshot)
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
/// after it are its courtyards. Each ring is cut to the tile; each polygon comes with
/// its outline before the cut.
fn polygons(parts: &[Vec<(f64, f64)>], extent: f64) -> Vec<(TilePolygon, &[(f64, f64)])> {
    let mut polygons: Vec<(TilePolygon, &[(f64, f64)])> = Vec::new();
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
            polygons.push((
                if area >= MIN_RING_AREA {
                    vec![ring]
                } else {
                    Vec::new()
                },
                open,
            ));
        } else if area >= MIN_RING_AREA
            && let Some((polygon, _)) = polygons
                .last_mut()
                .filter(|(polygon, _)| !polygon.is_empty())
        {
            polygon.push(ring);
        }
    }
    polygons.retain(|(polygon, _)| !polygon.is_empty());
    polygons
}

pub(crate) fn decode(key: TileKey, bytes: &[u8]) -> Option<BuildingTile> {
    let layers = mvt::decode(bytes, |name| matches!(name, "building" | "building_part"))?;
    let mut tile = BuildingTile::default();
    let mut triangulator = earcut::Earcut::new();
    let mut triangles: Vec<u32> = Vec::new();
    // Tile units are as long across as down (Web Mercator) at the tile's middle.
    let metres_per_unit_at = |extent: f64| {
        let (lat, _) = key.point(0.5, 0.5);
        std::f64::consts::TAU * globecraft_core::EARTH_RADIUS_M * lat.to_radians().cos()
            / f64::from(1u32 << key.z)
            / extent
    };
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
            let top_m = match (number("height"), number("num_floors")) {
                (Some(height), _) if height > 0.0 => height,
                (_, Some(floors)) if floors > 0.0 => floors * FLOOR_HEIGHT_M,
                _ => DEFAULT_HEIGHT_M,
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
            let roof_tags = roof::RoofTags {
                shape: text("roof_shape"),
                height_m: number("roof_height"),
                direction_deg: number("roof_direction"),
                across: text("roof_orientation") == Some("across"),
            };
            for (polygon, outline) in polygons(&feature.parts, extent) {
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
                let walls: Vec<Vec<bool>> = polygon
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
                let shaped = roof::shaped(
                    &roof_tags,
                    &roof::Footprint {
                        outline,
                        rings: &polygon,
                        walls: &walls,
                        metres_per_unit: metres_per_unit_at(extent),
                    },
                    (base_m, top_m),
                    |(x, y)| key.point(x / extent, y / extent),
                );
                tile.buildings.push(Building {
                    rings,
                    walls,
                    // A shaped roof has its own triangles.
                    roof: if shaped.is_some() {
                        Vec::new()
                    } else {
                        triangles.clone()
                    },
                    base_m,
                    top_m,
                    roof_colour,
                    wall_colour,
                    shaped,
                });
            }
        }
    }
    tile.heights = Heights::of(key.point(0.0, 0.0), key.point(1.0, 1.0), &tile.buildings);
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
    fn the_blocks_around_stay_wanted_low_down() {
        // 300 m over Munich, tilted: the view's own area is a few hundred metres, but
        // the tiles 2 km around are wanted (at least five by five there).
        let view = crate::vector_tiles::View {
            at: crate::ViewQuery {
                latitude: 48.137,
                longitude: 11.575,
                altitude_km: 0.3,
                tilt_deg: 70.0,
                heading_deg: 0.0,
            },
            floor: ZOOM,
        };
        let (zoom, tiles) = crate::vector_tiles::wanted(&SPEC, view);
        assert_eq!(zoom, ZOOM);
        assert!(tiles.len() >= 9, "{} tiles", tiles.len());
        let here = TileKey::at(ZOOM, 48.137, 11.575);
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let next = (i64::from(here.x) + dx, i64::from(here.y) + dy);
            assert!(
                tiles
                    .iter()
                    .any(|t| (i64::from(t.x), i64::from(t.y)) == next),
                "the tile beside, {dx},{dy}"
            );
        }
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
            .map(|b| (b.base_m, b.top_m, b.roof_colour))
            .collect();
        assert_eq!(
            shown,
            [
                (0.0, 42.0, None),
                (3.0, 15.0, None),
                (0.0, 6.0, Some([0xcc, 0x33, 0x33])),
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
    fn roof_shapes_are_read_with_their_height_and_direction() {
        let features = [
            TestFeature {
                tags: vec![
                    ("height", TestValue::Int(12)),
                    ("roof_shape", TestValue::Text("gabled")),
                    ("roof_height", TestValue::Int(4)),
                ],
                kind: 3,
                parts: vec![square(100, 100, 50)],
            },
            TestFeature {
                tags: vec![
                    ("height", TestValue::Int(12)),
                    ("roof_shape", TestValue::Text("skillion")),
                    ("roof_height", TestValue::Int(3)),
                    ("roof_direction", TestValue::Int(180)),
                ],
                kind: 3,
                parts: vec![square(300, 100, 50)],
            },
            TestFeature {
                tags: vec![("roof_shape", TestValue::Text("flat"))],
                kind: 3,
                parts: vec![square(500, 100, 50)],
            },
        ];
        let tile = decode(KEY, &tile(&[layer("building", 4096, &features)])).unwrap();
        let gable = tile.buildings[0].shaped.as_deref().unwrap();
        assert!(gable.tops_m.iter().all(|&top| (top - 8.0).abs() < 1e-4));
        assert_eq!(gable.bends.len(), 2);
        assert!(
            gable
                .bends
                .iter()
                .all(|bend| (bend.height_m - 12.0).abs() < 1e-4)
        );
        // Facing south: the southern corners (the larger tile y) are the low ones.
        let skillion = tile.buildings[1].shaped.as_deref().unwrap();
        let south = tile.buildings[1].rings[0]
            .iter()
            .map(|&(lat, _)| lat)
            .fold(f64::INFINITY, f64::min);
        for (&(lat, _), &top) in tile.buildings[1].rings[0].iter().zip(&skillion.tops_m) {
            let expected = if lat == south { 9.0 } else { 12.0 };
            assert!((top - expected).abs() < 1e-3, "{lat}: {top}");
        }
        assert!(tile.buildings[2].shaped.is_none());
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
    fn the_height_grid_holds_each_block_and_leaves_courtyards_open() {
        let mut courtyard = square(1200, 1200, 800);
        courtyard.reverse();
        let features = [
            TestFeature {
                tags: vec![("height", TestValue::Int(40))],
                kind: 3,
                parts: vec![square(1000, 1000, 1200), courtyard],
            },
            TestFeature {
                tags: vec![("height", TestValue::Int(120))],
                kind: 3,
                parts: vec![square(3000, 3000, 400)],
            },
        ];
        let tile = decode(KEY, &tile(&[layer("building", 4096, &features)])).unwrap();
        let at = |x: f64, y: f64| {
            let (lat, lon) = KEY.point(x / 4096.0, y / 4096.0);
            tile.heights.at(lat, lon)
        };
        assert_eq!(at(1100.0, 1100.0), Some(40.0));
        assert_eq!(at(1600.0, 1600.0), Some(0.0), "the courtyard is open");
        assert_eq!(at(3200.0, 3200.0), Some(120.0));
        assert_eq!(at(2500.0, 500.0), Some(0.0));
        assert_eq!(tile.heights.tallest_m, 120.0);
        // Off the tile: none.
        let (north, west) = KEY.point(0.0, 0.0);
        assert_eq!(tile.heights.at(north + 0.01, west), None);
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
            let shaped: Vec<&ShapedRoof> = tile
                .buildings
                .iter()
                .filter_map(|b| b.shaped.as_deref())
                .collect();
            println!(
                "{name}: {} blocks, {} roof triangles, {} shaped roofs with {} triangles, {:.0} KiB, {:?}",
                tile.buildings.len(),
                tile.buildings
                    .iter()
                    .map(|b| b.roof.len() / 3)
                    .sum::<usize>(),
                shaped.len(),
                shaped
                    .iter()
                    .map(|roof| roof.triangles.len() / 3)
                    .sum::<usize>(),
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
