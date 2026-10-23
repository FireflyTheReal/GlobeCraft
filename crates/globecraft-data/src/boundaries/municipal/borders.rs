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
//!    only one loaded. The other side's outline is hidden wherever the chosen line runs
//!    (within about 16 km), and draws the border itself where the chosen side has no
//!    line (a gap in its file). The Natural Earth line of that border is cut wherever
//!    the municipal one runs (`topology::CountryCells`).
//! 4. Land that both countries' files claim is contested: its outline is not drawn as a
//!    country border (it stays a municipal line). Bangladesh's union file still holds
//!    pockets of India given up in the 2015 land swap, which India's file covers; drawn
//!    as country border they looked like stray blobs along the border.
use super::super::{CountryPair, Line};
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
/// An outline segment that both countries' outlines confirm (topology class
/// `COUNTRY`) is a border whatever the Natural Earth line says; that line only names
/// the neighbour, so it may be this far away. Natural Earth's 1:10m lines and the
/// municipal sources differ by 5–10 km in places (the Andes, Central Asia), and the
/// narrow band broke such borders into pieces.
const CONFIRMED_BAND_DEG: f64 = 0.15;
const GRID_DEG: f64 = 0.1;

/// Natural Earth land-border lines in the boundary data, with the two countries each
/// separates (`None` for lines without that information, such as disputed overlays).
#[derive(Clone, Copy)]
pub struct CountryBorderLines<'a> {
    pub lines: &'a [Line],
    pub pairs: &'a [Option<CountryPair>],
    /// Natural Earth's country outlines (outer rings), for telling land from sea.
    pub(crate) land: &'a [super::super::CountryRegion],
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
    /// The outlines' rings, for [`Land`].
    pub(super) fn land_rings(self) -> Vec<Line> {
        self.land
            .iter()
            .flat_map(|region| region.rings.iter().map(|ring| ring.line.clone()))
            .collect()
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
    /// Where there is land, from Natural Earth's country outlines, when known.
    land: Option<Land>,
}

/// Land from Natural Earth's country outlines (outer rings: lakes count as land), for
/// telling a land border from a coast. Edges are kept per latitude row, so a point is
/// tested against the edges of its row only.
pub(super) struct Land {
    rows: HashMap<i32, Vec<RingEdge>>,
}

/// An outline edge and the ring it belongs to.
type RingEdge = (u32, [(f64, f64); 2]);

const LAND_ROW_DEG: f64 = 0.05;
/// How far to each side of an outline segment land is looked for (about 1 km).
const LAND_PROBE_DEG: f64 = 0.009;

impl Land {
    pub(super) fn new(rings: &[Line]) -> Self {
        let mut rows: HashMap<i32, Vec<RingEdge>> = HashMap::new();
        for (index, ring) in rings.iter().enumerate() {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                if a.0 == b.0 {
                    continue;
                }
                let row = |lat: f64| (lat / LAND_ROW_DEG).floor() as i32;
                for r in row(a.0.min(b.0))..=row(a.0.max(b.0)) {
                    rows.entry(r).or_default().push((index as u32, [a, b]));
                }
            }
        }
        Self { rows }
    }

    /// Whether the point is inside any outline (even-odd per ring).
    pub(super) fn contains(&self, (lat, lon): (f64, f64)) -> bool {
        let Some(edges) = self.rows.get(&((lat / LAND_ROW_DEG).floor() as i32)) else {
            return false;
        };
        // Rings crossed west of the point, each toggled per crossing: few per row.
        let mut odd: Vec<u32> = Vec::new();
        for &(ring, [a, b]) in edges {
            if (a.0 > lat) != (b.0 > lat) && lon < a.1 + (lat - a.0) * (b.1 - a.1) / (b.0 - a.0) {
                match odd.iter().position(|&r| r == ring) {
                    Some(i) => {
                        odd.swap_remove(i);
                    }
                    None => odd.push(ring),
                }
            }
        }
        !odd.is_empty()
    }

    /// Whether there is land about 1 km to both sides of the segment: a land border,
    /// not a coast.
    fn on_both_sides(&self, a: (f64, f64), b: (f64, f64)) -> bool {
        sides(a, b).is_some_and(|sides| sides.iter().all(|&point| self.contains(point)))
    }
}

fn cell(lat: f64, lon: f64) -> (i32, i32) {
    (
        (lat / GRID_DEG).floor() as i32,
        (lon / GRID_DEG).floor() as i32,
    )
}

impl CountryBorders {
    /// As [`CountryBorders::new`], also knowing where there is land.
    pub(super) fn with_land(lines: Vec<(Line, [[u8; 3]; 2])>, land: Option<Land>) -> Self {
        Self {
            land,
            ..Self::new(lines)
        }
    }

    /// Whether an outline segment may be a land border: land on both sides, or land
    /// unknown.
    fn land_border(&self, a: (f64, f64), b: (f64, f64)) -> bool {
        self.land
            .as_ref()
            .is_none_or(|land| land.on_both_sides(a, b))
    }

    pub(super) fn new(lines: Vec<(Line, [[u8; 3]; 2])>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<(u32, u32)>> = HashMap::new();
        for (index, (line, _)) in lines.iter().enumerate() {
            for (segment, pair) in line.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                let margin = CONFIRMED_BAND_DEG;
                let margin_lon = margin / lon_scale(a.0.abs().max(b.0.abs()) + margin);
                let (y0, x0) = cell(a.0.min(b.0) - margin, a.1.min(b.1) - margin_lon);
                let (y1, x1) = cell(a.0.max(b.0) + margin, a.1.max(b.1) + margin_lon);
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        grid.entry((y, x))
                            .or_default()
                            .push((index as u32, segment as u32));
                    }
                }
            }
        }
        Self {
            lines,
            grid,
            land: None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The country across the nearest indexed border of `country` within the band of
    /// `point`.
    fn nearest(&self, point: (f64, f64), country: [u8; 3], band: f64) -> Option<[u8; 3]> {
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
                (nearest.distance_sq <= band.powi(2) && !past_free_end)
                    .then_some((nearest.distance_sq, across))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, across)| across)
    }

    /// The country across the border that the segment `a`–`b` of an outline of
    /// `country` runs along, if it runs along one.
    /// `confirmed`: both countries' outlines run here (see `CONFIRMED_BAND_DEG`).
    pub(super) fn across(
        &self,
        a: (f64, f64),
        b: (f64, f64),
        country: [u8; 3],
        confirmed: bool,
    ) -> Option<[u8; 3]> {
        if a == b {
            return None;
        }
        let band = if confirmed {
            CONFIRMED_BAND_DEG
        } else {
            BAND_DEG
        };
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let across = self.nearest(mid, country, band)?;
        [a, b]
            .iter()
            .all(|&end| self.nearest(end, country, band) == Some(across))
            .then_some(across)
    }
}

/// A ring's points and the country holding it.
type OwnedRing<'a> = (&'a [(f64, f64)], [u8; 3]);

/// Which country's rings cover a point, for finding land that two countries claim.
struct Contested<'a> {
    rings: Vec<OwnedRing<'a>>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

const CONTESTED_GRID_DEG: f64 = 0.25;

impl<'a> Contested<'a> {
    fn new(rings: &[BorderRing<'a>]) -> Self {
        let cell = |lat: f64, lon: f64| {
            (
                (lat / CONTESTED_GRID_DEG).floor() as i32,
                (lon / CONTESTED_GRID_DEG).floor() as i32,
            )
        };
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut kept = Vec::new();
        for ring in rings {
            let Some(country) = ring.country else {
                continue;
            };
            let line: &'a [(f64, f64)] = ring.line;
            let Some((min, max)) = bounds(line) else {
                continue;
            };
            let ((y0, x0), (y1, x1)) = (cell(min.0, min.1), cell(max.0, max.1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.entry((y, x)).or_default().push(kept.len() as u32);
                }
            }
            kept.push((line, country));
        }
        Self { rings: kept, grid }
    }

    /// Whether `country`'s rings cover `point` (even-odd over all of them, so holes
    /// count as uncovered).
    fn covers(&self, country: [u8; 3], point: (f64, f64)) -> bool {
        let key = (
            (point.0 / CONTESTED_GRID_DEG).floor() as i32,
            (point.1 / CONTESTED_GRID_DEG).floor() as i32,
        );
        self.grid
            .get(&key)
            .into_iter()
            .flatten()
            .map(|&index| self.rings[index as usize])
            .filter(|(_, owner)| *owner == country)
            .fold(false, |inside, (ring, _)| {
                inside ^ super::super::parents::encloses(ring, point)
            })
    }

    /// Whether the land inside `ring`, which `own` holds, is also held by `across`.
    /// A hole in `own`'s land (an enclave of `across`) is not `own`'s land, so its
    /// border stays a border.
    fn claimed_by(&self, ring: &Line, own: [u8; 3], across: [u8; 3]) -> bool {
        let Some(point) = super::super::parents::interior_point(std::slice::from_ref(ring)) else {
            return false;
        };
        self.covers(own, point) && self.covers(across, point)
    }
}

fn bounds(line: &[(f64, f64)]) -> Option<((f64, f64), (f64, f64))> {
    let first = *line.first()?;
    Some(line.iter().fold((first, first), |(min, max), p| {
        (
            (min.0.min(p.0), min.1.min(p.1)),
            (max.0.max(p.0), max.1.max(p.1)),
        )
    }))
}

/// The points about 1 km to either side of a segment's middle.
fn sides(a: (f64, f64), b: (f64, f64)) -> Option<[(f64, f64); 2]> {
    sides_at(a, b, LAND_PROBE_DEG)
}

/// The points `distance_deg` to either side of a segment's middle.
fn sides_at(a: (f64, f64), b: (f64, f64), distance_deg: f64) -> Option<[(f64, f64); 2]> {
    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let scale = lon_scale(mid.0);
    let (dy, dx) = (b.0 - a.0, (b.1 - a.1) * scale);
    let length = dy.hypot(dx);
    if length == 0.0 {
        return None;
    }
    let (ny, nx) = (-dx / length * distance_deg, dy / length * distance_deg);
    Some([1.0, -1.0].map(|side| (mid.0 + side * ny, mid.1 + side * nx / scale)))
}

/// How far across an outline segment the drawing side's land is looked for (about 1,
/// 2.5 and 5 km): two countries' files often leave a gap of a few km between them.
const ACROSS_PROBES_DEG: [f64; 3] = [0.009, 0.0225, 0.045];

/// Holes of up to this length in a drawn border are bridged (see [`bridge`]).
const BRIDGE_KM: f64 = 2.0;

/// What the first pass of [`resolve`] made of an outline segment.
#[derive(Clone, Copy, PartialEq)]
enum Segment {
    /// Not an outline candidate, or the other side's copy of a border.
    Other,
    /// An outline segment that did not qualify as border.
    Open,
    /// Drawn as this border.
    Drawn(CountryPair),
}

/// Short open stretches of a ring between two drawn stretches of the same border, with
/// that border. Where the neighbour's outline bends away from this one for a few hundred
/// metres (beyond the shared-segment tolerance), those segments did not qualify and the
/// border was drawn with small holes.
fn bridge(line: &[(f64, f64)], states: &[Segment]) -> Vec<(usize, CountryPair)> {
    let n = states.len();
    let closed = n > 2 && line.first() == line.last();
    let length_km = |segment: usize| {
        let (a, b) = (line[segment], line[segment + 1]);
        let scale = lon_scale((a.0 + b.0) / 2.0);
        ((b.0 - a.0).hypot((b.1 - a.1) * scale)) * 111.32
    };
    let mut out = Vec::new();
    let mut start = 0;
    while start < n {
        if states[start] != Segment::Open {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < n && states[end] == Segment::Open {
            end += 1;
        }
        // The drawn segments on either side (around the ring's seam if it is closed).
        let before = match start {
            0 if closed => Some(states[n - 1]),
            0 => None,
            _ => Some(states[start - 1]),
        };
        let after = match end {
            _ if end == n && closed => Some(states[0]),
            _ if end == n => None,
            _ => Some(states[end]),
        };
        if let (Some(Segment::Drawn(a)), Some(Segment::Drawn(b))) = (before, after)
            && a == b
            && (start..end).map(length_km).sum::<f64>() <= BRIDGE_KM
        {
            out.extend((start..end).map(|segment| (segment, a)));
        }
        start = end;
    }
    out
}

/// A border's two countries in a fixed order, so both sides name it alike.
pub(super) fn border_pair(a: [u8; 3], b: [u8; 3]) -> CountryPair {
    if a <= b { [a, b] } else { [b, a] }
}

/// One ring in an assembled area: its points, its classes, and its country.
pub(super) struct BorderRing<'a> {
    pub(super) line: &'a Line,
    pub(super) classes: &'a mut Vec<u8>,
    /// For each segment drawn as country border, the border it draws (set here).
    pub(super) pairs: &'a mut Vec<Option<CountryPair>>,
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
    let contested = Contested::new(rings);
    let loaded: HashSet<[u8; 3]> = rings.iter().filter_map(|ring| ring.country).collect();
    let draws = |own: [u8; 3], across: [u8; 3]| {
        !loaded.contains(&across) || better(own, across) || (!better(across, own) && own < across)
    };
    // First pass: classify outline segments; the drawing side's become `COUNTRY` and are
    // collected, the other side's wait for the second pass.
    let mut drawn: Vec<(Line, [[u8; 3]; 2])> = Vec::new();
    let mut waiting: Vec<(usize, usize, [u8; 3], [u8; 3])> = Vec::new();
    for (index, ring) in rings.iter_mut().enumerate() {
        let Some(own) = ring.country else {
            continue;
        };
        // Asked once per ring and neighbour, only for rings along a border.
        let mut claimed: HashMap<[u8; 3], bool> = HashMap::new();
        let mut states = vec![Segment::Other; ring.classes.len()];
        for (segment, pair) in ring.line.windows(2).enumerate() {
            let class = ring.classes[segment];
            if !matches!(class, OUTER | COUNTRY) {
                continue;
            }
            states[segment] = Segment::Open;
            let mut is_claimed = |across: [u8; 3]| {
                *claimed
                    .entry(across)
                    .or_insert_with(|| contested.claimed_by(ring.line, own, across))
            };
            // A segment the neighbour's outline does not confirm must have land on both
            // sides: a coast near a border line is not the border.
            let across = borders
                .across(pair[0], pair[1], own, class == COUNTRY)
                .filter(|_| class == COUNTRY || borders.land_border(pair[0], pair[1]));
            match across {
                // Contested land is not drawn as a border by the drawing side; the
                // other side's handling below is unchanged, so its copy stays hidden
                // where a drawn line runs.
                Some(across) if draws(own, across) && !is_claimed(across) => {
                    ring.classes[segment] = COUNTRY;
                    if let Some(slot) = ring.pairs.get_mut(segment) {
                        *slot = Some(border_pair(own, across));
                    }
                    states[segment] = Segment::Drawn(border_pair(own, across));
                }
                Some(across) if !draws(own, across) => {
                    waiting.push((index, segment, own, across));
                    states[segment] = Segment::Other;
                }
                // An outline segment off any border (a coast or the edge of the loaded
                // area), or around contested land. A cross-country match off Natural
                // Earth's lines is drawn as an ordinary municipal line.
                _ if class == COUNTRY => ring.classes[segment] = INTERIOR,
                _ => {}
            }
        }
        for (segment, pair) in bridge(ring.line, &states) {
            ring.classes[segment] = COUNTRY;
            ring.pairs[segment] = Some(pair);
        }
        // The drawn line as joined runs, so only its real ends count as ends.
        let mut run: Vec<(f64, f64)> = Vec::new();
        let mut run_pair = None;
        for segment in 0..ring.classes.len() {
            let pair = ring.pairs.get(segment).copied().flatten();
            if pair != run_pair || pair.is_none() {
                if let Some(done) = run_pair.filter(|_| run.len() >= 2) {
                    drawn.push((Arc::new(std::mem::take(&mut run)), done));
                }
                run.clear();
                run_pair = pair;
            }
            if pair.is_some() {
                if run.is_empty() {
                    run.push(ring.line[segment]);
                }
                run.push(ring.line[segment + 1]);
            }
        }
        if let Some(done) = run_pair.filter(|_| run.len() >= 2) {
            drawn.push((Arc::new(run), done));
        }
    }
    // Second pass: the other side's segments are hidden where the drawn line runs, stay
    // ordinary municipal lines where another version of the border is within reach (so
    // two versions are never both drawn as border), and draw the border themselves where
    // the drawing side has no line at all (a gap in its file).
    let drawn = CountryBorders::new(drawn);
    let mut claimed: HashMap<(usize, [u8; 3]), bool> = HashMap::new();
    for (index, segment, own, across) in waiting {
        let ring = &mut rings[index];
        let (a, b) = (ring.line[segment], ring.line[segment + 1]);
        let confirmed = ring.classes[segment] == COUNTRY;
        if drawn.across(a, b, own, false).is_some() {
            // The drawn line runs here: this copy is hidden.
            ring.classes[segment] = DROP;
        } else if drawn.across(a, b, own, true).is_some() {
            // Another version of the border runs within reach: this outline stays an
            // ordinary municipal line, so its municipalities stay closed.
            ring.classes[segment] = INTERIOR;
        } else if (!confirmed && !borders.land_border(a, b))
            // The drawing side's own land must lie across this segment: where its file
            // has no land here at all, this is no border it shares (a coast facing land
            // that only Natural Earth gives it).
            || !ACROSS_PROBES_DEG.iter().any(|&distance| {
                sides_at(a, b, distance).is_some_and(|sides| {
                    sides.iter().any(|&point| contested.covers(across, point))
                })
            })
            || *claimed
                .entry((index, across))
                .or_insert_with(|| contested.claimed_by(ring.line, own, across))
        {
            // Disputed land: neither side draws it as a border (see `Contested`).
            ring.classes[segment] = INTERIOR;
        } else {
            ring.classes[segment] = COUNTRY;
            ring.pairs[segment] = Some(border_pair(own, across));
        }
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
        assert_eq!(
            borders.across((48.02, 8.2), (48.02, 8.4), DEU, false),
            Some(CZE)
        );
        assert_eq!(
            borders.across((47.98, 8.2), (47.98, 8.4), CZE, false),
            Some(DEU)
        );
        // Another country's outline does not belong to this border.
        assert_eq!(borders.across((48.02, 8.2), (48.02, 8.4), POL, false), None);
        // A zig-zagging detailed outline still follows the border.
        assert_eq!(
            borders.across((48.02, 8.2), (47.99, 8.21), DEU, false),
            Some(CZE)
        );
        // Past the end of the border line (a coast where the border reaches the sea),
        // or 10 km away: not the border.
        assert_eq!(borders.across((48.0, 9.03), (48.0, 9.06), DEU, false), None);
        assert_eq!(
            borders.across((48.0, 7.94), (48.01, 7.97), DEU, false),
            None
        );
        assert_eq!(borders.across((48.09, 8.2), (48.09, 8.4), DEU, false), None);
        // 10 km off, but both countries' outlines confirm the border there.
        assert_eq!(
            borders.across((48.09, 8.2), (48.09, 8.4), DEU, true),
            Some(CZE)
        );
    }

    #[test]
    fn one_side_draws_each_border_and_the_other_is_hidden_where_it_runs() {
        let mut spare = [(); 4].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1, ..] = &mut spare;
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
                pairs: s0,
                country: Some(DEU),
            },
            BorderRing {
                line: &czech,
                classes: &mut czech_classes,
                pairs: s1,
                country: Some(CZE),
            },
        ];
        // Germany's source is preferred.
        resolve(&mut rings, &borders, |a, _| a == DEU);
        assert_eq!(german_classes, [COUNTRY, COUNTRY, OUTER]);
        assert_eq!(czech_classes, [DROP, DROP]);
    }

    fn rect(lat0: f64, lon0: f64, lat1: f64, lon1: f64) -> Line {
        line(&[
            (lat0, lon0),
            (lat0, lon1),
            (lat1, lon1),
            (lat1, lon0),
            (lat0, lon0),
        ])
    }

    #[test]
    fn land_both_countries_claim_is_not_drawn_as_a_country_border() {
        let mut spare = [(); 4].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1, ..] = &mut spare;
        let borders = border_48n();
        // A stale Czech pocket north of the border, inside Germany's own outline.
        let pocket = rect(48.01, 8.3, 48.03, 8.32);
        let germany = rect(48.0, 8.0, 48.2, 9.0);
        let mut pocket_classes = vec![OUTER; 4];
        let mut german_classes = vec![OUTER; 4];
        resolve(
            &mut [
                BorderRing {
                    line: &pocket,
                    classes: &mut pocket_classes,
                    pairs: s0,
                    country: Some(CZE),
                },
                BorderRing {
                    line: &germany,
                    classes: &mut german_classes,
                    pairs: s1,
                    country: Some(DEU),
                },
            ],
            &borders,
            |_, _| false,
        );
        assert!(
            !pocket_classes.contains(&COUNTRY),
            "contested pocket: {pocket_classes:?}"
        );
    }

    #[test]
    fn an_enclave_held_as_a_hole_keeps_its_border() {
        let mut spare = [(); 4].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1, s2, ..] = &mut spare;
        let borders = border_48n();
        // Czech land south of the border with a hole that a German enclave fills.
        let czechia = rect(47.8, 8.0, 47.99, 9.0);
        let hole = rect(47.95, 8.4, 47.97, 8.42);
        let enclave = rect(47.95, 8.4, 47.97, 8.42);
        let (mut cz, mut cz_hole, mut de) = (vec![OUTER; 4], vec![OUTER; 4], vec![OUTER; 4]);
        resolve(
            &mut [
                BorderRing {
                    line: &czechia,
                    classes: &mut cz,
                    pairs: s0,
                    country: Some(CZE),
                },
                BorderRing {
                    line: &hole,
                    classes: &mut cz_hole,
                    pairs: s1,
                    country: Some(CZE),
                },
                BorderRing {
                    line: &enclave,
                    classes: &mut de,
                    pairs: s2,
                    country: Some(DEU),
                },
            ],
            &borders,
            |_, _| false,
        );
        assert_eq!(cz_hole, [COUNTRY; 4], "Czechia draws around the enclave");
        assert_eq!(de, [DROP; 4], "Germany's copy is hidden");
    }

    #[test]
    fn the_other_side_draws_where_the_drawing_side_has_no_line() {
        let mut spare = [(); 2].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1] = &mut spare;
        let borders = border_48n();
        // Germany's land reaches the border, but its edge is one long segment past both
        // ends of the border line (a tripoint), so it does not qualify. Czechia's edge
        // along the border draws instead: German land lies right across it.
        let german = rect(48.0, 7.5, 48.3, 9.5);
        let czech = line(&[(47.99, 8.1), (47.99, 8.4), (47.99, 8.95)]);
        let (mut de, mut cz) = (vec![OUTER; 4], vec![OUTER, OUTER]);
        resolve(
            &mut [
                BorderRing {
                    line: &german,
                    classes: &mut de,
                    pairs: s0,
                    country: Some(DEU),
                },
                BorderRing {
                    line: &czech,
                    classes: &mut cz,
                    pairs: s1,
                    country: Some(CZE),
                },
            ],
            &borders,
            |a, _| a == DEU,
        );
        assert_ne!(de[0], COUNTRY);
        assert_eq!(cz, [COUNTRY, COUNTRY], "Czechia fills Germany's gap");
        assert_eq!(s1[1], Some(border_pair(CZE, DEU)));
        // Where Germany's file has no land across Czechia's edge, Czechia's edge is no
        // border they share (a coast facing land only Natural Earth gives Germany).
        let mut spare = [(); 2].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1] = &mut spare;
        let far_german = rect(48.2, 7.5, 48.3, 9.5);
        let (mut de, mut cz) = (vec![OUTER; 4], vec![OUTER, OUTER]);
        resolve(
            &mut [
                BorderRing {
                    line: &far_german,
                    classes: &mut de,
                    pairs: s0,
                    country: Some(DEU),
                },
                BorderRing {
                    line: &czech,
                    classes: &mut cz,
                    pairs: s1,
                    country: Some(CZE),
                },
            ],
            &borders,
            |a, _| a == DEU,
        );
        assert!(!cz.contains(&COUNTRY), "{cz:?}");
    }

    #[test]
    fn a_coast_beside_a_border_line_is_not_the_border() {
        // Land north of 47.97° N only (the sea lies south), with the Natural Earth border
        // along 48.0° N.
        let land = Land::new(&[rect(47.97, 7.0, 49.0, 10.0)]);
        assert!(land.contains((48.5, 8.0)) && !land.contains((47.8, 8.0)));
        assert!(land.on_both_sides((48.0, 8.2), (48.0, 8.4)));
        assert!(!land.on_both_sides((47.97, 8.2), (47.97, 8.4)), "a coast");
        let borders = CountryBorders::with_land(
            vec![(line(&[(48.0, 8.0), (48.0, 9.0)]), [DEU, CZE])],
            Some(land),
        );
        let mut spare = [(); 2].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1] = &mut spare;
        // A German outline along the border, and a Czech coast 3 km south of it, well
        // inside the band that would take it for the border.
        let german = line(&[(48.01, 8.1), (48.01, 8.5)]);
        let coast = line(&[(47.97, 8.1), (47.97, 8.5)]);
        let (mut de, mut cz) = (vec![OUTER], vec![OUTER]);
        resolve(
            &mut [
                BorderRing {
                    line: &german,
                    classes: &mut de,
                    pairs: s0,
                    country: Some(DEU),
                },
                BorderRing {
                    line: &coast,
                    classes: &mut cz,
                    pairs: s1,
                    country: Some(CZE),
                },
            ],
            &CountryBorders { ..borders },
            |_, _| false,
        );
        assert_eq!(cz, [OUTER], "the coast stays a coast");
        // Czechia's file has no land across the German edge either: no border drawn.
        assert_ne!(de, [COUNTRY]);
    }

    #[test]
    fn short_holes_in_a_drawn_border_are_bridged() {
        // Six 1.1 km segments along 48.01° N.
        let line: Vec<(f64, f64)> = (0..=6).map(|i| (48.01, 8.0 + i as f64 * 0.015)).collect();
        let pair = border_pair(DEU, CZE);
        let (d, o) = (Segment::Drawn(pair), Segment::Open);
        // One open segment between two drawn ones: bridged.
        assert_eq!(bridge(&line, &[d, d, o, d, d, d]), [(2, pair)]);
        // Three open segments (3.3 km): too long, a real gap in the data.
        assert!(bridge(&line, &[d, o, o, o, d, d]).is_empty());
        // Between two different borders, or at the end of an open line: not bridged.
        let other = Segment::Drawn(border_pair(DEU, POL));
        assert!(bridge(&line, &[d, d, o, other, d, d]).is_empty());
        assert!(bridge(&line, &[d, d, d, d, d, o]).is_empty());
    }

    #[test]
    fn the_only_loaded_side_draws_and_ties_go_to_the_first_code() {
        let mut spare = [(); 4].map(|_| vec![None::<CountryPair>; 8]);
        let [s0, s1, s2, ..] = &mut spare;
        let borders = border_48n();
        let czech = line(&[(47.99, 8.1), (47.99, 8.5)]);
        let mut classes = vec![OUTER];
        resolve(
            &mut [BorderRing {
                line: &czech,
                classes: &mut classes,
                pairs: s0,
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
                    pairs: s1,
                    country: Some(CZE),
                },
                BorderRing {
                    line: &german,
                    classes: &mut de,
                    pairs: s2,
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
