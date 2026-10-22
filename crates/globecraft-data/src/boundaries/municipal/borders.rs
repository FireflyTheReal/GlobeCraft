//! Country borders drawn from municipal outlines, one source per border.
//!
//! Neighbouring countries' municipalities come from different sources, which rarely trace
//! their common border alike. Matching the two versions segment by segment broke the
//! border into pieces that alternated between them. Instead:
//!
//! 1. Only a country's *outline* is a candidate: once its municipalities are loaded,
//!    every internal edge is shared, so the unshared edges are its coast and its land
//!    borders (plus, briefly, the edge of a partly loaded area).
//! 2. Natural Earth's land-border lines name the two countries they separate, so an
//!    outline segment running along one says which border it belongs to.
//! 3. Each border is drawn by one side only: the one with the better source, or the
//!    only one loaded. The other side's outline is hidden wherever the chosen line runs,
//!    and the Natural Earth line is cut there.
use super::super::Line;
use super::topology::{COUNTRY, DROP, INTERIOR, OUTER, lon_scale, segment_distance};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// An outline segment belongs to a border when its ends and middle lie within this
/// distance of the border's Natural Earth line (Natural Earth 1:10m and the municipal
/// sources differ by up to a few km), beside it rather than past its end. Only outline
/// segments are tested, never a country's inner edges, so no direction test is needed:
/// detailed outlines zig-zag along a border, and a direction test rejected most of
/// their segments. Past a border line's end (where it reaches the sea) the coast is not
/// taken for the border.
const BAND_DEG: f64 = 0.05;
const GRID_DEG: f64 = 0.1;

/// Natural Earth land-border lines in the boundary data, with the two countries each
/// separates (`None` for lines without that information, such as disputed overlays).
#[derive(Clone, Copy)]
pub struct CountryBorderLines<'a> {
    pub lines: &'a [Line],
    pub pairs: &'a [Option<super::super::CountryPair>],
}

impl CountryBorderLines<'_> {
    /// Identifies the lines without hashing them: the list is replaced as a whole when a
    /// detail level arrives, so its length and first allocation change.
    pub(super) fn key(self) -> (usize, usize) {
        (
            self.lines.len(),
            self.lines
                .first()
                .map_or(0, |line| Arc::as_ptr(line) as usize),
        )
    }
    pub(super) fn to_owned(self) -> Vec<(Line, [[u8; 3]; 2])> {
        self.lines
            .iter()
            .zip(self.pairs)
            .filter_map(|(line, pair)| Some((line.clone(), (*pair)?)))
            .collect()
    }
}

/// Lines indexed with the pair of countries each one separates.
pub(super) struct CountryBorders {
    lines: Vec<(Line, [[u8; 3]; 2])>,
    grid: HashMap<(i32, i32), Vec<(u32, u32)>>,
}

fn cell(lat: f64, lon: f64) -> (i32, i32) {
    (
        (lat / GRID_DEG).floor() as i32,
        (lon / GRID_DEG).floor() as i32,
    )
}

impl CountryBorders {
    pub(super) fn new(lines: Vec<(Line, [[u8; 3]; 2])>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<(u32, u32)>> = HashMap::new();
        for (index, (line, _)) in lines.iter().enumerate() {
            for (segment, pair) in line.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                let margin_lon = BAND_DEG / lon_scale(a.0.abs().max(b.0.abs()) + BAND_DEG);
                let (y0, x0) = cell(a.0.min(b.0) - BAND_DEG, a.1.min(b.1) - margin_lon);
                let (y1, x1) = cell(a.0.max(b.0) + BAND_DEG, a.1.max(b.1) + margin_lon);
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        grid.entry((y, x))
                            .or_default()
                            .push((index as u32, segment as u32));
                    }
                }
            }
        }
        Self { lines, grid }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The country across the nearest indexed border of `country` within the band of
    /// `point`.
    fn nearest(&self, point: (f64, f64), country: [u8; 3]) -> Option<[u8; 3]> {
        let scale = lon_scale(point.0);
        self.grid
            .get(&cell(point.0, point.1))?
            .iter()
            .filter_map(|&(line, segment)| {
                let (points, pair) = &self.lines[line as usize];
                let across = match *pair {
                    [a, b] if a == country => b,
                    [a, b] if b == country => a,
                    _ => return None,
                };
                let segment = segment as usize;
                let (a, b) = (points[segment], points[segment + 1]);
                let nearest = segment_distance(point, a, b, scale);
                let past_free_end = (segment == 0 && nearest.along < 0.0)
                    || (segment + 2 == points.len() && nearest.along > 1.0);
                (nearest.distance_sq <= BAND_DEG.powi(2) && !past_free_end)
                    .then_some((nearest.distance_sq, across))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, across)| across)
    }

    /// The country across the border that the segment `a`–`b` of an outline of
    /// `country` runs along, if it runs along one.
    pub(super) fn across(&self, a: (f64, f64), b: (f64, f64), country: [u8; 3]) -> Option<[u8; 3]> {
        if a == b {
            return None;
        }
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let across = self.nearest(mid, country)?;
        [a, b]
            .iter()
            .all(|&end| self.nearest(end, country) == Some(across))
            .then_some(across)
    }
}

/// One ring in an assembled area: its points, its classes, and its country.
pub(super) struct BorderRing<'a> {
    pub(super) line: &'a Line,
    pub(super) classes: &'a mut Vec<u8>,
    pub(super) country: Option<[u8; 3]>,
}

/// Decides which side draws each country border (see the module documentation):
/// outline segments along a border become `COUNTRY` on the drawing side and `DROP` on the
/// other where the drawn line runs; everything else that is not a border is `INTERIOR`.
/// `better(x, y)` says whether country `x`'s source is preferred to `y`'s.
pub(super) fn resolve(
    rings: &mut [BorderRing<'_>],
    borders: &CountryBorders,
    better: impl Fn([u8; 3], [u8; 3]) -> bool,
) {
    let loaded: HashSet<[u8; 3]> = rings.iter().filter_map(|ring| ring.country).collect();
    let draws = |own: [u8; 3], across: [u8; 3]| {
        !loaded.contains(&across) || better(own, across) || (!better(across, own) && own < across)
    };
    // First pass: classify outline segments; the drawing side's become `COUNTRY` and are
    // collected, the other side's wait for the second pass.
    let mut drawn: Vec<(Line, [[u8; 3]; 2])> = Vec::new();
    let mut waiting: Vec<(usize, usize, [u8; 3])> = Vec::new();
    for (index, ring) in rings.iter_mut().enumerate() {
        let Some(own) = ring.country else {
            continue;
        };
        for (segment, pair) in ring.line.windows(2).enumerate() {
            let class = ring.classes[segment];
            if !matches!(class, OUTER | COUNTRY) {
                continue;
            }
            match borders.across(pair[0], pair[1], own) {
                Some(across) if draws(own, across) => {
                    ring.classes[segment] = COUNTRY;
                    drawn.push((Arc::new(vec![pair[0], pair[1]]), [own, across]));
                }
                Some(_) => waiting.push((index, segment, own)),
                // An outline segment off any border: a coast or the edge of the loaded
                // area. A cross-country match off Natural Earth's lines is drawn as an
                // ordinary municipal line.
                None if class == COUNTRY => ring.classes[segment] = INTERIOR,
                None => {}
            }
        }
    }
    // Second pass: the other side's segments are hidden where the drawn line runs, and
    // stay ordinary municipal lines where it does not (that side not loaded there yet).
    let drawn = CountryBorders::new(drawn);
    for (index, segment, own) in waiting {
        let ring = &mut rings[index];
        let (a, b) = (ring.line[segment], ring.line[segment + 1]);
        ring.classes[segment] = if drawn.across(a, b, own).is_some() {
            DROP
        } else {
            INTERIOR
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: &[(f64, f64)]) -> Line {
        Arc::new(points.to_vec())
    }

    const DEU: [u8; 3] = *b"DEU";
    const CZE: [u8; 3] = *b"CZE";
    const POL: [u8; 3] = *b"POL";

    fn border_48n() -> CountryBorders {
        // Natural Earth: Germany to the north of 48°N, Czechia to the south.
        CountryBorders::new(vec![(line(&[(48.0, 8.0), (48.0, 9.0)]), [DEU, CZE])])
    }

    #[test]
    fn outline_segments_name_the_border_they_run_along() {
        let borders = border_48n();
        // 2 km off, parallel: the German–Czech border.
        assert_eq!(borders.across((48.02, 8.2), (48.02, 8.4), DEU), Some(CZE));
        assert_eq!(borders.across((47.98, 8.2), (47.98, 8.4), CZE), Some(DEU));
        // Another country's outline does not belong to this border.
        assert_eq!(borders.across((48.02, 8.2), (48.02, 8.4), POL), None);
        // A zig-zagging detailed outline still follows the border.
        assert_eq!(borders.across((48.02, 8.2), (47.99, 8.21), DEU), Some(CZE));
        // Past the end of the border line (a coast where the border reaches the sea),
        // or 10 km away: not the border.
        assert_eq!(borders.across((48.0, 9.03), (48.0, 9.06), DEU), None);
        assert_eq!(borders.across((48.0, 7.94), (48.01, 7.97), DEU), None);
        assert_eq!(borders.across((48.09, 8.2), (48.09, 8.4), DEU), None);
    }

    #[test]
    fn one_side_draws_each_border_and_the_other_is_hidden_where_it_runs() {
        let borders = border_48n();
        // Each side's outline along the border, 2 km apart (two sources), and a German
        // segment far from it (a coast or an inner edge).
        let german = line(&[(48.02, 8.1), (48.02, 8.5), (48.02, 8.9), (48.5, 8.9)]);
        let czech = line(&[(47.99, 8.1), (47.99, 8.5), (47.99, 8.9)]);
        let mut german_classes = vec![OUTER, OUTER, OUTER];
        let mut czech_classes = vec![OUTER, OUTER];
        let mut rings = [
            BorderRing {
                line: &german,
                classes: &mut german_classes,
                country: Some(DEU),
            },
            BorderRing {
                line: &czech,
                classes: &mut czech_classes,
                country: Some(CZE),
            },
        ];
        // Germany's source is preferred.
        resolve(&mut rings, &borders, |a, _| a == DEU);
        assert_eq!(german_classes, [COUNTRY, COUNTRY, OUTER]);
        assert_eq!(czech_classes, [DROP, DROP]);
    }

    #[test]
    fn the_only_loaded_side_draws_and_ties_go_to_the_first_code() {
        let borders = border_48n();
        let czech = line(&[(47.99, 8.1), (47.99, 8.5)]);
        let mut classes = vec![OUTER];
        resolve(
            &mut [BorderRing {
                line: &czech,
                classes: &mut classes,
                country: Some(CZE),
            }],
            &borders,
            |_, _| false,
        );
        assert_eq!(classes, [COUNTRY], "Germany is not loaded: Czechia draws");
        let german = line(&[(48.02, 8.1), (48.02, 8.5)]);
        let (mut cz, mut de) = (vec![OUTER], vec![OUTER]);
        resolve(
            &mut [
                BorderRing {
                    line: &czech,
                    classes: &mut cz,
                    country: Some(CZE),
                },
                BorderRing {
                    line: &german,
                    classes: &mut de,
                    country: Some(DEU),
                },
            ],
            &borders,
            |_, _| false,
        );
        assert_eq!(
            (cz, de),
            (vec![COUNTRY], vec![DROP]),
            "CZE sorts before DEU"
        );
    }
}
