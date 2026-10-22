//! Where coarse Natural Earth state lines give way to the municipal state borders.
//!
//! The handover is a cut, not a fade: the covered part of a coarse line is not drawn at
//! all, and the municipal border that replaces it is drawn at full strength in the same
//! geometry. So no frame shows both lines, and none shows neither.
use globecraft_core::math::wrap_lon;
use globecraft_data::boundaries::{MAJOR_COVER_DEG, MajorCells};

/// Spacing of the coverage samples along a coarse line. A sixth of the cover distance
/// puts each cut within about 500 m of the exact point, under a pixel at the altitudes
/// where municipal borders show.
const SAMPLE_DEG: f64 = MAJOR_COVER_DEG / 6.0;

/// The parts of `line` that no municipal border covers, as separate polylines. Each cut
/// lies on the original line, halfway between the last sample on one side and the first
/// on the other.
pub(super) fn uncovered_runs(line: &[(f64, f64)], cells: &MajorCells) -> Vec<Vec<(f64, f64)>> {
    let mut runs = Vec::new();
    let Some(&first) = line.first() else {
        return runs;
    };
    let mut covered = cells.covers(first.0, first.1);
    let mut run = if covered { Vec::new() } else { vec![first] };
    for pair in line.windows(2) {
        let ((lat1, lon1), end) = (pair[0], pair[1]);
        let dlat = end.0 - lat1;
        let dlon = wrapped_delta(end.1 - lon1);
        let steps = (dlat.abs().max(dlon.abs()) / SAMPLE_DEG).ceil().max(1.0) as usize;
        let at = |t: f64| (lat1 + dlat * t, wrap_lon(lon1 + dlon * t));
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let point = if step == steps { end } else { at(t) };
            let now = cells.covers(point.0, point.1);
            if now != covered {
                let cut = at(t - 0.5 / steps as f64);
                if covered {
                    // Leaving the municipal border: a new run starts at the cut.
                    run = vec![cut];
                } else {
                    // Reaching it: the current run ends at the cut.
                    run.push(cut);
                    if run.len() >= 2 {
                        runs.push(std::mem::take(&mut run));
                    }
                    run.clear();
                }
                covered = now;
            }
        }
        if !covered {
            run.push(end);
        }
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

fn wrapped_delta(dlon: f64) -> f64 {
    if dlon > 180.0 {
        dlon - 360.0
    } else if dlon < -180.0 {
        dlon + 360.0
    } else {
        dlon
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn along_48n(from_lon: f64, to_lon: f64) -> Vec<(f64, f64)> {
        let steps = ((to_lon - from_lon) / 0.1).round() as usize;
        (0..=steps)
            .map(|i| (48.0, from_lon + (to_lon - from_lon) * i as f64 / steps as f64))
            .collect()
    }

    #[test]
    fn the_coarse_line_resumes_exactly_where_the_municipal_border_ends() {
        // The municipal state border covers 8.0–9.0°E; the coarse line runs 7.0–10.0°E,
        // 1 km to the north (a typical Natural Earth offset).
        let cells = MajorCells::of(&[Arc::new(vec![(48.0, 8.0), (48.0, 9.0)])]);
        let coarse: Vec<_> = along_48n(7.0, 10.0)
            .into_iter()
            .map(|(lat, lon)| (lat + 0.009, lon))
            .collect();
        let runs = uncovered_runs(&coarse, &cells);
        assert_eq!(runs.len(), 2, "{runs:?}");
        let west_end = runs[0].last().unwrap().1;
        let east_start = runs[1].first().unwrap().1;
        // Within one sample of the municipal border's ends: no gap, no overlap.
        assert!((west_end - 8.0).abs() <= SAMPLE_DEG, "{west_end}");
        assert!((east_start - 9.0).abs() <= SAMPLE_DEG, "{east_start}");
        assert_eq!(runs[0].first(), coarse.first());
        assert_eq!(runs[1].last(), coarse.last());
    }

    #[test]
    fn uncovered_and_fully_covered_lines() {
        let cells = MajorCells::of(&[Arc::new(vec![(48.0, 8.0), (48.0, 9.0)])]);
        // A different border 10 km away stays whole.
        let other: Vec<_> = along_48n(8.0, 9.0)
            .into_iter()
            .map(|(lat, lon)| (lat + 0.09, lon))
            .collect();
        assert_eq!(uncovered_runs(&other, &cells), vec![other.clone()]);
        // A coarse line entirely beside the municipal border disappears.
        assert!(uncovered_runs(&along_48n(8.2, 8.8), &cells).is_empty());
        // No municipal data: every line stays.
        let line = along_48n(8.0, 9.0);
        assert_eq!(uncovered_runs(&line, &MajorCells::default()), vec![line]);
    }

    #[test]
    fn a_covered_stretch_in_the_middle_splits_the_line() {
        let cells = MajorCells::of(&[Arc::new(vec![(48.0, 8.4), (48.0, 8.6)])]);
        let runs = uncovered_runs(&along_48n(8.0, 9.0), &cells);
        assert_eq!(runs.len(), 2);
        assert!((runs[0].last().unwrap().1 - 8.4).abs() <= SAMPLE_DEG);
        assert!((runs[1][0].1 - 8.6).abs() <= SAMPLE_DEG);
    }
}
