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

/// Two outlines closer than this share a border. Covers the 0.005° server-side
/// simplification on both sides, and stays well below the width of a municipality.
const SHARED_TOLERANCE_DEG: f64 = 0.012;
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

/// Per-segment classes for every ring. `parents[i]` names ring `i`'s state (or
/// country, when the source has no state code); `None` matches nothing.
pub(super) fn classify(lines: &[Line], parents: &[Option<u64>]) -> Vec<Vec<u8>> {
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
                        .filter(|(other, _)| *other as usize != ring)
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
                            match (
                                parents.get(ring).copied().flatten(),
                                parents.get(other).copied().flatten(),
                            ) {
                                (Some(own), Some(theirs)) if own != theirs => MAJOR,
                                _ => INTERIOR,
                            }
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
    let to_plane = |q: (f64, f64)| (q.0 - p.0, (q.1 - p.1) * lon_scale);
    let (ay, ax) = to_plane(a);
    let (by, bx) = to_plane(b);
    let (dy, dx) = (by - ay, bx - ax);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq > 0.0 {
        (-(ax * dx + ay * dy) / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (y, x) = (ay + t * dy, ax + t * dx);
    x * x + y * y
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

/// Splits rings into runs of drawn segments: state-border runs and everything else.
pub(super) fn split(line: &Line, classes: &[u8], major: &mut Vec<Line>, interior: &mut Vec<Line>) {
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut run_class = None;
    let flush = |run: &mut Vec<(f64, f64)>,
                 class: Option<u8>,
                 major: &mut Vec<Line>,
                 interior: &mut Vec<Line>| {
        if run.len() >= 2 {
            let line = std::sync::Arc::new(std::mem::take(run));
            if class == Some(MAJOR) {
                major.push(line);
            } else {
                interior.push(line);
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
            flush(&mut run, run_class, major, interior);
            run_class = class;
        }
        if class.is_some() {
            if run.is_empty() {
                run.push(pair[0]);
            }
            run.push(pair[1]);
        }
    }
    flush(&mut run, run_class, major, interior);
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
        let classes = classify(&lines, &[Some(1), Some(1), Some(2)]);
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
        let classes = classify(&[a, b], &[Some(1), Some(2)]);
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
        let (mut major, mut interior) = (Vec::new(), Vec::new());
        split(
            &ring,
            &[MAJOR, MAJOR, DROP, OUTER],
            &mut major,
            &mut interior,
        );
        assert_eq!(major.len(), 1);
        assert_eq!(major[0].len(), 3);
        assert_eq!(interior.len(), 1);
        assert_eq!(interior[0].len(), 2);
        let mut seen = vec![OUTER, INTERIOR, MAJOR, OUTER];
        merge(&mut seen, &[INTERIOR, DROP, OUTER, OUTER]);
        assert_eq!(seen, [INTERIOR, DROP, MAJOR, OUTER]);
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
}
