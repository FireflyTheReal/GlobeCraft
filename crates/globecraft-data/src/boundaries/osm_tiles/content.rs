//! What one OpenMapTiles tile holds for GlobeCraft: border lines by administrative
//! level, and place names, in latitude/longitude and clipped to the tile.
use super::super::{LabelKind, Line, MapLabel};
use super::super::{PLACE_HAMLET_RANK, PLACE_TOWN_RANK, PLACE_VILLAGE_RANK};
use super::compact::{self, Filtered, PlaceClass};
use std::f64::consts::PI;
use std::sync::Arc;

/// A tile of the Web Mercator pyramid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TileKey {
    pub(crate) z: u8,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

/// Latitude/longitude bounds of a tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TileBounds {
    pub(crate) south: f64,
    pub(crate) north: f64,
    pub(crate) west: f64,
    pub(crate) east: f64,
}

impl TileKey {
    pub(crate) fn tiles_per_side(z: u8) -> u32 {
        1 << z
    }
    /// The tile at `z` holding a point.
    pub(crate) fn at(z: u8, lat: f64, lon: f64) -> Self {
        let n = f64::from(Self::tiles_per_side(z));
        let lat = lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT).to_radians();
        let x = ((lon + 180.0) / 360.0 * n).floor();
        let y = ((1.0 - lat.tan().asinh() / PI) / 2.0 * n).floor();
        let last = n - 1.0;
        Self {
            z,
            x: x.clamp(0.0, last) as u32,
            y: y.clamp(0.0, last) as u32,
        }
    }
    /// The point of the tile at fractional tile coordinates (0..1 across, 0..1 down).
    pub(crate) fn point(self, across: f64, down: f64) -> (f64, f64) {
        let n = f64::from(Self::tiles_per_side(self.z));
        let lon = (f64::from(self.x) + across) / n * 360.0 - 180.0;
        let lat = (PI * (1.0 - 2.0 * (f64::from(self.y) + down) / n))
            .sinh()
            .atan()
            .to_degrees();
        (lat, lon)
    }
    pub(crate) fn bounds(self) -> TileBounds {
        let (north, west) = self.point(0.0, 0.0);
        let (south, east) = self.point(1.0, 1.0);
        TileBounds {
            south,
            north,
            west,
            east,
        }
    }
    pub(crate) fn parent(self) -> Option<Self> {
        (self.z > 0).then(|| Self {
            z: self.z - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }
    pub(super) fn children(self) -> [Self; 4] {
        let (z, x, y) = (self.z + 1, self.x * 2, self.y * 2);
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| Self {
            z,
            x: x + dx,
            y: y + dy,
        })
    }
}

/// Web Mercator ends here.
pub(super) const MAX_MERCATOR_LAT: f64 = 85.051_128_78;

/// Administrative levels (OpenStreetMap `admin_level`) drawn as state borders.
const STATE_LEVELS: std::ops::RangeInclusive<u8> = 3..=4;
/// Levels drawn as municipal lines: districts and counties down to municipalities.
/// A line carries the lowest level it belongs to, so a county border that is also a
/// municipal border has level 6 and must be drawn with the municipalities.
const MUNICIPAL_LEVELS: std::ops::RangeInclusive<u8> = 5..=8;
/// Islands found in tiles up to this zoom are named like towns, up to
/// `ISLAND_VILLAGE_MAX_ZOOM` like villages, beyond that like hamlets (OpenMapTiles
/// puts an island into tiles from a zoom that grows as its area shrinks).
const ISLAND_TOWN_MAX_ZOOM: u8 = 9;
const ISLAND_VILLAGE_MAX_ZOOM: u8 = 12;

/// A place's label rank: cities by OpenMapTiles' importance rank on the populated-place
/// scale (0–8, so the biggest show from far out), towns, villages and hamlets by
/// class, islands by the zoom from which their tile holds them.
fn place_rank(class: PlaceClass, importance: u8, z: u8) -> u8 {
    match class {
        PlaceClass::City if importance == 0 => CITY_UNRANKED_RANK,
        PlaceClass::City => (importance - 1).min(CITY_UNRANKED_RANK),
        PlaceClass::Town => PLACE_TOWN_RANK,
        PlaceClass::Village => PLACE_VILLAGE_RANK,
        PlaceClass::Hamlet => PLACE_HAMLET_RANK,
        PlaceClass::Island if z <= ISLAND_TOWN_MAX_ZOOM => PLACE_TOWN_RANK,
        PlaceClass::Island if z <= ISLAND_VILLAGE_MAX_ZOOM => PLACE_VILLAGE_RANK,
        PlaceClass::Island | PlaceClass::State => PLACE_HAMLET_RANK,
    }
}
/// The least important populated-place rank.
const CITY_UNRANKED_RANK: u8 = 8;

/// The borders and names of one tile (or of an ancestor clipped to it).
#[derive(Clone, Default)]
pub(super) struct TileContent {
    pub(super) country: Vec<Line>,
    /// Disputed lines and each claimant's claimed line (`claimed_by`), all drawn.
    pub(super) disputed: Vec<Line>,
    pub(super) state: Vec<Line>,
    pub(super) municipal: Vec<Line>,
    /// The sea's outline, drawn on the night side.
    pub(super) coast: Vec<Line>,
    pub(super) places: Vec<MapLabel>,
    /// Vertices held, for the memory budget.
    pub(super) points: usize,
}

impl TileContent {
    /// The drawable content of a tile's kept data (see `compact`).
    pub(super) fn from_filtered(key: TileKey, filtered: &Filtered) -> Self {
        let extent = f64::from(compact::EXTENT);
        let to_geo = |(x, y): (i32, i32)| key.point(f64::from(x) / extent, f64::from(y) / extent);
        let mut content = Self::default();
        let mut disputed_seen = std::collections::HashSet::new();
        for line in &filtered.lines {
            // Every side's line of a disputed border is drawn (the user found one
            // neutral line unreliable, 2026-10-25).
            let disputed = line.flags & (compact::DISPUTED | compact::CLAIM) != 0;
            // Claimants may carry identical copies of a line: draw it once.
            if line.level == 2 && disputed && !disputed_seen.insert(line.points.clone()) {
                continue;
            }
            let out = match line.level {
                compact::COAST_LEVEL => &mut content.coast,
                2 if disputed => &mut content.disputed,
                2 => &mut content.country,
                l if STATE_LEVELS.contains(&l) => &mut content.state,
                l if MUNICIPAL_LEVELS.contains(&l) => &mut content.municipal,
                _ => continue,
            };
            out.push(Arc::new(line.points.iter().copied().map(to_geo).collect()));
        }
        for place in &filtered.places {
            let (kind, rank) = match place.class {
                PlaceClass::State => (LabelKind::State, 0),
                place_class => (LabelKind::City, place_rank(place_class, place.rank, key.z)),
            };
            let (lat, lon) = to_geo((place.x, place.y));
            let en = place.name_en.clone().unwrap_or_else(|| place.name.clone());
            content.places.push(MapLabel {
                lat,
                lon,
                pt: place.name_pt.clone().unwrap_or_else(|| en.clone()),
                en,
                kind,
                rank,
                big: false,
                from_tiles: true,
            });
        }
        content.points = filtered.points();
        content
    }

    /// This content cut to `bounds`: an ancestor tile standing in for a missing one.
    pub(super) fn clipped(&self, bounds: TileBounds) -> Self {
        let lines = |lines: &[Line]| -> Vec<Line> {
            lines
                .iter()
                .flat_map(|line| clip(line, bounds))
                .map(Arc::new)
                .collect()
        };
        Self {
            country: lines(&self.country),
            disputed: lines(&self.disputed),
            state: lines(&self.state),
            municipal: lines(&self.municipal),
            coast: lines(&self.coast),
            places: self
                .places
                .iter()
                .filter(|p| {
                    (bounds.south..bounds.north).contains(&p.lat)
                        && (bounds.west..bounds.east).contains(&p.lon)
                })
                .cloned()
                .collect(),
            points: 0,
        }
    }
}

/// The parts of a polyline inside the bounds (Liang–Barsky per segment). Tiles
/// carry a small buffer beyond their edge; cutting every tile at its exact edge
/// leaves no stretch drawn twice by two neighbours.
pub(crate) fn clip(line: &[(f64, f64)], b: TileBounds) -> Vec<Vec<(f64, f64)>> {
    let mut parts: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    for pair in line.windows(2) {
        let (a, c) = (pair[0], pair[1]);
        let (dlat, dlon) = (c.0 - a.0, c.1 - a.1);
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        let mut inside = true;
        for (p, q) in [
            (-dlon, a.1 - b.west),
            (dlon, b.east - a.1),
            (-dlat, a.0 - b.south),
            (dlat, b.north - a.0),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    inside = false;
                    break;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
                if t0 > t1 {
                    inside = false;
                    break;
                }
            }
        }
        if !inside {
            if current.len() >= 2 {
                parts.push(std::mem::take(&mut current));
            }
            current.clear();
            continue;
        }
        let start = (a.0 + dlat * t0, a.1 + dlon * t0);
        let end = (a.0 + dlat * t1, a.1 + dlon * t1);
        if current.last() != Some(&start) {
            if current.len() >= 2 {
                parts.push(std::mem::take(&mut current));
            }
            current.clear();
            current.push(start);
        }
        current.push(end);
        if t1 < 1.0 {
            // Left the bounds inside this segment.
            parts.push(std::mem::take(&mut current));
        }
    }
    if current.len() >= 2 {
        parts.push(current);
    }
    parts.retain(|part| part.len() >= 2 && part.windows(2).any(|w| w[0] != w[1]));
    parts
}

#[cfg(test)]
mod tests {
    use super::super::mvt::encode::{TestFeature, TestValue, layer, tile};
    use super::*;

    #[test]
    fn every_disputed_and_claimed_line_is_drawn_once() {
        use compact::{CLAIM, COUNTRY_EDGE, DISPUTED, FilteredLine};
        let line = |flags, y| FilteredLine {
            level: 2,
            flags,
            points: vec![(0, y), (4096, y)],
        };
        let filtered = Filtered {
            lines: vec![
                line(COUNTRY_EDGE, 100),
                line(DISPUTED, 200),
                // Each claimant's copy of the same claim-only line.
                line(DISPUTED | CLAIM, 300),
                line(DISPUTED | CLAIM, 300),
                line(CLAIM, 400),
                // Where the disputed border actually runs: claimed, but a country edge.
                line(DISPUTED | CLAIM | COUNTRY_EDGE, 500),
            ],
            places: Vec::new(),
        };
        let content = TileContent::from_filtered(
            TileKey {
                z: 8,
                x: 180,
                y: 110,
            },
            &filtered,
        );
        assert_eq!(content.country.len(), 1);
        // Every disputed and claimed line, the claimants' copies once.
        assert_eq!(content.disputed.len(), 4);
    }

    #[test]
    fn tile_keys_round_trip_and_nest() {
        let key = TileKey::at(11, 49.0, 8.4);
        let b = key.bounds();
        assert!(b.south <= 49.0 && 49.0 < b.north && b.west <= 8.4 && 8.4 < b.east);
        assert_eq!(key.children().map(|c| c.parent()), [Some(key); 4]);
        assert_eq!(TileKey::at(0, 10.0, 10.0), TileKey { z: 0, x: 0, y: 0 });
        let world = TileKey { z: 0, x: 0, y: 0 }.bounds();
        assert!((world.north - MAX_MERCATOR_LAT).abs() < 1e-6 && world.west == -180.0);
    }

    #[test]
    fn clipping_keeps_inside_parts_and_cuts_at_the_edge() {
        let b = TileBounds {
            south: 0.0,
            north: 1.0,
            west: 0.0,
            east: 1.0,
        };
        let parts = clip(
            &[(0.5, -1.0), (0.5, 0.5), (2.0, 0.5), (0.5, 0.8), (0.5, 0.9)],
            b,
        );
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], vec![(0.5, 0.0), (0.5, 0.5), (1.0, 0.5)]);
        let (lat, lon) = parts[1][0];
        assert!((lat - 1.0).abs() < 1e-12 && (lon - 0.7).abs() < 1e-12);
        assert_eq!(parts[1][1..], [(0.5, 0.8), (0.5, 0.9)]);
        assert!(clip(&[(2.0, 2.0), (3.0, 3.0)], b).is_empty());
    }

    #[test]
    fn a_tile_gives_land_borders_by_level_and_named_places() {
        let line = |level: i64, maritime: i64| TestFeature {
            tags: vec![
                ("admin_level", TestValue::Int(level)),
                ("maritime", TestValue::Int(maritime)),
            ],
            kind: 2,
            parts: vec![vec![(-100, 1024), (2048, 1024), (5000, 1024)]],
        };
        let bytes = tile(&[
            layer(
                "boundary",
                4096,
                &[
                    line(2, 0),
                    line(4, 0),
                    line(6, 0),
                    line(8, 0),
                    line(10, 0),
                    line(2, 1),
                ],
            ),
            layer(
                "place",
                4096,
                &[
                    TestFeature {
                        tags: vec![
                            ("class", TestValue::Text("island")),
                            ("name", TestValue::Text("Fais")),
                        ],
                        kind: 1,
                        parts: vec![vec![(1000, 1000)]],
                    },
                    TestFeature {
                        tags: vec![
                            ("class", TestValue::Text("village")),
                            ("name", TestValue::Text("Outside")),
                        ],
                        kind: 1,
                        parts: vec![vec![(-20, 1000)]],
                    },
                ],
            ),
        ]);
        let key = TileKey {
            z: 11,
            x: 1071,
            y: 991,
        };
        let content = TileContent::from_filtered(key, &Filtered::from_mvt(&bytes).unwrap());
        assert_eq!(content.country.len(), 1);
        assert_eq!(content.state.len(), 1);
        assert_eq!(content.municipal.len(), 2);
        let b = key.bounds();
        let country = &content.country[0];
        assert!(
            (country[0].1 - b.west).abs() < 1e-9,
            "cut at the tile's west edge"
        );
        assert!((country.last().unwrap().1 - b.east).abs() < 1e-9);
        assert_eq!(content.places.len(), 1);
        assert_eq!(content.places[0].en, "Fais");
        assert_eq!(content.places[0].rank, PLACE_VILLAGE_RANK);
        // The tile standing in for a missing child keeps only that child's part.
        let top_left = content.clipped(key.children()[0].bounds());
        assert_eq!(top_left.country.len(), 1);
        assert_eq!(top_left.places.len(), 1, "Fais lies in the top-left child");
        let bottom_left = content.clipped(key.children()[2].bounds());
        assert!(bottom_left.country.is_empty() && bottom_left.places.is_empty());
    }

    /// Which disputed and claimed lines a real cache holds per zoom level, for a few
    /// disputed regions. Set `GLOBECRAFT_BORDER_CACHE` to an `osm_borders.sqlite`.
    #[test]
    #[ignore = "needs a border cache file"]
    fn disputed_lines_per_zoom_in_a_cache() {
        let Ok(path) = std::env::var("GLOBECRAFT_BORDER_CACHE") else {
            return;
        };
        let db = rusqlite::Connection::open(path).unwrap();
        let regions = [
            ("Kashmir", 32.0, 37.5, 72.5, 80.5),
            ("Arunachal", 26.5, 29.8, 91.5, 97.5),
            ("Western Sahara", 21.0, 28.0, -17.5, -8.5),
            ("Crimea", 44.3, 46.3, 32.4, 36.7),
        ];
        let mut query = db
            .prepare("SELECT z, x, y, data FROM tiles ORDER BY z")
            .unwrap();
        let rows: Vec<(u8, u32, u32, Vec<u8>)> = query
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for (name, south, north, west, east) in regions {
            println!("== {name}");
            for z in 0..=12_u8 {
                // (flags) -> (lines, km)
                let mut by_flags: std::collections::BTreeMap<u8, (usize, f64)> = Default::default();
                let mut tiles = 0;
                for (_, x, y, data) in rows.iter().filter(|r| r.0 == z) {
                    let key = TileKey { z, x: *x, y: *y };
                    let b = key.bounds();
                    if b.north < south || b.south > north || b.east < west || b.west > east {
                        continue;
                    }
                    tiles += 1;
                    let Some(filtered) = Filtered::decode(data) else {
                        continue;
                    };
                    let extent = f64::from(compact::EXTENT);
                    for line in filtered.lines.iter().filter(|l| l.level == 2) {
                        let geo: Vec<(f64, f64)> = line
                            .points
                            .iter()
                            .map(|&(x, y)| key.point(f64::from(x) / extent, f64::from(y) / extent))
                            .collect();
                        let inside = geo.iter().any(|&(lat, lon)| {
                            (south..north).contains(&lat) && (west..east).contains(&lon)
                        });
                        if !inside {
                            continue;
                        }
                        let km: f64 = geo
                            .windows(2)
                            .map(|w| {
                                let dlat = w[1].0 - w[0].0;
                                let dlon = (w[1].1 - w[0].1) * w[0].0.to_radians().cos();
                                111.2 * (dlat * dlat + dlon * dlon).sqrt()
                            })
                            .sum();
                        let entry = by_flags.entry(line.flags).or_default();
                        entry.0 += 1;
                        entry.1 += km;
                    }
                }
                if tiles == 0 {
                    continue;
                }
                let summary: Vec<String> = by_flags
                    .iter()
                    .map(|(flags, (n, km))| {
                        let mut name = String::new();
                        if flags & compact::DISPUTED != 0 {
                            name.push('D');
                        }
                        if flags & compact::CLAIM != 0 {
                            name.push('C');
                        }
                        if flags & compact::COUNTRY_EDGE != 0 {
                            name.push('E');
                        }
                        if name.is_empty() {
                            name.push('-');
                        }
                        format!("{name}: {n} lines {km:.0} km")
                    })
                    .collect();
                println!("z{z:<2} {tiles:>3} tiles  {}", summary.join(" | "));
            }
        }
    }
}
