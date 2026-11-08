//! The state and municipality a point lies in, from the border tiles in memory
//! (roadmap 6.12, the "What's here?" card).
//!
//! OpenMapTiles gives borders as unnamed lines (one per stretch, shared by the areas on
//! both sides) and the areas' names as label points, never as named polygons. So an
//! area is found the other way round: its name is a label the point reaches in a
//! straight line without crossing a border of that level or a higher one, or the coast.
//! Nothing is downloaded: the lookup uses the tiles the view loaded, and says when the
//! tiles it needs are not in memory.
use super::super::{LabelKind, Line, MapLabel, PLACE_VILLAGE_RANK};
use super::content::{TileContent, TileKey};
use super::{MUNICIPAL_MIN_ZOOM, pick};
use globecraft_core::math::haversine_km;
use std::collections::HashMap;
use std::sync::Arc;

/// OpenMapTiles puts state names into tiles up to about this zoom (a cache measured
/// 2026-11-08 had them at zooms 2–10 and none at 12); deeper tiles add no candidates.
const STATE_NAME_MAX_ZOOM: u8 = 10;
/// Names are looked for this many tiles around the point's own tile, at each zoom.
const SEARCH_RADIUS_TILES: i64 = 3;
/// The nearest places tried as a turn on the way to a name (each costs a crossing test
/// against every name the straight way misses).
const MAX_STOPS_TRIED: usize = 12;
/// The nearest names tried. Bounds the lookup's time where no name is found (a point
/// whose own area has no name in the tiles: up to this many times `MAX_STOPS_TRIED`
/// crossing tests).
const MAX_NAMES_TRIED: usize = 24;

/// What the tiles in memory say about one administrative level at a point.
#[derive(Clone, Debug, PartialEq)]
pub enum AdminLookup {
    /// The name of the area the point lies in.
    Found(MapLabel),
    /// The tiles this level needs around the point are not in memory (border tiles
    /// load around the view; municipal lines only close up).
    NotLoaded,
    /// The tiles are loaded, but no name lies inside the same borders as the point.
    NoName,
}

/// The state (province) and municipality a point lies in.
#[derive(Clone, Debug, PartialEq)]
pub struct AdminAreas {
    pub state: AdminLookup,
    pub municipality: AdminLookup,
}

/// An administrative level the lookup can name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AdminLevel {
    /// States and provinces (`admin_level` 3–4), named by OpenMapTiles' state places.
    State,
    /// Municipalities (lines of `admin_level` 5–8), named by the most important city,
    /// town or village inside them: in most countries the municipality's seat, which
    /// carries its name.
    Municipality,
}

impl AdminLevel {
    /// The tile zooms whose lines are complete for the level and whose places name it.
    fn zooms(self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::State => pick::STATE_MIN_ZOOM..=STATE_NAME_MAX_ZOOM,
            Self::Municipality => MUNICIPAL_MIN_ZOOM..=pick::MAX_ZOOM,
        }
    }

    fn names(self, label: &MapLabel) -> bool {
        match self {
            Self::State => label.kind == LabelKind::State,
            // Hamlets, suburbs and neighbourhoods are parts of a municipality.
            Self::Municipality => label.kind == LabelKind::City && label.rank <= PLACE_VILLAGE_RANK,
        }
    }

    /// The lines a straight way from the point to its area's name never crosses: the
    /// level's borders, every higher level's, and the coast (a name across a bay or
    /// out at sea is not the point's).
    fn walls(self, content: &TileContent) -> impl Iterator<Item = &Line> {
        let municipal: &[Line] = match self {
            Self::State => &[],
            Self::Municipality => &content.municipal,
        };
        content
            .country
            .iter()
            .chain(&content.disputed)
            .chain(&content.state)
            .chain(&content.coast)
            .chain(municipal)
    }
}

/// A place label near the point: a name that may be the point's area, or a stop on
/// the way to one.
#[derive(Clone, Copy)]
struct Candidate<'a> {
    label: &'a MapLabel,
    /// The tile it was found in.
    tile: TileKey,
    km: f64,
}

/// Whether a straight way reaches a name without crossing a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Way {
    Clear,
    Blocked,
    /// A tile on the way is not in memory.
    Missing,
}

/// The area of `level` the point lies in, from the tiles in memory.
pub(super) fn lookup(
    tiles: &HashMap<TileKey, Arc<TileContent>>,
    level: AdminLevel,
    lat: f64,
    lon: f64,
) -> AdminLookup {
    let point_loaded = level
        .zooms()
        .any(|z| tiles.contains_key(&TileKey::at(z, lat, lon)));
    if !point_loaded {
        return AdminLookup::NotLoaded;
    }
    let mut names: Vec<Candidate> = Vec::new();
    let mut stops: Vec<Candidate> = Vec::new();
    // Names come from the level's zooms; stops from every zoom loaded, the close-up
    // tiles' villages being the likeliest turns.
    for z in pick::STATE_MIN_ZOOM..=pick::MAX_ZOOM {
        let home = TileKey::at(z, lat, lon);
        if !tiles.contains_key(&home) {
            continue;
        }
        let names_here = level.zooms().contains(&z);
        for (key, content) in tiles {
            if key.z != z || !within_search(home, *key) {
                continue;
            }
            for label in &content.places {
                let candidate = Candidate {
                    label,
                    tile: *key,
                    km: haversine_km((lat, lon), (label.lat, label.lon)),
                };
                if level.names(label) {
                    if names_here {
                        names.push(candidate);
                    }
                } else if label.kind == LabelKind::City {
                    stops.push(candidate);
                }
            }
        }
    }
    // The nearest names only (the search area at zoom 5 spans a continent), the most
    // important first (state names all have rank 0), then the nearest.
    names.sort_by(|a, b| a.km.total_cmp(&b.km));
    names.truncate(MAX_NAMES_TRIED);
    names.sort_by(|a, b| a.label.rank.cmp(&b.label.rank).then(a.km.total_cmp(&b.km)));
    // A border bending between the point and its area's name blocks the straight way:
    // the way may then turn once at a place the point itself reaches (any place names
    // a stop, the municipality's own names included).
    stops.extend(names.iter().copied());
    stops.sort_by(|a, b| a.km.total_cmp(&b.km));
    let reachable_stops: Vec<(f64, f64)> = stops
        .iter()
        .take(MAX_STOPS_TRIED)
        .filter(|stop| way(tiles, level, (lat, lon), stop) == Way::Clear)
        .map(|stop| (stop.label.lat, stop.label.lon))
        .collect();
    let mut missing_tiles = false;
    for name in &names {
        match way(tiles, level, (lat, lon), name) {
            Way::Clear => return AdminLookup::Found(name.label.clone()),
            Way::Missing => missing_tiles = true,
            Way::Blocked => {}
        }
        let via_stop = reachable_stops
            .iter()
            .filter(|&&stop| stop != (name.label.lat, name.label.lon))
            .any(|&stop| way(tiles, level, stop, name) == Way::Clear);
        if via_stop {
            return AdminLookup::Found(name.label.clone());
        }
    }
    if missing_tiles {
        AdminLookup::NotLoaded
    } else {
        AdminLookup::NoName
    }
}

/// Whether the straight way from `from` to a candidate crosses one of the level's walls,
/// read from every tile between them at the candidate's zoom.
fn way(
    tiles: &HashMap<TileKey, Arc<TileContent>>,
    level: AdminLevel,
    from: (f64, f64),
    to: &Candidate,
) -> Way {
    let start = TileKey::at(to.tile.z, from.0, from.1);
    // Every tile between the two, so no border on the way is missing.
    let Some(between) = tiles_between(tiles, start, to.tile) else {
        return Way::Missing;
    };
    let segment = (from, (to.label.lat, to.label.lon));
    let blocked = between
        .iter()
        .flat_map(|content| level.walls(content))
        .any(|line| crosses(line, segment));
    if blocked { Way::Blocked } else { Way::Clear }
}

/// Whether `other` lies within the search radius of `home` (no wrap across the
/// antimeridian: a name there is too far to be the point's).
fn within_search(home: TileKey, other: TileKey) -> bool {
    let dx = i64::from(other.x) - i64::from(home.x);
    let dy = i64::from(other.y) - i64::from(home.y);
    dx.abs() <= SEARCH_RADIUS_TILES && dy.abs() <= SEARCH_RADIUS_TILES
}

/// The contents of the block of tiles spanning `a` and `b` (same zoom), when all are
/// in memory. The block holds the straight way between any two of its points:
/// latitude and longitude both grow monotonically across Web Mercator tiles.
fn tiles_between(
    tiles: &HashMap<TileKey, Arc<TileContent>>,
    a: TileKey,
    b: TileKey,
) -> Option<Vec<&TileContent>> {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    let mut out = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            out.push(tiles.get(&TileKey { z: a.z, x, y })?.as_ref());
        }
    }
    Some(out)
}

/// Whether the polyline crosses the segment `way` (latitude/longitude taken as a
/// plane: the ways are at most a few tiles long).
fn crosses(line: &[(f64, f64)], way: ((f64, f64), (f64, f64))) -> bool {
    let (p, q) = way;
    let (min_lat, max_lat) = (p.0.min(q.0), p.0.max(q.0));
    let (min_lon, max_lon) = (p.1.min(q.1), p.1.max(q.1));
    line.windows(2).any(|pair| {
        let (a, b) = (pair[0], pair[1]);
        if a.0.max(b.0) < min_lat
            || a.0.min(b.0) > max_lat
            || a.1.max(b.1) < min_lon
            || a.1.min(b.1) > max_lon
        {
            return false;
        }
        segments_cross(p, q, a, b)
    })
}

/// Whether segments `pq` and `ab` cross (a touch at an end counts).
fn segments_cross(p: (f64, f64), q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    let side = |o: (f64, f64), s: (f64, f64), t: (f64, f64)| {
        ((s.0 - o.0) * (t.1 - o.1) - (s.1 - o.1) * (t.0 - o.0)).signum()
    };
    let (d1, d2) = (side(p, q, a), side(p, q, b));
    let (d3, d4) = (side(a, b, p), side(a, b, q));
    // Collinear overlaps are left out: a border never runs exactly along the way.
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0 && !(d1 == 0.0 && d2 == 0.0)
}

#[cfg(test)]
mod tests {
    use super::super::super::PLACE_HAMLET_RANK;
    use super::*;

    fn label(name: &str, kind: LabelKind, rank: u8, lat: f64, lon: f64) -> MapLabel {
        MapLabel {
            lat,
            lon,
            en: name.into(),
            pt: name.into(),
            kind,
            rank,
            big: false,
            from_tiles: true,
        }
    }

    fn line(points: &[(f64, f64)]) -> Line {
        Arc::new(points.to_vec())
    }

    /// The zoom 6 tiles around a point, all loaded, with the given content in the
    /// point's own tile and nothing in the others.
    fn tiles_around(
        z: u8,
        lat: f64,
        lon: f64,
        content: TileContent,
    ) -> HashMap<TileKey, Arc<TileContent>> {
        let home = TileKey::at(z, lat, lon);
        let mut tiles = HashMap::new();
        for dy in -1..=1_i64 {
            for dx in -1..=1_i64 {
                let key = TileKey {
                    z,
                    x: (i64::from(home.x) + dx) as u32,
                    y: (i64::from(home.y) + dy) as u32,
                };
                tiles.insert(key, Arc::new(TileContent::default()));
            }
        }
        tiles.insert(home, Arc::new(content));
        tiles
    }

    /// Two states side by side split by a north–south line at longitude 10.2, inside one
    /// zoom 6 tile (which spans longitudes 5.625–11.25).
    fn two_states() -> TileContent {
        TileContent {
            state: vec![line(&[(45.0, 10.2), (46.0, 10.2), (47.0, 10.25)])],
            places: vec![
                label("West", LabelKind::State, 0, 46.0, 7.0),
                label("East", LabelKind::State, 0, 46.0, 11.0),
            ],
            ..TileContent::default()
        }
    }

    #[test]
    fn the_state_is_the_name_reached_without_crossing_a_border() {
        let tiles = tiles_around(6, 46.0, 10.0, two_states());
        // Nearer to East's label, but on West's side of the line.
        let found = lookup(&tiles, AdminLevel::State, 46.0, 10.0);
        assert!(
            matches!(found, AdminLookup::Found(ref l) if l.en == "West"),
            "{found:?}"
        );
        let found = lookup(&tiles, AdminLevel::State, 46.5, 10.5);
        assert!(
            matches!(found, AdminLookup::Found(ref l) if l.en == "East"),
            "{found:?}"
        );
    }

    #[test]
    fn a_name_across_the_coast_is_not_the_points() {
        let mut content = two_states();
        // A bay between the point and West's name.
        content.coast = vec![line(&[(45.0, 8.0), (47.0, 8.0)])];
        let tiles = tiles_around(6, 46.0, 10.0, content);
        assert_eq!(
            lookup(&tiles, AdminLevel::State, 46.0, 10.0),
            AdminLookup::NoName
        );
    }

    #[test]
    fn a_way_blocked_by_a_bend_turns_at_a_place_the_point_reaches() {
        let content = TileContent {
            // A closed area (an enclave) right between the point and its state's name.
            state: vec![line(&[
                (45.5, 8.5),
                (46.5, 8.5),
                (46.5, 9.0),
                (45.5, 9.0),
                (45.5, 8.5),
            ])],
            places: vec![
                label("Home state", LabelKind::State, 0, 46.0, 7.0),
                label("Enclave", LabelKind::State, 0, 46.2, 8.75),
                label("Village", LabelKind::City, PLACE_VILLAGE_RANK, 47.0, 8.75),
            ],
            ..TileContent::default()
        };
        let tiles = tiles_around(6, 46.0, 10.0, content);
        let found = lookup(&tiles, AdminLevel::State, 46.0, 10.0);
        assert!(
            matches!(found, AdminLookup::Found(ref l) if l.en == "Home state"),
            "{found:?}"
        );
        let found = lookup(&tiles, AdminLevel::State, 46.2, 8.7);
        assert!(
            matches!(found, AdminLookup::Found(ref l) if l.en == "Enclave"),
            "{found:?}"
        );
    }

    #[test]
    fn missing_tiles_are_reported_as_not_loaded() {
        // Nothing loaded at the state zooms.
        assert_eq!(
            lookup(&HashMap::new(), AdminLevel::State, 46.0, 10.0),
            AdminLookup::NotLoaded
        );
        // Municipal lines exist only from zoom 11: zoom 6 tiles do not answer.
        let tiles = tiles_around(6, 46.0, 10.0, two_states());
        assert_eq!(
            lookup(&tiles, AdminLevel::Municipality, 46.0, 10.0),
            AdminLookup::NotLoaded
        );
        // A name whose tile is loaded, but not every tile on the way to it.
        let mut tiles = tiles_around(6, 46.0, 10.0, TileContent::default());
        let far = TileKey::at(6, 46.0, 10.0);
        let far = TileKey {
            x: far.x + 2,
            ..far
        };
        let (lat, lon) = far.point(0.5, 0.5);
        tiles.insert(
            far,
            Arc::new(TileContent {
                places: vec![label("Far", LabelKind::State, 0, lat, lon)],
                ..TileContent::default()
            }),
        );
        tiles.remove(&TileKey {
            x: far.x - 1,
            ..far
        });
        assert_eq!(
            lookup(&tiles, AdminLevel::State, 46.0, 10.0),
            AdminLookup::NotLoaded
        );
    }

    #[test]
    fn the_municipality_is_named_by_its_most_important_place() {
        // A zoom 12 tile around (46.0, 10.0) spans about 0.088° of longitude.
        let (lat, lon) = (46.0, 10.0);
        let home = TileKey::at(12, lat, lon);
        let (north, west) = home.point(0.0, 0.0);
        let (south, east) = home.point(1.0, 1.0);
        let mid_lon = (west + east) / 2.0;
        let at = |across: f64, down: f64| home.point(across, down);
        let content = TileContent {
            // The municipal border down the middle of the tile.
            municipal: vec![line(&[(south, mid_lon), (north, mid_lon)])],
            places: vec![
                // Same side as the point: a village near it and the town farther away.
                label(
                    "Village",
                    LabelKind::City,
                    PLACE_VILLAGE_RANK,
                    at(0.3, 0.4).0,
                    at(0.3, 0.4).1,
                ),
                label(
                    "Seat",
                    LabelKind::City,
                    super::super::super::PLACE_TOWN_RANK,
                    at(0.1, 0.9).0,
                    at(0.1, 0.9).1,
                ),
                // A hamlet even nearer, and a city across the border.
                label(
                    "Hamlet",
                    LabelKind::City,
                    PLACE_HAMLET_RANK,
                    at(0.26, 0.5).0,
                    at(0.26, 0.5).1,
                ),
                label("City", LabelKind::City, 2, at(0.8, 0.5).0, at(0.8, 0.5).1),
            ],
            ..TileContent::default()
        };
        let tiles = tiles_around(12, lat, lon, content);
        let (plat, plon) = at(0.25, 0.5);
        let found = lookup(&tiles, AdminLevel::Municipality, plat, plon);
        assert!(
            matches!(found, AdminLookup::Found(ref l) if l.en == "Seat"),
            "{found:?}"
        );
        let (plat, plon) = at(0.75, 0.2);
        let found = lookup(&tiles, AdminLevel::Municipality, plat, plon);
        assert!(
            matches!(found, AdminLookup::Found(ref l) if l.en == "City"),
            "{found:?}"
        );
    }

    #[test]
    fn segments_cross_only_when_they_meet() {
        let way = ((0.0, 0.0), (0.0, 2.0));
        assert!(crosses(&[(-1.0, 1.0), (1.0, 1.0)], way));
        assert!(!crosses(&[(-1.0, 3.0), (1.0, 3.0)], way));
        assert!(!crosses(&[(0.5, 0.0), (0.5, 2.0)], way));
        // A border that bends around the way's end does not block it.
        assert!(!crosses(&[(-1.0, 2.5), (1.0, 2.5), (1.0, -1.0)], way));
    }
}
