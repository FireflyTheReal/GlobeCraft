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

/// A stretch of coarse line between two covered stretches, shorter than this, is the
/// generalised line straying more than the cover distance from the real border (a
/// river bend it cut off, a smoothed corner): the municipal border already draws that
/// border there, so the stretch is dropped instead of left as a stray piece.
const MAX_DETOUR_KM: f64 = 30.0;
/// A detour is dropped only if all of it lies this close to a municipal border (about
/// 16 km), so that border really bridges it. Without this, a coarse stretch over a hole
/// in the municipal data (the undefined border across the Southern Patagonian Ice
/// Field) was dropped with nothing drawn in its place.
const DETOUR_REACH_DEG: f64 = 0.15;
const KM_PER_DEGREE: f64 = 111.32;

fn length_km(line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let lon_scale = ((a.0 + b.0) / 2.0).to_radians().cos();
            let dlon = wrapped_delta(b.1 - a.1) * lon_scale;
            (b.0 - a.0).hypot(dlon) * KM_PER_DEGREE
        })
        .sum()
}

/// The parts of `line` that no municipal border covers, as separate polylines. Each cut
/// lies on the original line, where coverage changes. Short detours between two covered
/// stretches are dropped (see `MAX_DETOUR_KM`).
pub(super) fn uncovered_runs(line: &[(f64, f64)], cells: &MajorCells) -> Vec<Vec<(f64, f64)>> {
    let mut runs = Vec::new();
    let Some(&first) = line.first() else {
        return runs;
    };
    let covers = |point: (f64, f64)| cells.covers(point.0, point.1);
    let mut covered = covers(first);
    let mut run = if covered { Vec::new() } else { vec![first] };
    // Whether the current run began where a covered stretch ended.
    let mut after_cover = false;
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
                    after_cover = true;
                } else {
                    // Reaching it: the current run ends at the cut.
                    run.push(cut);
                    let detour =
                        after_cover && length_km(&run) < MAX_DETOUR_KM && bridged(&run, cells);
                    if run.len() >= 2 && !detour {
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

/// Whether a municipal border passes within `DETOUR_REACH_DEG` of every point of
/// `run`, sampled about every 2 km.
fn bridged(run: &[(f64, f64)], cells: &MajorCells) -> bool {
    run.windows(2).all(|pair| {
        let (a, b) = (pair[0], pair[1]);
        let (dlat, dlon) = (b.0 - a.0, wrapped_delta(b.1 - a.1));
        let steps = (dlat.abs().max(dlon.abs()) / 0.02).ceil().max(1.0) as usize;
        (0..=steps).all(|step| {
            let t = step as f64 / steps as f64;
            cells.reaches(a.0 + dlat * t, wrap_lon(a.1 + dlon * t), DETOUR_REACH_DEG)
        })
    })
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
    fn short_detours_between_covered_stretches_are_dropped() {
        // The municipal border runs along 48°N from 8.0 to 9.0°E. The coarse line
        // follows it but strays 4 km north for about 13 km, beyond the cover distance.
        let cells = MajorCells::of(&[Arc::new(vec![(48.0, 8.0), (48.0, 9.0)])]);
        let mut coarse = along_48n(8.0, 8.4);
        coarse.extend([(48.035, 8.47), (48.035, 8.53)]);
        coarse.extend(along_48n(8.6, 9.0));
        assert!(uncovered_runs(&coarse, &cells).is_empty());
        // A long stretch between two covered ones is another border: it stays.
        let long = MajorCells::of(&[
            Arc::new(vec![(48.0, 7.0), (48.0, 7.2)]),
            Arc::new(vec![(48.0, 8.8), (48.0, 9.0)]),
        ]);
        let runs = uncovered_runs(&along_48n(7.0, 9.0), &long);
        assert_eq!(runs.len(), 1);
        // A stretch past the end of the municipal data stays too.
        let runs = uncovered_runs(&along_48n(8.5, 9.5), &cells);
        assert_eq!(runs.len(), 1);
        // A short stretch over a hole in the municipal data (nothing within reach of
        // its middle) is not bridged: it stays, or nothing would be drawn there.
        let hole = MajorCells::of(&[
            Arc::new(vec![(48.0, 8.0), (48.0, 8.3)]),
            Arc::new(vec![(48.0, 8.6), (48.0, 9.0)]),
        ]);
        let mut around = along_48n(8.0, 8.32);
        around.extend([(48.25, 8.4), (48.25, 8.5)]);
        around.extend(along_48n(8.58, 9.0));
        assert_eq!(
            uncovered_runs(&around, &hole).len(),
            1,
            "the unbridged stretch stays"
        );
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

    /// Applies the real cut to the output of the data crate's
    /// `real_countries_classify_their_borders` check: set `GLOBECRAFT_HANDOVER_CHECK` to
    /// its `check.json`; writes `ne_state` (what stays drawn) back into it.
    #[test]
    #[ignore = "manual check on real data"]
    fn real_cut_of_natural_earth_state_lines() {
        let Ok(path) = std::env::var("GLOBECRAFT_HANDOVER_CHECK") else {
            return;
        };
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let lines = |key: &str| -> Vec<globecraft_data::boundaries::Line> {
            serde_json::from_value::<Vec<Vec<(f64, f64)>>>(value[key].clone())
                .unwrap()
                .into_iter()
                .map(Arc::new)
                .collect()
        };
        let cut = |drawn: &str, coarse: &str| -> Vec<Vec<(f64, f64)>> {
            let cells = MajorCells::of(&lines(drawn));
            lines(coarse)
                .iter()
                .flat_map(|line| uncovered_runs(line, &cells))
                .collect()
        };
        let states = cut("state", "ne_state_all");
        // Country lines are cut per border, as the renderer does.
        let pairs = |key: &str| -> Vec<Option<globecraft_data::boundaries::CountryPair>> {
            serde_json::from_value::<Vec<Option<[String; 2]>>>(value[key].clone())
                .unwrap_or_default()
                .into_iter()
                .map(|pair| {
                    pair.and_then(|[a, b]| {
                        Some([a.as_bytes().try_into().ok()?, b.as_bytes().try_into().ok()?])
                    })
                })
                .collect()
        };
        let country_cells = globecraft_data::boundaries::CountryCells::of(
            &lines("country"),
            &pairs("country_pairs"),
        );
        let ne_pairs = pairs("ne_country_pairs");
        let countries: Vec<Vec<(f64, f64)>> = lines("ne_country_all")
            .iter()
            .enumerate()
            .flat_map(|(index, line)| {
                let cells = country_cells.for_pair(ne_pairs.get(index).copied().flatten());
                uncovered_runs(line, cells)
            })
            .collect();
        println!(
            "{} Natural Earth state runs and {} country runs stay drawn",
            states.len(),
            countries.len()
        );
        value["ne_state"] = serde_json::to_value(states).unwrap();
        value["ne_country"] = serde_json::to_value(countries).unwrap();
        std::fs::write(&path, value.to_string()).unwrap();
    }
}
