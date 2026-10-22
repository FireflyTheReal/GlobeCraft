//! The Natural Earth lines that municipal outlines replace up close.
//!
//! Most sources do not say which state a municipality belongs to (geoBoundaries files
//! carry only names, OSM cells only outlines), so their borders cannot be classified by
//! parent. Instead, a municipal segment that runs along a Natural Earth state line is
//! that state border, and one along a Natural Earth country line is that country border.
//! This replaces exactly the lines on screen, for every source, and also classifies a
//! country's edge while its neighbour is not loaded.
use super::super::Line;
use super::topology::{COUNTRY, MAJOR, MajorCells, OUTER};
use std::sync::Arc;

/// Indexes of the Natural Earth lines municipal outlines are traced against.
pub(super) struct Reference {
    states: MajorCells,
    countries: MajorCells,
}

/// Identifies the reference lines without hashing them: the lists are replaced as a
/// whole when a detail level arrives, so their lengths and first allocations change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReferenceKey([(usize, usize); 3]);

/// The Natural Earth lines in the current boundary data.
#[derive(Clone, Copy)]
pub struct ReferenceLines<'a> {
    pub states: &'a [Line],
    pub states_big: &'a [Line],
    pub countries: &'a [Line],
}

impl ReferenceLines<'_> {
    pub(super) fn key(self) -> ReferenceKey {
        let id = |lines: &[Line]| {
            (
                lines.len(),
                lines.first().map_or(0, |line| Arc::as_ptr(line) as usize),
            )
        };
        ReferenceKey([id(self.states), id(self.states_big), id(self.countries)])
    }
    pub(super) fn is_empty(self) -> bool {
        self.states.is_empty() && self.states_big.is_empty() && self.countries.is_empty()
    }
    /// Shared copies of the lines, for building the index off this thread.
    pub(super) fn to_owned(self) -> (Vec<Line>, Vec<Line>) {
        let states = self.states.iter().chain(self.states_big).cloned().collect();
        (states, self.countries.to_vec())
    }
}

impl Reference {
    pub(super) fn build(states: &[Line], countries: &[Line]) -> Self {
        Self {
            states: MajorCells::of(states),
            countries: MajorCells::of(countries),
        }
    }

    /// The class each segment of `line` traces: `COUNTRY` along a country line, `MAJOR`
    /// along a state line, `OUTER` for neither.
    pub(super) fn trace(&self, line: &Line) -> Arc<[u8]> {
        line.windows(2)
            .map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                if self.countries.traces(a, b) {
                    COUNTRY
                } else if self.states.traces(a, b) {
                    MAJOR
                } else {
                    OUTER
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_along_natural_earth_lines_take_their_class() {
        let state: Line = Arc::new(vec![(48.0, 8.0), (48.0, 9.0)]);
        let country: Line = Arc::new(vec![(47.0, 8.0), (47.0, 9.0)]);
        let reference = Reference::build(&[state], &[country]);
        // A municipality straddling nothing, one along the state line, one along the
        // country line (both 1 km off, as simplified sources are).
        let ring: Line = Arc::new(vec![
            (48.01, 8.2),
            (48.01, 8.4),
            (48.3, 8.4),
            (47.01, 8.6),
            (47.01, 8.8),
        ]);
        assert_eq!(&*reference.trace(&ring), [MAJOR, OUTER, OUTER, COUNTRY]);
    }

    #[test]
    fn keys_change_when_a_list_is_replaced() {
        let a: Vec<Line> = vec![Arc::new(vec![(0.0, 0.0), (1.0, 1.0)])];
        let b: Vec<Line> = vec![Arc::new(vec![(0.0, 0.0), (1.0, 1.0)])];
        fn lines(states: &[Line]) -> ReferenceLines<'_> {
            ReferenceLines {
                states,
                states_big: &[],
                countries: &[],
            }
        }
        assert_eq!(lines(&a).key(), lines(&a).key());
        assert_ne!(lines(&a).key(), lines(&b).key());
        assert!(lines(&[]).is_empty());
    }

    /// Real data, outside the repository: set `GLOBECRAFT_REFERENCE_BENCH` to a folder
    /// holding `ne_10m_admin_1_states_provinces_lines.geojson`,
    /// `ne_10m_admin_0_boundary_lines_land.geojson` and a geoBoundaries file `ITA-ADM4.geojson`.
    #[test]
    #[ignore = "manual benchmark; run with --release --ignored --nocapture"]
    fn reference_build_and_trace_time() {
        let Ok(dir) = std::env::var("GLOBECRAFT_REFERENCE_BENCH") else {
            return;
        };
        let read = |name: &str| -> Vec<Line> {
            let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            let mut lines = Vec::new();
            for feature in value["features"].as_array().unwrap() {
                super::super::super::geojson::collect_geojson_lines(
                    &feature["geometry"]["coordinates"],
                    &mut lines,
                );
            }
            lines
        };
        let states = read("ne_10m_admin_1_states_provinces_lines.geojson");
        let countries = read("ne_10m_admin_0_boundary_lines_land.geojson");
        let comuni = read("ITA-ADM4.geojson");
        let count = |lines: &[Line]| lines.iter().map(|l| l.len()).sum::<usize>();
        let start = std::time::Instant::now();
        let reference = Reference::build(&states, &countries);
        let build_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = std::time::Instant::now();
        let traced: Vec<Arc<[u8]>> = comuni.iter().map(|line| reference.trace(line)).collect();
        let trace_ms = start.elapsed().as_secs_f64() * 1000.0;
        let all: Vec<u8> = traced.iter().flat_map(|t| t.iter().copied()).collect();
        let share = |class| all.iter().filter(|c| **c == class).count() as f64 / all.len() as f64;
        println!(
            "reference: {} state + {} country vertices, built in {build_ms:.0} ms; \
             traced {} comuni ({} vertices) in {trace_ms:.0} ms: {:.1} % state, {:.1} % country",
            count(&states),
            count(&countries),
            comuni.len(),
            count(&comuni),
            share(MAJOR) * 100.0,
            share(COUNTRY) * 100.0,
        );
    }
}
