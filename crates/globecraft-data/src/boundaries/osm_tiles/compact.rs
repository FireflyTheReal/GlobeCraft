//! What GlobeCraft keeps of an OpenMapTiles tile, and how it is stored on disk.
//!
//! A downloaded tile is a whole basemap tile (roads, buildings, land use); only its land
//! border lines, the sea's outline (coast) and place names are kept. Lines are cut at the tile's edge, so edge
//! points of neighbouring tiles coincide, and stored with their raw `admin_level` and
//! disputed flags, so a later change in how levels are drawn needs no new download.
//! Coordinates are integer tile units, stored as zig-zag varint deltas.
use super::mvt::{self, GeomType, Reader, Value};

/// Tile units across a tile in this format (OpenMapTiles' usual extent).
pub(super) const EXTENT: i32 = 4096;
/// Bumped when the stored layout changes; rows of other versions are ignored.
/// 4: coast lines; state names keep their class on disk.
pub(super) const FORMAT_VERSION: u8 = 4;
/// `FilteredLine::level` of a coast line: the outline of the `water` layer's ocean,
/// without the tile's edges. Administrative levels start at 1.
pub(super) const COAST_LEVEL: u8 = 0;

/// `FilteredLine::flags`: OpenMapTiles marks the line `disputed=1`.
pub(super) const DISPUTED: u8 = 1;
/// `FilteredLine::flags`: the line belongs to a claim (`claimed_by` is set).
pub(super) const CLAIM: u8 = 2;
/// `FilteredLine::flags`: the line is part of a country's actual border (OpenMapTiles
/// names the countries either side, `adm0_l`/`adm0_r`). OpenMapTiles merges each
/// OpenStreetMap way into one feature, so the line where a disputed border actually
/// runs carries `claimed_by` too; this flag tells it from a claim-only line.
pub(super) const COUNTRY_EDGE: u8 = 4;

/// Place kinds kept from the `place` layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlaceClass {
    City,
    Town,
    Village,
    Hamlet,
    Island,
    /// States and provinces.
    State,
}

impl PlaceClass {
    const ALL: [Self; 6] = [
        Self::City,
        Self::Town,
        Self::Village,
        Self::Hamlet,
        Self::Island,
        Self::State,
    ];
    fn parse(class: &str) -> Option<Self> {
        Some(match class {
            "city" => Self::City,
            "town" => Self::Town,
            "village" => Self::Village,
            "hamlet" => Self::Hamlet,
            "island" => Self::Island,
            "state" | "province" => Self::State,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FilteredLine {
    /// OpenStreetMap `admin_level`: the lowest level this line bounds.
    pub(super) level: u8,
    pub(super) flags: u8,
    /// Tile units, 0..=`EXTENT` (cut at the tile's edge).
    pub(super) points: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FilteredPlace {
    pub(super) class: PlaceClass,
    /// Tile units, inside the tile.
    pub(super) x: i32,
    pub(super) y: i32,
    /// OpenMapTiles' importance rank (1 most important; 0 when absent).
    pub(super) rank: u8,
    pub(super) name: String,
    pub(super) name_en: Option<String>,
    pub(super) name_pt: Option<String>,
}

/// The kept part of one tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Filtered {
    pub(super) lines: Vec<FilteredLine>,
    pub(super) places: Vec<FilteredPlace>,
}

impl Filtered {
    /// Keeps the land borders and places of a vector tile. `None` when it is malformed.
    pub(super) fn from_mvt(bytes: &[u8]) -> Option<Self> {
        let layers = mvt::decode(bytes, |name| {
            name == "boundary" || name == "place" || name == "water"
        })?;
        let mut out = Self::default();
        for layer in &layers {
            let scale = f64::from(EXTENT) / f64::from(layer.extent.max(1));
            for feature in &layer.features {
                let number = |tag: &str| layer.tag(feature, tag).and_then(Value::as_number);
                let text = |tag: &str| {
                    layer
                        .tag(feature, tag)
                        .and_then(Value::as_text)
                        .filter(|t| !t.trim().is_empty())
                };
                match (layer.name.as_str(), feature.kind) {
                    ("boundary", GeomType::Line) => {
                        if number("maritime").unwrap_or(0.0) != 0.0 {
                            continue;
                        }
                        let Some(level) = number("admin_level")
                            .filter(|l| (1.0..=15.0).contains(l))
                            .map(|l| l as u8)
                        else {
                            continue;
                        };
                        let mut flags = 0;
                        if number("disputed").unwrap_or(0.0) != 0.0 {
                            flags |= DISPUTED;
                        }
                        if text("claimed_by").is_some() {
                            flags |= CLAIM;
                        }
                        if text("adm0_l").is_some() || text("adm0_r").is_some() {
                            flags |= COUNTRY_EDGE;
                        }
                        for part in &feature.parts {
                            let scaled: Vec<(f64, f64)> =
                                part.iter().map(|&(x, y)| (x * scale, y * scale)).collect();
                            for clipped in clip_to_tile(&scaled) {
                                out.lines.push(FilteredLine {
                                    level,
                                    flags,
                                    points: clipped,
                                });
                            }
                        }
                    }
                    ("water", GeomType::Polygon) if text("class") == Some("ocean") => {
                        for ring in &feature.parts {
                            let scaled: Vec<(f64, f64)> =
                                ring.iter().map(|&(x, y)| (x * scale, y * scale)).collect();
                            for clipped in clip_to_tile(&scaled) {
                                for points in without_tile_edges(clipped) {
                                    out.lines.push(FilteredLine {
                                        level: COAST_LEVEL,
                                        flags: 0,
                                        points,
                                    });
                                }
                            }
                        }
                    }
                    ("place", GeomType::Point) => {
                        let (Some(class), Some(name)) =
                            (text("class").and_then(PlaceClass::parse), text("name"))
                        else {
                            continue;
                        };
                        // English, else OpenMapTiles' international (usually Latin)
                        // spelling, so a name in another script has a readable form.
                        let name_en = text("name:en")
                            .or_else(|| text("name_en"))
                            .or_else(|| text("name_int"))
                            .or_else(|| text("name:latin"));
                        for part in &feature.parts {
                            let Some(&(px, py)) = part.first() else {
                                continue;
                            };
                            let (x, y) = ((px * scale).round() as i32, (py * scale).round() as i32);
                            // Points in the tile's buffer belong to the neighbour.
                            if !(0..EXTENT).contains(&x) || !(0..EXTENT).contains(&y) {
                                continue;
                            }
                            out.places.push(FilteredPlace {
                                class,
                                x,
                                y,
                                rank: number("rank").map_or(0, |r| r.clamp(0.0, 255.0) as u8),
                                name: name.to_owned(),
                                name_en: name_en.filter(|en| *en != name).map(str::to_owned),
                                name_pt: text("name:pt").map(str::to_owned),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        Some(out)
    }

    /// Vertices held, for the memory budget.
    pub(super) fn points(&self) -> usize {
        self.lines.iter().map(|l| l.points.len()).sum::<usize>() + self.places.len()
    }

    pub(super) fn encode(&self) -> Vec<u8> {
        let mut out = vec![FORMAT_VERSION];
        varint(self.lines.len() as u64, &mut out);
        for line in &self.lines {
            out.push(line.level);
            out.push(line.flags);
            varint(line.points.len() as u64, &mut out);
            let mut last = (0, 0);
            for &(x, y) in &line.points {
                varint(zigzag(x - last.0), &mut out);
                varint(zigzag(y - last.1), &mut out);
                last = (x, y);
            }
        }
        varint(self.places.len() as u64, &mut out);
        for place in &self.places {
            let class = PlaceClass::ALL.iter().position(|c| *c == place.class);
            out.push(class.unwrap_or(0) as u8);
            varint(zigzag(place.x), &mut out);
            varint(zigzag(place.y), &mut out);
            out.push(place.rank);
            text(&place.name, &mut out);
            for optional in [&place.name_en, &place.name_pt] {
                match optional {
                    Some(name) => {
                        out.push(1);
                        text(name, &mut out);
                    }
                    None => out.push(0),
                }
            }
        }
        out
    }

    /// `None` for data of another format version or damaged data.
    pub(super) fn decode(bytes: &[u8]) -> Option<Self> {
        let (&version, rest) = bytes.split_first()?;
        if version != FORMAT_VERSION {
            return None;
        }
        let mut r = Reader::new(rest);
        let count = |r: &mut Reader<'_>| usize::try_from(r.varint()?).ok();
        let byte = |r: &mut Reader<'_>| r.take(1).map(|b| b[0]);
        let coordinate = |r: &mut Reader<'_>| Some(unzigzag(r.varint()?));
        let mut out = Self::default();
        let lines = count(&mut r)?;
        for _ in 0..lines {
            let (level, flags) = (byte(&mut r)?, byte(&mut r)?);
            let n = count(&mut r)?;
            // A corrupt count must not allocate gigabytes.
            let mut points = Vec::with_capacity(n.min(rest.len()));
            let mut last = (0, 0);
            for _ in 0..n {
                last = (last.0 + coordinate(&mut r)?, last.1 + coordinate(&mut r)?);
                points.push(last);
            }
            out.lines.push(FilteredLine {
                level,
                flags,
                points,
            });
        }
        let places = count(&mut r)?;
        for _ in 0..places {
            let class = *PlaceClass::ALL.get(usize::from(byte(&mut r)?))?;
            let (x, y) = (coordinate(&mut r)?, coordinate(&mut r)?);
            let rank = byte(&mut r)?;
            let read_text =
                |r: &mut Reader<'_>| Some(String::from_utf8_lossy(r.bytes()?).into_owned());
            let name = read_text(&mut r)?;
            let mut optional = || -> Option<Option<String>> {
                match byte(&mut r)? {
                    0 => Some(None),
                    _ => Some(Some(read_text(&mut r)?)),
                }
            };
            let (name_en, name_pt) = (optional()?, optional()?);
            out.places.push(FilteredPlace {
                class,
                x,
                y,
                rank,
                name,
                name_en,
                name_pt,
            });
        }
        r.done().then_some(out)
    }
}

fn varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}
fn zigzag(value: i32) -> u64 {
    u64::from(((value << 1) ^ (value >> 31)) as u32)
}
fn unzigzag(value: u64) -> i32 {
    let value = value as u32;
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}
fn text(value: &str, out: &mut Vec<u8>) {
    varint(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

/// The parts of a polyline (tile units) inside the tile, rounded to whole units. Cut
/// points lie exactly on the edge (0 or `EXTENT`), so a line crossing into the next
/// tile ends where that tile's copy starts (up to the tiles' separate simplification).
/// Splits a clipped ring outline where it runs along the tile's edge: there the
/// polygon was cut, not the coast.
fn without_tile_edges(points: Vec<(i32, i32)>) -> Vec<Vec<(i32, i32)>> {
    let on_edge = |a: (i32, i32), b: (i32, i32)| {
        (a.0 == b.0 && (a.0 <= 0 || a.0 >= EXTENT)) || (a.1 == b.1 && (a.1 <= 0 || a.1 >= EXTENT))
    };
    let mut runs: Vec<Vec<(i32, i32)>> = Vec::new();
    let mut run: Vec<(i32, i32)> = Vec::new();
    for pair in points.windows(2) {
        if on_edge(pair[0], pair[1]) {
            if run.len() >= 2 {
                runs.push(std::mem::take(&mut run));
            }
            run.clear();
            continue;
        }
        if run.is_empty() {
            run.push(pair[0]);
        }
        run.push(pair[1]);
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

fn clip_to_tile(line: &[(f64, f64)]) -> Vec<Vec<(i32, i32)>> {
    let max = f64::from(EXTENT);
    let bounds = super::content::TileBounds {
        south: 0.0,
        north: max,
        west: 0.0,
        east: max,
    };
    // `clip` works on (lat, lon); tile units are (x, y), so swap in and out.
    let swapped: Vec<(f64, f64)> = line.iter().map(|&(x, y)| (y, x)).collect();
    super::content::clip(&swapped, bounds)
        .into_iter()
        .map(|part| {
            let mut points: Vec<(i32, i32)> = part
                .into_iter()
                .map(|(y, x)| (x.round() as i32, y.round() as i32))
                .collect();
            points.dedup();
            points
        })
        .filter(|points| points.len() >= 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::mvt::encode::{TestFeature, TestValue, layer, tile};
    use super::*;

    fn sample() -> Vec<u8> {
        let line = |level: i64, maritime: i64, extra: Vec<(&'static str, TestValue<'static>)>| {
            let mut tags = vec![
                ("admin_level", TestValue::Int(level)),
                ("maritime", TestValue::Int(maritime)),
            ];
            tags.extend(extra);
            TestFeature {
                tags,
                kind: 2,
                parts: vec![vec![(-100, 1000), (2048, 1000), (5000, 1000)]],
            }
        };
        tile(&[
            layer(
                "boundary",
                4096,
                &[
                    line(2, 0, vec![("adm0_l", TestValue::Text("Germany"))]),
                    line(8, 0, vec![]),
                    line(2, 1, vec![]),
                    line(
                        2,
                        0,
                        vec![
                            ("disputed", TestValue::Int(1)),
                            ("claimed_by", TestValue::Text("CN")),
                        ],
                    ),
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
                            ("class", TestValue::Text("town")),
                            ("name", TestValue::Text("München")),
                            ("name:en", TestValue::Text("Munich")),
                            ("name:pt", TestValue::Text("Munique")),
                        ],
                        kind: 1,
                        parts: vec![vec![(10, 4000)]],
                    },
                    TestFeature {
                        tags: vec![
                            ("class", TestValue::Text("village")),
                            ("name", TestValue::Text("Outside")),
                        ],
                        kind: 1,
                        parts: vec![vec![(-20, 1000)]],
                    },
                    TestFeature {
                        tags: vec![
                            ("class", TestValue::Text("suburb")),
                            ("name", TestValue::Text("Not kept")),
                        ],
                        kind: 1,
                        parts: vec![vec![(20, 1000)]],
                    },
                ],
            ),
        ])
    }

    #[test]
    fn keeps_land_borders_with_levels_and_flags_cut_at_the_edge() {
        let filtered = Filtered::from_mvt(&sample()).unwrap();
        let summary: Vec<(u8, u8)> = filtered.lines.iter().map(|l| (l.level, l.flags)).collect();
        assert_eq!(
            summary,
            vec![(2, COUNTRY_EDGE), (8, 0), (2, DISPUTED | CLAIM)]
        );
        assert_eq!(
            filtered.lines[0].points,
            vec![(0, 1000), (2048, 1000), (EXTENT, 1000)]
        );
        let names: Vec<&str> = filtered.places.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Fais", "München"]);
        assert_eq!(filtered.places[1].name_en.as_deref(), Some("Munich"));
        assert_eq!(filtered.places[1].name_pt.as_deref(), Some("Munique"));
    }

    #[test]
    fn keeps_the_ocean_outline_without_the_tile_edges_and_state_names_survive_the_disk() {
        let polygon = |class: &'static str, ring: Vec<(i32, i32)>| TestFeature {
            tags: vec![("class", TestValue::Text(class))],
            kind: 3,
            parts: vec![ring],
        };
        let raw = tile(&[
            layer(
                "water",
                4096,
                &[
                    // Runs out of the tile: only the two sides inside are coast.
                    polygon(
                        "ocean",
                        vec![(2000, 5000), (2000, 2000), (5000, 2000), (5000, 5000)],
                    ),
                    // Two sides lie on the tile's edge: cut there.
                    polygon("ocean", vec![(0, 0), (1000, 0), (1000, 1000), (0, 1000)]),
                    polygon("lake", vec![(100, 100), (200, 100), (200, 200)]),
                ],
            ),
            layer(
                "place",
                4096,
                &[TestFeature {
                    tags: vec![
                        ("class", TestValue::Text("state")),
                        ("name", TestValue::Text("Bayern")),
                    ],
                    kind: 1,
                    parts: vec![vec![(500, 500)]],
                }],
            ),
        ]);
        let filtered = Filtered::from_mvt(&raw).unwrap();
        let coasts: Vec<&Vec<(i32, i32)>> = filtered
            .lines
            .iter()
            .filter(|l| l.level == COAST_LEVEL)
            .map(|l| &l.points)
            .collect();
        assert_eq!(
            coasts,
            [
                &vec![(2000, EXTENT), (2000, 2000), (EXTENT, 2000)],
                &vec![(1000, 0), (1000, 1000), (0, 1000)],
            ]
        );
        let restored = Filtered::decode(&filtered.encode()).unwrap();
        assert_eq!(restored.places[0].class, PlaceClass::State);
    }

    #[test]
    fn the_cache_format_round_trips_and_is_much_smaller() {
        let raw = sample();
        let filtered = Filtered::from_mvt(&raw).unwrap();
        let encoded = filtered.encode();
        assert_eq!(Filtered::decode(&encoded), Some(filtered));
        assert!(
            encoded.len() < raw.len() / 2,
            "{} vs {}",
            encoded.len(),
            raw.len()
        );
        // Damaged, truncated or foreign data is rejected, never a panic.
        for cut in 0..encoded.len() {
            assert!(Filtered::decode(&encoded[..cut]).is_none());
        }
        let mut other = encoded.clone();
        other[0] = FORMAT_VERSION + 1;
        assert!(Filtered::decode(&other).is_none());
    }
}
