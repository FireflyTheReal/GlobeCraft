//! Where coarse Natural Earth state lines give way to the municipal state borders.
//!
//! The handover is a cut, not a fade: the covered part of a coarse line is not drawn at
//! all, and the municipal border that replaces it is drawn at full strength in the same
//! geometry. So no frame shows both lines, and none shows neither.
use globecraft_core::math::wrap_lon;
use globecraft_data::boundaries::{MAJOR_COVER_DEG, MajorCells};

/// Spacing of the coverage tests along a coarse line, as long as the cover distance:
/// a municipal border or a gap shorter than that (3 km) is not worth a cut. Most coarse
/// segments are shorter, so each vertex is tested once.
const STEP_DEG: f64 = MAJOR_COVER_DEG;
/// Where coverage changes inside a step, the cut is bisected to within this distance of
/// the exact point (about 100 m, far under a pixel where municipal borders show).
const CUT_PRECISION_DEG: f64 = MAJOR_COVER_DEG / 30.0;

/// The parts of `line` that no municipal border covers, as separate polylines. Each cut
/// lies on the original line, where coverage changes.
pub(super) fn uncovered_runs(line: &[(f64, f64)], cells: &MajorCells) -> Vec<Vec<(f64, f64)>> {
    let mut runs = Vec::new();
    let Some(&first) = line.first() else {
        return runs;
    };
    let covers = |point: (f64, f64)| cells.covers(point.0, point.1);
    let mut covered = covers(first);
    let mut run = if covered { Vec::new() } else { vec![first] };
    for pair in line.windows(2) {
        let ((lat1, lon1), end) = (pair[0], pair[1]);
        let dlat = end.0 - lat1;
        let dlon = wrapped_delta(end.1 - lon1);
        let span = dlat.abs().max(dlon.abs());
        let steps = (span / STEP_DEG).ceil().max(1.0) as usize;
        let at = |t: f64| (lat1 + dlat * t, wrap_lon(lon1 + dlon * t));
        let mut from = 0.0;
        for step in 1..=steps {
            let to = step as f64 / steps as f64;
            let now = covers(if step == steps { end } else { at(to) });
            if now != covered {
                // Bisect the step for the point where coverage changes.
                let (mut lo, mut hi) = (from, to);
                while (hi - lo) * span > CUT_PRECISION_DEG {
                    let mid = (lo + hi) / 2.0;
                    if covers(at(mid)) == covered {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let cut = at((lo + hi) / 2.0);
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
            from = to;
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
            .map(|i| {
                (
                    48.0,
                    from_lon + (to_lon - from_lon) * i as f64 / steps as f64,
                )
            })
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
        assert!((west_end - 8.0).abs() <= CUT_PRECISION_DEG, "{west_end}");
        assert!(
            (east_start - 9.0).abs() <= CUT_PRECISION_DEG,
            "{east_start}"
        );
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
        assert!((runs[0].last().unwrap().1 - 8.4).abs() <= CUT_PRECISION_DEG);
        assert!((runs[1][0].1 - 8.6).abs() <= CUT_PRECISION_DEG);
    }

    #[test]
    #[ignore = "manual benchmark; run alone with --release --ignored --nocapture"]
    fn handover_cut_time() {
        // A Germany-sized view at the handover altitude: 60 state lines of 6° with
        // 1:10m-like vertex spacing (0.02°), half of each traced by municipal borders.
        let lines: Vec<Vec<(f64, f64)>> = (0..60)
            .map(|i| {
                let lat = 47.0 + f64::from(i) * 0.1;
                (0..=300)
                    .map(|j| (lat + 0.005, 6.0 + f64::from(j) * 0.02))
                    .collect()
            })
            .collect();
        let municipal: Vec<_> = (0..60)
            .map(|i| {
                let lat = 47.0 + f64::from(i) * 0.1;
                Arc::new(
                    (0..=150)
                        .map(|j| (lat, 6.0 + f64::from(j) * 0.02))
                        .collect(),
                )
            })
            .collect();
        let start = std::time::Instant::now();
        let cells = MajorCells::of(&municipal);
        let index_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        let mut kept = 0;
        for _ in 0..9 {
            let start = std::time::Instant::now();
            kept = lines
                .iter()
                .map(|line| uncovered_runs(line, &cells).len())
                .sum::<usize>();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "60 lines x 301 points, half covered: cut {:.3} ms (median of 9), {kept} runs kept; index {index_ms:.3} ms",
            times[4]
        );
    }
}
