//! Joins border lines that tiles cut at their edges back into continuous lines.
//!
//! Each tile holds its own piece of a border, cut at the tile's edge. Drawn as separate
//! pieces, a dashed line restarts its dash pattern at every tile edge, and the tiles'
//! separate simplification leaves a small step there. Two pieces are joined where
//! exactly two line ends meet (within a few tile units); where three or more meet (a
//! point where three borders meet) the lines stay separate.
use super::super::Line;
use std::collections::HashMap;
use std::sync::Arc;

/// Ends this many tile units apart (of 4,096 across a tile) are the same point: tiles
/// are simplified separately, so a border's two pieces may miss each other slightly.
const JOIN_TOLERANCE_UNITS: f64 = 8.0;

/// The join tolerance in degrees for tiles of zoom `z`.
pub(super) fn tolerance_deg(z: u8) -> f64 {
    360.0 / (f64::from(1_u32 << z) * f64::from(super::compact::EXTENT)) * JOIN_TOLERANCE_UNITS
}

type Cell = (i64, i64);

struct Ends {
    cells: HashMap<Cell, Vec<(usize, bool)>>,
    tolerance: f64,
}

impl Ends {
    fn cell(&self, p: (f64, f64)) -> Cell {
        (
            (p.0 / self.tolerance).floor() as i64,
            (p.1 / self.tolerance).floor() as i64,
        )
    }
    /// Line ends (line, is its start) within the tolerance of `p`, other than `own`.
    fn near(&self, lines: &[Line], p: (f64, f64), own: (usize, bool)) -> Vec<(usize, bool)> {
        let (cy, cx) = self.cell(p);
        let mut found = Vec::new();
        for y in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                for &(line, start) in self.cells.get(&(y, x)).into_iter().flatten() {
                    if (line, start) == own {
                        continue;
                    }
                    let q = end_point(&lines[line], start);
                    if (q.0 - p.0).abs() <= self.tolerance && (q.1 - p.1).abs() <= self.tolerance {
                        found.push((line, start));
                    }
                }
            }
        }
        found
    }
}

fn end_point(line: &Line, start: bool) -> (f64, f64) {
    if start { line[0] } else { line[line.len() - 1] }
}

/// The lines with every two-way meeting of ends joined into one line.
pub(super) fn join(lines: Vec<Line>, tolerance: f64) -> Vec<Line> {
    let lines: Vec<Line> = lines.into_iter().filter(|l| l.len() >= 2).collect();
    let mut ends = Ends {
        cells: HashMap::new(),
        tolerance,
    };
    for (index, line) in lines.iter().enumerate() {
        for start in [true, false] {
            let cell = ends.cell(end_point(line, start));
            ends.cells.entry(cell).or_default().push((index, start));
        }
    }
    // The only other end meeting this one, if exactly one does.
    let partner = |ends: &Ends, line: usize, start: bool| -> Option<(usize, bool)> {
        let p = end_point(&lines[line], start);
        match ends.near(&lines, p, (line, start)).as_slice() {
            // A closed piece meets itself; that is not a join.
            [(other, other_start)] if *other != line => Some((*other, *other_start)),
            _ => None,
        }
    };
    let mut used = vec![false; lines.len()];
    let mut out = Vec::with_capacity(lines.len());
    for first in 0..lines.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut points: Vec<(f64, f64)> = lines[first].to_vec();
        let mut joined = false;
        // Forward from the end, then backward from the start.
        for forward in [true, false] {
            let (mut line, mut at_start) = (first, !forward);
            while let Some((next, next_start)) = partner(&ends, line, at_start) {
                // Join only when the relation is mutual and the partner is still free.
                if used[next] || partner(&ends, next, next_start) != Some((line, at_start)) {
                    break;
                }
                used[next] = true;
                joined = true;
                let mut piece: Vec<(f64, f64)> = lines[next].to_vec();
                // Walk the next piece away from the joint.
                if !next_start {
                    piece.reverse();
                }
                piece.remove(0);
                if forward {
                    points.extend(piece);
                } else {
                    piece.reverse();
                    points.splice(0..0, piece);
                }
                (line, at_start) = (next, !next_start);
            }
        }
        out.push(if joined {
            Arc::new(points)
        } else {
            lines[first].clone()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: &[(f64, f64)]) -> Line {
        Arc::new(points.to_vec())
    }

    #[test]
    fn pieces_cut_at_tile_edges_become_one_line_in_any_direction() {
        let tol = 0.001;
        // Three pieces of one border, the middle one reversed and slightly off.
        let pieces = vec![
            line(&[(1.0, 2.0), (1.0, 3.0)]),
            line(&[(1.0, 1.0), (1.0004, 2.0002)]),
            line(&[(1.0, 0.0), (1.0, 0.5), (1.0, 1.0)]),
        ];
        let joined = join(pieces, tol);
        assert_eq!(joined.len(), 1);
        let l = &joined[0];
        assert_eq!(l.len(), 5);
        let lons: Vec<f64> = l.iter().map(|p| p.1).collect();
        assert!(
            lons.windows(2).all(|w| w[0] < w[1]) || lons.windows(2).all(|w| w[0] > w[1]),
            "{lons:?}"
        );
    }

    #[test]
    fn three_borders_meeting_at_a_point_stay_separate() {
        let tol = 0.001;
        let joined = join(
            vec![
                line(&[(0.0, 0.0), (1.0, 1.0)]),
                line(&[(1.0, 1.0), (2.0, 1.0)]),
                line(&[(1.0, 1.0), (1.0, 2.0)]),
            ],
            tol,
        );
        assert_eq!(joined.len(), 3);
        // A closed ring stays as it is.
        let ring = line(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.0, 0.0)]);
        assert_eq!(join(vec![ring.clone()], tol), vec![ring]);
    }

    #[test]
    fn the_tolerance_follows_the_zoom() {
        assert!(tolerance_deg(12) < 0.0002 && tolerance_deg(12) > 0.00005);
        assert!((tolerance_deg(5) / tolerance_deg(6) - 2.0).abs() < 1e-9);
    }
}
