//! Which state each municipality of a geoBoundaries file belongs to.
//!
//! geoBoundaries files carry names and outlines but no parent unit. Each municipality is
//! assigned the state polygon (from the same release's state-level file) that contains
//! a point inside it. Levels of one release nest, so neighbouring municipalities agree on
//! their states exactly, and a border is a state border only where they differ: the same
//! rule as the national sources, which name the state directly.
use super::Line;

/// A point inside the polygon whose rings are given (outer rings and holes, any
/// order), found on the horizontal line through the middle of its largest ring: the
/// middle of the widest stretch that lies inside. Unlike a centroid, it is inside
/// crescent and ring shapes too.
pub(super) fn interior_point(rings: &[Line]) -> Option<(f64, f64)> {
    let largest = rings
        .iter()
        .max_by(|a, b| ring_area(a).total_cmp(&ring_area(b)))?;
    let (min, max) = largest
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.0), hi.max(p.0))
        });
    if min >= max {
        return largest.first().copied();
    }
    let lat = (min + max) / 2.0;
    let mut crossings: Vec<f64> = rings
        .iter()
        .flat_map(|ring| ring.windows(2))
        .filter_map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            ((a.0 > lat) != (b.0 > lat)).then(|| a.1 + (lat - a.0) * (b.1 - a.1) / (b.0 - a.0))
        })
        .collect();
    crossings.sort_by(f64::total_cmp);
    crossings
        .chunks_exact(2)
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
        .map(|pair| (lat, (pair[0] + pair[1]) / 2.0))
}

/// Twice the absolute area of a ring in squared degrees (only compared, never shown).
fn ring_area(ring: &Line) -> f64 {
    ring.windows(2)
        .map(|pair| pair[0].1 * pair[1].0 - pair[1].1 * pair[0].0)
        .sum::<f64>()
        .abs()
}

/// Even-odd test against all rings of one polygon, so holes are outside.
fn inside(rings: &[Line], point: (f64, f64)) -> bool {
    rings
        .iter()
        .fold(false, |inside, ring| inside ^ encloses(ring, point))
}

/// Whether `ring` encloses `point` (even-odd). Over several rings, XOR the results:
/// a point in a hole is enclosed by the outer ring and the hole.
pub(super) fn encloses(ring: &[(f64, f64)], (lat, lon): (f64, f64)) -> bool {
    let mut inside = false;
    for pair in ring.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if (a.0 > lat) != (b.0 > lat) && lon < a.1 + (lat - a.0) * (b.1 - a.1) / (b.0 - a.0) {
            inside = !inside;
        }
    }
    inside
}

struct State {
    rings: Vec<Line>,
    min: (f64, f64),
    max: (f64, f64),
}

/// The state polygons of one country.
#[derive(Default)]
pub(super) struct States(Vec<State>);

impl States {
    /// One entry per state, each with all its rings.
    pub(super) fn new(states: Vec<Vec<Line>>) -> Self {
        Self(
            states
                .into_iter()
                .filter_map(|rings| {
                    let points = rings.iter().flat_map(|ring| ring.iter());
                    let (min, max) = points.fold(
                        (
                            (f64::INFINITY, f64::INFINITY),
                            (f64::NEG_INFINITY, f64::NEG_INFINITY),
                        ),
                        |(lo, hi), p| {
                            (
                                (lo.0.min(p.0), lo.1.min(p.1)),
                                (hi.0.max(p.0), hi.1.max(p.1)),
                            )
                        },
                    );
                    (min.0 <= max.0).then_some(State { rings, min, max })
                })
                .collect(),
        )
    }

    /// The index of the state containing the point.
    pub(super) fn containing(&self, point: (f64, f64)) -> Option<usize> {
        self.0.iter().position(|state| {
            point.0 >= state.min.0
                && point.0 <= state.max.0
                && point.1 >= state.min.1
                && point.1 <= state.max.1
                && inside(&state.rings, point)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ring(points: &[(f64, f64)]) -> Line {
        Arc::new(points.to_vec())
    }

    #[test]
    fn the_interior_point_lies_inside_concave_shapes_and_outside_holes() {
        // A "C" open to the east: its bounding-box centre is outside the shape.
        let c = ring(&[
            (0.0, 0.0),
            (0.0, 3.0),
            (1.0, 3.0),
            (1.0, 1.0),
            (2.0, 1.0),
            (2.0, 3.0),
            (3.0, 3.0),
            (3.0, 0.0),
            (0.0, 0.0),
        ]);
        let point = interior_point(std::slice::from_ref(&c)).unwrap();
        assert!(inside(std::slice::from_ref(&c), point), "{point:?}");
        // A square with a hole in the middle.
        let outer = ring(&[(0.0, 0.0), (0.0, 4.0), (4.0, 4.0), (4.0, 0.0), (0.0, 0.0)]);
        let hole = ring(&[(1.0, 1.0), (1.0, 3.0), (3.0, 3.0), (3.0, 1.0), (1.0, 1.0)]);
        let rings = [outer, hole];
        let point = interior_point(&rings).unwrap();
        assert!(inside(&rings, point), "{point:?}");
        assert!(!inside(&rings, (2.0, 2.0)), "the hole is outside");
    }

    #[test]
    fn municipalities_find_their_state() {
        let west = ring(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]);
        let east = ring(&[(0.0, 1.0), (0.0, 2.0), (1.0, 2.0), (1.0, 1.0), (0.0, 1.0)]);
        let states = States::new(vec![vec![west], vec![east]]);
        assert_eq!(states.containing((0.5, 0.25)), Some(0));
        assert_eq!(states.containing((0.5, 1.75)), Some(1));
        assert_eq!(states.containing((5.0, 5.0)), None);
    }
}
