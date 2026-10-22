//! Which municipal outline segments are state borders, which are interior, and which
//! are shared and drawn by the neighbour instead.
//!
//! Municipal outlines are far more accurate than the Natural Earth state lines, and the
//! municipalities on a state border trace that border. So the state-border segments are
//! drawn first, from the altitude where municipal data appears; the interior segments
//! (the thousand small municipalities that cost hardware) appear only once zoomed in
//! far enough for the municipalities to be a reasonable size on screen.
//!
//! Neighbouring outlines are matched by distance, not by identical vertices: services
//! that simplify each polygon on the server (ArcGIS `maxAllowableOffset`) return
//! slightly different copies of a shared border.
use super::super::Line;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Segment classes, one byte per segment of a ring.
pub(super) const OUTER: u8 = 0;
pub(super) const INTERIOR: u8 = 1;
pub(super) const MAJOR: u8 = 2;
pub(super) const DROP: u8 = 3;
/// A border between two countries' municipalities. Above `MAJOR` so `merge` keeps it
/// when two copies disagree; `DROP` still wins.
pub(super) const COUNTRY: u8 = 4;

/// Two outlines closer than this share a border. Covers the 0.005° server-side
/// simplification on both sides, and stays well below the width of a municipality.
pub(super) const SHARED_TOLERANCE_DEG: f64 = 0.012;
/// Spatial hash cell for the neighbour search.
const GRID_DEG: f64 = 0.05;
/// Municipalities that should fit on screen before the interior lines and the small
/// names appear. A thousand German municipalities at once is what the staging avoids.
const DETAIL_TARGET_COUNT: f64 = 400.0;
/// Ground width shown per kilometre of camera altitude, for the default field of view.
const VIEW_WIDTH_PER_ALTITUDE: f64 = 1.5;
/// Interior lines never wait for less altitude than this.
pub(super) const DETAIL_MIN_ALTITUDE_KM: f64 = 30.0;
const KM_PER_DEGREE: f64 = 111.32;

/// A ring's identity across units: the same municipality returned by two grid cells
/// has the same id, so both copies make the same keep-or-drop decision.
pub(super) fn ring_id(line: &Line) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    line.len().hash(&mut hasher);
    for point in [line.first(), line.last()].into_iter().flatten() {
        point.0.to_bits().hash(&mut hasher);
        point.1.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// Where a municipality belongs: its country (from the unit that loaded it) and its
/// state, when the source names one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Parent {
    pub(super) country: Option<[u8; 3]>,
    pub(super) state: Option<u64>,
}
impl Parent {
    /// The class of a border between municipalities of these parents: a country
    /// border, a state border, or interior. Unknown on either side counts as interior.
    fn border_class(self, other: Self) -> u8 {
        match (self.country, other.country) {
            (Some(a), Some(b)) if a != b => COUNTRY,
            _ if matches!((self.state, other.state), (Some(a), Some(b)) if a != b) => MAJOR,
            _ => INTERIOR,
        }
    }
}

/// Per-segment classes for every ring. `parents[i]` is ring `i`'s parent (default
/// when unknown). Copies of the same ring (same `ring_id`) never count as neighbours.
pub(super) fn classify(lines: &[Line], parents: &[Parent]) -> Vec<Vec<u8>> {
    let ids: Vec<u64> = lines.iter().map(ring_id).collect();
    let mut grid: HashMap<(i32, i32), Vec<(u32, u32)>> = HashMap::new();
    let cell = |value: f64| (value / GRID_DEG).floor() as i32;
    for (ring, line) in lines.iter().enumerate() {
        for (segment, pair) in line.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            // Repeated points carry no border; they would only crowd one grid cell.
            if a == b {
                continue;
            }
            let (min_lat, max_lat) = (a.0.min(b.0), a.0.max(b.0));
            let (min_lon, max_lon) = (a.1.min(b.1), a.1.max(b.1));
            for y in cell(min_lat - SHARED_TOLERANCE_DEG)..=cell(max_lat + SHARED_TOLERANCE_DEG) {
                for x in cell(min_lon - SHARED_TOLERANCE_DEG)..=cell(max_lon + SHARED_TOLERANCE_DEG)
                {
                    grid.entry((y, x))
                        .or_default()
                        .push((ring as u32, segment as u32));
                }
            }
        }
    }
    lines
        .iter()
        .enumerate()
        .map(|(ring, line)| {
            line.windows(2)
                .map(|pair| {
                    if pair[0] == pair[1] {
                        return OUTER;
                    }
                    let mid = ((pair[0].0 + pair[1].0) / 2.0, (pair[0].1 + pair[1].1) / 2.0);
                    let lon_scale = mid.0.to_radians().cos().max(0.05);
                    let nearest = grid
                        .get(&(cell(mid.0), cell(mid.1)))
                        .into_iter()
                        .flatten()
                        .filter(|(other, _)| ids[*other as usize] != ids[ring])
                        .map(|&(other, segment)| {
                            let points = &lines[other as usize];
                            let (a, b) = (points[segment as usize], points[segment as usize + 1]);
                            (other as usize, distance_sq(mid, a, b, lon_scale))
                        })
                        .filter(|(_, distance)| *distance <= SHARED_TOLERANCE_DEG.powi(2))
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    match nearest {
                        None => OUTER,
                        // The ring with the smaller id draws a shared border.
                        Some((other, _)) if ids[other] < ids[ring] => DROP,
                        Some((other, _)) => {
                            let own = parents.get(ring).copied().unwrap_or_default();
                            let theirs = parents.get(other).copied().unwrap_or_default();
                            own.border_class(theirs)
                        }
                    }
                })
                .collect()
        })
        .collect()
}

/// Squared distance in degrees (longitude scaled to latitude degrees) from `p` to the
/// segment `a`–`b`.
fn distance_sq(p: (f64, f64), a: (f64, f64), b: (f64, f64), lon_scale: f64) -> f64 {
    segment_distance(p, a, b, lon_scale).distance_sq
}

/// Two copies of the same ring from different units: a segment is dropped if either
/// copy saw it drawn by a neighbour, otherwise it takes the most shared class seen
/// (a unit at the edge of the loaded area may not have seen the neighbour).
pub(super) fn merge(into: &mut [u8], other: &[u8]) {
    for (class, seen) in into.iter_mut().zip(other) {
        *class = if *class == DROP || *seen == DROP {
            DROP
        } else {
            (*class).max(*seen)
        };
    }
}

/// A segment whose class was `OUTER` (no neighbour seen) takes the class found once
/// the neighbouring unit loaded; anything already decided stays.
pub(super) fn upgrade_outer(into: &mut [u8], found: &[u8]) {
    for (class, new) in into.iter_mut().zip(found) {
        if *class == OUTER {
            *class = *new;
        }
    }
}

/// Grid cell size of [`MajorCells`], about 5 km.
pub const MAJOR_CELL_DEG: f64 = 0.05;
/// A coarse (Natural Earth) border point this close to a municipal state or country
/// border is the same border: 1:10m lines stay within about 2 km of the municipal
/// outlines. Farther than this, the coarse line is a different border and stays.
pub const MAJOR_COVER_DEG: f64 = 0.03;
/// Two municipal border runs whose ends lie this close continue each other (runs
/// break where the neighbouring ring draws the shared segment instead).
const MAJOR_JOIN_DEG: f64 = 2.0 * SHARED_TOLERANCE_DEG;
/// Longitude scale floor near the poles, so cells stay finite.
const MIN_LON_SCALE: f64 = 0.05;

/// The state and country borders traced by municipalities, indexed so renderers can
/// hide exactly the part of a coarser Natural Earth line that they replace.
///
/// A point is covered when it lies within `MAJOR_COVER_DEG` of a municipal border and
/// beside it, not beyond the end of a run: where the municipal data stops (the edge of
/// the loaded area, a coast, an unloaded neighbour), the coarse line starts again at
/// the perpendicular through the run's last point. So along any border exactly one of
/// the two lines is drawn, with neither a gap nor a doubled stretch.
#[derive(Clone, Debug, Default)]
pub struct MajorCells {
    lines: Vec<Line>,
    /// Whether each line's first and last point continue another run.
    joined: Vec<[bool; 2]>,
    /// Segments (line, first point) whose cover area touches each cell.
    grid: HashMap<(i32, i32), Vec<(u32, u32)>>,
}
impl MajorCells {
    pub fn of(lines: &[Line]) -> Self {
        let lines: Vec<Line> = lines
            .iter()
            .filter(|line| line.len() >= 2)
            .cloned()
            .collect();
        let mut grid: HashMap<(i32, i32), Vec<(u32, u32)>> = HashMap::new();
        for (index, line) in lines.iter().enumerate() {
            for (segment, pair) in line.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                let lon_scale = lon_scale(a.0.abs().max(b.0.abs()) + MAJOR_COVER_DEG);
                let lon_margin = MAJOR_COVER_DEG / lon_scale;
                let (y0, x0) =
                    Self::cell(a.0.min(b.0) - MAJOR_COVER_DEG, a.1.min(b.1) - lon_margin);
                let (y1, x1) =
                    Self::cell(a.0.max(b.0) + MAJOR_COVER_DEG, a.1.max(b.1) + lon_margin);
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        grid.entry((y, x))
                            .or_default()
                            .push((index as u32, segment as u32));
                    }
                }
            }
        }
        let mut cells = Self {
            joined: Vec::with_capacity(lines.len()),
            lines,
            grid,
        };
        cells.joined = (0..cells.lines.len())
            .map(|index| {
                let line = &cells.lines[index];
                let (first, last) = (line[0], line[line.len() - 1]);
                if first == last {
                    return [true, true];
                }
                [first, last].map(|end| cells.continues(index, end))
            })
            .collect();
        cells
    }
    fn cell(lat: f64, lon: f64) -> (i32, i32) {
        (
            (lat / MAJOR_CELL_DEG).floor() as i32,
            (lon / MAJOR_CELL_DEG).floor() as i32,
        )
    }
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
    /// Whether another run passes within `MAJOR_JOIN_DEG` of this end of `line`.
    fn continues(&self, line: usize, end: (f64, f64)) -> bool {
        let scale = lon_scale(end.0);
        self.candidates(end).any(|(other, a, b)| {
            other != line
                && segment_distance(end, a, b, scale).distance_sq <= MAJOR_JOIN_DEG.powi(2)
        })
    }
    fn candidates(
        &self,
        point: (f64, f64),
    ) -> impl Iterator<Item = (usize, (f64, f64), (f64, f64))> + '_ {
        self.grid
            .get(&Self::cell(point.0, point.1))
            .into_iter()
            .flatten()
            .map(|&(line, segment)| {
                let points = &self.lines[line as usize];
                let segment = segment as usize;
                (line as usize, points[segment], points[segment + 1])
            })
    }
    /// Whether a municipal state or country border replaces a coarse line at this point
    /// (see the type documentation).
    pub fn covers(&self, lat: f64, lon: f64) -> bool {
        let point = (lat, lon);
        let scale = lon_scale(lat);
        let Some(candidates) = self.grid.get(&Self::cell(lat, lon)) else {
            return false;
        };
        candidates.iter().any(|&(line, segment)| {
            let (line, segment) = (line as usize, segment as usize);
            let points = &self.lines[line];
            let (a, b) = (points[segment], points[segment + 1]);
            let nearest = segment_distance(point, a, b, scale);
            if nearest.distance_sq > MAJOR_COVER_DEG.powi(2) {
                return false;
            }
            if nearest.along < 0.0 {
                // Beside the start: an inner vertex or a joined end still covers.
                segment > 0 || self.joined[line][0]
            } else if nearest.along > 1.0 {
                segment + 2 < points.len() || self.joined[line][1]
            } else {
                true
            }
        })
    }
}

fn lon_scale(lat: f64) -> f64 {
    lat.to_radians().cos().max(MIN_LON_SCALE)
}

/// Where a point lies relative to a segment.
struct SegmentDistance {
    /// Squared distance in degrees to the nearest point of the segment.
    distance_sq: f64,
    /// Position of the perpendicular foot along the segment, unclamped: below zero
    /// before the first point, above one past the second.
    along: f64,
}

/// Distance from `p` to the segment `a`–`b` on a local plane (longitude scaled to
/// latitude degrees).
fn segment_distance(
    p: (f64, f64),
    a: (f64, f64),
    b: (f64, f64),
    lon_scale: f64,
) -> SegmentDistance {
    let to_plane = |q: (f64, f64)| (q.0 - p.0, (q.1 - p.1) * lon_scale);
    let (ay, ax) = to_plane(a);
    let (by, bx) = to_plane(b);
    let (dy, dx) = (by - ay, bx - ax);
    let length_sq = dx * dx + dy * dy;
    let along = if length_sq > 0.0 {
        -(ax * dx + ay * dy) / length_sq
    } else {
        0.0
    };
    let t = along.clamp(0.0, 1.0);
    let (y, x) = (ay + t * dy, ax + t * dx);
    SegmentDistance {
        distance_sq: x * x + y * y,
        along,
    }
}

/// Drawn municipal segments, as runs by kind of border.
#[derive(Default)]
pub(super) struct SplitLines {
    pub(super) major: Vec<Line>,
    pub(super) interior: Vec<Line>,
}

/// Splits rings into runs of drawn segments: state-border runs and everything else.
/// Borders between two countries' municipalities stay ordinary municipal lines: two
/// countries' sources rarely trace their border alike, so a municipal version of the
/// country line would break up where they part, and the Natural Earth country line
/// stays the one country border.
pub(super) fn split(line: &Line, classes: &[u8], out: &mut SplitLines) {
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut run_class = None;
    let flush = |run: &mut Vec<(f64, f64)>, class: Option<u8>, out: &mut SplitLines| {
        if run.len() >= 2 {
            let line = std::sync::Arc::new(std::mem::take(run));
            match class {
                Some(MAJOR) => out.major.push(line),
                _ => out.interior.push(line),
            }
        }
        run.clear();
    };
    for (segment, pair) in line.windows(2).enumerate() {
        let class = match classes.get(segment).copied().unwrap_or(OUTER) {
            DROP => None,
            MAJOR => Some(MAJOR),
            _ => Some(INTERIOR),
        };
        if class != run_class {
            flush(&mut run, run_class, out);
            run_class = class;
        }
        if class.is_some() {
            if run.is_empty() {
                run.push(pair[0]);
            }
            run.push(pair[1]);
        }
    }
    flush(&mut run, run_class, out);
}

/// Area in km² of a closed ring (zero for open lines), on a local equirectangular plane.
pub(super) fn ring_area_km2(line: &Line) -> f64 {
    let (Some(first), Some(last)) = (line.first(), line.last()) else {
        return 0.0;
    };
    if line.len() < 4 || first != last {
        return 0.0;
    }
    let lon_scale = first.0.to_radians().cos() * KM_PER_DEGREE;
    let twice: f64 = line
        .windows(2)
        .map(|pair| {
            let (y1, x1) = (pair[0].0 * KM_PER_DEGREE, pair[0].1 * lon_scale);
            let (y2, x2) = (pair[1].0 * KM_PER_DEGREE, pair[1].1 * lon_scale);
            x1 * y2 - x2 * y1
        })
        .sum();
    twice.abs() / 2.0
}

/// The altitude below which the interior lines and small names of municipalities this
/// size appear: when about `DETAIL_TARGET_COUNT` of them fit on screen. Large ones
/// (US counties, Brazilian municipalities) appear with the state borders.
pub(super) fn detail_altitude_km(mean_area_km2: f64, max_km: f64) -> f64 {
    if !(mean_area_km2.is_finite() && mean_area_km2 > 0.0) {
        return max_km;
    }
    ((DETAIL_TARGET_COUNT * mean_area_km2).sqrt() / VIEW_WIDTH_PER_ALTITUDE)
        .clamp(DETAIL_MIN_ALTITUDE_KM, max_km)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn square(lat: f64, lon: f64, size: f64) -> Line {
        Arc::new(vec![
            (lat, lon),
            (lat, lon + size),
            (lat + size, lon + size),
            (lat + size, lon),
            (lat, lon),
        ])
    }

    #[test]
    fn shared_borders_are_drawn_once_and_state_borders_are_major() {
        // Three municipalities in a row: A and B in state 1, C in state 2.
        let a = square(48.0, 8.0, 0.1);
        let b = square(48.0, 8.1, 0.1);
        let c = square(48.0, 8.2, 0.1);
        let lines = vec![a.clone(), b.clone(), c.clone()];
        let state = |code| Parent {
            country: Some(*b"FRA"),
            state: Some(code),
        };
        let classes = classify(&lines, &[state(1), state(1), state(2)]);
        // Segment 1 of each square is its east side, segment 3 its west side.
        let (east, west) = (1, 3);
        // A–B: interior, drawn by exactly one of them.
        let ab = [classes[0][east], classes[1][west]];
        assert!(ab.contains(&DROP) && ab.contains(&INTERIOR), "{ab:?}");
        // B–C crosses a state border: major, drawn once.
        let bc = [classes[1][east], classes[2][west]];
        assert!(bc.contains(&DROP) && bc.contains(&MAJOR), "{bc:?}");
        // The outer sides have no neighbour.
        assert_eq!(classes[0][west], OUTER);
        assert_eq!(classes[2][east], OUTER);
    }

    #[test]
    fn simplified_copies_of_a_border_still_match() {
        // The shared side of B is shifted by 0.004° and has an extra vertex, as a
        // server that simplifies each polygon separately returns it.
        let a = square(10.0, 20.0, 0.2);
        let b: Line = Arc::new(vec![
            (10.0, 20.204),
            (10.0, 20.4),
            (10.2, 20.4),
            (10.2, 20.204),
            (10.1, 20.198),
            (10.0, 20.204),
        ]);
        let state = |code| Parent {
            country: None,
            state: Some(code),
        };
        let classes = classify(&[a, b], &[state(1), state(2)]);
        let shared = [classes[0][1], classes[1][3], classes[1][4]];
        assert!(
            shared.iter().all(|class| matches!(*class, MAJOR | DROP)),
            "{shared:?}"
        );
        assert!(shared.contains(&MAJOR));
    }

    #[test]
    fn split_keeps_runs_and_merge_prefers_dropping() {
        let ring = square(0.0, 0.0, 1.0);
        let mut out = SplitLines::default();
        split(&ring, &[MAJOR, MAJOR, DROP, OUTER], &mut out);
        assert_eq!(out.major.len(), 1);
        assert_eq!(out.major[0].len(), 3);
        assert_eq!(out.interior.len(), 1);
        assert_eq!(out.interior[0].len(), 2);
        // Country borders are drawn like interior lines.
        let mut out = SplitLines::default();
        split(&ring, &[COUNTRY, MAJOR, INTERIOR, COUNTRY], &mut out);
        assert_eq!((out.major.len(), out.interior.len()), (1, 2));
        let mut seen = vec![OUTER, INTERIOR, MAJOR, OUTER, COUNTRY];
        merge(&mut seen, &[INTERIOR, DROP, OUTER, OUTER, DROP]);
        assert_eq!(seen, [INTERIOR, DROP, MAJOR, OUTER, DROP]);
        let mut seen = vec![MAJOR];
        merge(&mut seen, &[COUNTRY]);
        assert_eq!(seen, [COUNTRY]);
    }

    #[test]
    fn detail_altitude_follows_municipality_size() {
        // German municipalities (~32 km² on average) wait until about 75 km.
        let germany = detail_altitude_km(32.0, 402.0);
        assert!((60.0..90.0).contains(&germany), "{germany}");
        // US counties appear with the state borders.
        assert_eq!(detail_altitude_km(2_500.0, 402.0), 402.0);
        assert_eq!(detail_altitude_km(0.0, 402.0), 402.0);
        assert_eq!(detail_altitude_km(0.01, 402.0), DETAIL_MIN_ALTITUDE_KM);
        let area = ring_area_km2(&square(0.0, 0.0, 1.0));
        assert!((area - KM_PER_DEGREE * KM_PER_DEGREE).abs() < 1.0, "{area}");
    }

    #[test]
    fn country_borders_are_major_and_duplicate_copies_are_not_neighbours() {
        let a = square(48.0, 8.0, 0.1);
        let b = square(48.0, 8.1, 0.1);
        let germany = Parent {
            country: Some(*b"DEU"),
            state: None,
        };
        let france = Parent {
            country: Some(*b"FRA"),
            state: None,
        };
        // A second copy of `a`, as a neighbouring unit returns it.
        let classes = classify(&[a.clone(), b, a], &[germany, france, germany]);
        let (east, west) = (1, 3);
        let shared = [classes[0][east], classes[1][west]];
        assert!(
            shared.contains(&COUNTRY) && shared.contains(&DROP),
            "{shared:?}"
        );
        assert_eq!(
            classes[0][west], OUTER,
            "a copy of the same ring is no neighbour"
        );
        assert_eq!(classes[0], classes[2]);
        // Same country, states unknown: interior.
        assert_eq!(germany.border_class(germany), INTERIOR);
    }

    #[test]
    fn outer_segments_take_the_class_found_later() {
        let mut classes = vec![OUTER, INTERIOR, OUTER, DROP];
        upgrade_outer(&mut classes, &[MAJOR, OUTER, DROP, OUTER]);
        assert_eq!(classes, [MAJOR, INTERIOR, DROP, DROP]);
    }

    #[test]
    fn major_cells_cover_beside_the_border_and_stop_at_its_free_ends() {
        let cells = MajorCells::of(&[Arc::new(vec![(48.0, 8.0), (48.0, 9.0)])]);
        assert!(!cells.is_empty());
        assert!(cells.covers(48.0, 8.5));
        assert!(
            cells.covers(48.02, 8.5),
            "a coarse line 2 km off is the same border"
        );
        assert!(!cells.covers(48.05, 8.5), "farther away is another border");
        // Past a free end the coarse line takes over again, right at the end.
        assert!(cells.covers(48.0, 8.999));
        assert!(!cells.covers(48.0, 9.002));
        assert!(!cells.covers(48.01, 7.998));
        assert!(!MajorCells::default().covers(48.0, 8.5));
    }

    #[test]
    fn joined_runs_and_rings_cover_their_ends() {
        // Two runs meeting at a corner, as when a neighbour draws the next segment.
        let cells = MajorCells::of(&[
            Arc::new(vec![(48.0, 8.0), (48.0, 9.0)]),
            Arc::new(vec![(48.005, 9.0), (48.5, 9.0)]),
        ]);
        // Just outside the corner: beyond both runs' ends, covered because they join.
        assert!(cells.covers(47.995, 9.005));
        // The other ends stay free.
        assert!(!cells.covers(48.0, 7.99));
        assert!(!cells.covers(48.51, 9.0));
        // A closed ring has no free end.
        let ring = MajorCells::of(&[square(10.0, 10.0, 1.0)]);
        assert!(ring.covers(9.99, 9.99));
        // An inner vertex covers the outside of its corner.
        let corner = MajorCells::of(&[Arc::new(vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)])]);
        assert!(corner.covers(-0.01, 1.01));
    }
}
