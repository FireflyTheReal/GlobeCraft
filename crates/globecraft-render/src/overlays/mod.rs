//! Geographic overlay projection and occupancy, independent of a widget toolkit.
use emath::{Pos2, Rect};
use globecraft_core::{camera::Basis, math::wrap_lon};
use globecraft_data::boundaries::{
    BoundaryData, LabelKind, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM, MapLabel, OSM_MUNICIPAL_MIN_ZOOM,
    STATE_BIG_VISIBLE_MAX_ALTITUDE_KM,
};
use globecraft_data::settings::map_style::{DashStyle, LineType, MapStyle};
use std::collections::HashMap;
pub mod grid;
mod labels;
mod spatial;
mod worker;
pub use labels::LabelIndex;
pub use spatial::{BoundaryIndex, VisibleBoundaries};
pub use worker::{
    OverlayGeometry, OverlayInput, OverlayPreparer, OverlayView, OverlayWorker, PreparedLabel,
};
const SIMPLIFY_BORDER_ALTITUDE_KM: f64 = 1500.0;
const MIN_BORDER_SEGMENT_POINTS: f32 = 1.4;
const MAX_BORDER_SUBDIVISIONS: usize = 96;
const CLOSE_BORDER_ALTITUDE_KM: f64 = 500.0;
/// Screen-space dash patterns, on/off in points. States and municipalities use the
/// dashed pattern by default, so the states→municipal handoff keeps one visual.
const DASHED_ON_POINTS: f32 = 7.0;
const DASHED_OFF_POINTS: f32 = 5.0;
const DOTTED_ON_POINTS: f32 = 1.5;
const DOTTED_OFF_POINTS: f32 = 3.0;
pub const MAX_VISIBLE_LABELS: usize = 500;
/// Coast lines are drawn where the sun is below the horizon (the cosine of the sun's
/// angle from the zenith is below this): the imagery's twilight is already dark there.
const NIGHT_COAST_MAX_SUN_DOT: f64 = 0.0;
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OverlayLayers {
    pub countries: bool,
    pub states: bool,
    pub cities: bool,
    pub labels: bool,
    /// Sunlight on: the subsolar point (latitude, longitude), to the minute. Country
    /// outlines along the sea are then drawn on the night side, where the imagery no
    /// longer shows the coast (roadmap 13.4).
    pub sun: Option<(f64, f64)>,
}
pub struct OverlayProjection<'a> {
    pub basis: &'a Basis,
    pub rect: Rect,
    pub altitude_km: f64,
    /// Camera altitude (km) below which regular state/province lines and names
    /// appear; the app passes its user-tunable setting here. Big-country
    /// provinces keep their own earlier, fixed threshold.
    pub state_visible_max_km: f64,
    pub layers: OverlayLayers,
    pub dashes: BorderDashes,
}

/// On/off dash lengths in points for each styled line type; `None` draws a solid line.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BorderDashes {
    pub country: Option<(f32, f32)>,
    pub state: Option<(f32, f32)>,
    pub municipal: Option<(f32, f32)>,
    pub disputed: Option<(f32, f32)>,
}
impl BorderDashes {
    pub fn of(style: &MapStyle) -> Self {
        Self {
            country: dash_points(style.country.dash),
            state: dash_points(style.state.dash),
            municipal: dash_points(style.municipal.dash),
            disputed: dash_points(style.disputed.dash),
        }
    }
    fn of_kind(self, kind: BorderKind) -> Option<(f32, f32)> {
        match kind.line_type() {
            LineType::Country => self.country,
            LineType::State => self.state,
            LineType::Municipal => self.municipal,
            LineType::Disputed => self.disputed,
        }
    }
}
impl Default for BorderDashes {
    fn default() -> Self {
        Self::of(&MapStyle::default())
    }
}

/// The on/off pattern of a dash style, in points.
pub fn dash_points(dash: DashStyle) -> Option<(f32, f32)> {
    match dash {
        DashStyle::Solid => None,
        DashStyle::Dashed => Some((DASHED_ON_POINTS, DASHED_OFF_POINTS)),
        DashStyle::Dotted => Some((DOTTED_ON_POINTS, DOTTED_OFF_POINTS)),
    }
}

#[derive(Clone, Copy)]
pub enum BorderKind {
    Country,
    /// Provinces of the big countries; drawn earlier than `State` and faded
    /// across its own band, but with the same color, width and dash.
    StateBig,
    State,
    /// Municipal lines, drawn only below the data's detail altitude.
    City,
    /// Disputed country borders and claim lines, from the OpenStreetMap tiles.
    Disputed,
}

impl BorderKind {
    /// The user-styled line type this kind draws with.
    pub fn line_type(self) -> LineType {
        match self {
            Self::Country => LineType::Country,
            Self::StateBig | Self::State => LineType::State,
            Self::City => LineType::Municipal,
            Self::Disputed => LineType::Disputed,
        }
    }
}

/// Splits one projected segment into dash fragments, carrying the phase across
/// joined segments of the same polyline. A discontinuity resets the phase so
/// every line starts with a dash instead of a random gap.
fn emit_dashed(
    emit: &mut impl FnMut(BorderKind, [Pos2; 2]),
    kind: BorderKind,
    [from, to]: [Pos2; 2],
    on: f32,
    off: f32,
    phase: &mut f32,
    last_end: &mut Option<Pos2>,
) {
    let length = (to - from).length();
    if length <= f32::EPSILON {
        return;
    }
    if last_end.is_none_or(|end| end.distance_sq(from) > 0.25) {
        *phase = 0.0;
    }
    *last_end = Some(to);
    let cycle = on + off;
    let direction = (to - from) / length;
    let mut walked = 0.0_f32;
    while walked < length {
        let position = phase.rem_euclid(cycle);
        if position < on {
            let piece = (on - position).min(length - walked);
            emit(
                kind,
                [
                    from + direction * walked,
                    from + direction * (walked + piece),
                ],
            );
            walked += piece;
            *phase += piece;
        } else {
            let piece = (cycle - position).min(length - walked);
            walked += piece;
            *phase += piece;
        }
    }
    *phase = phase.rem_euclid(cycle);
}

pub fn project_borders(
    view: &OverlayProjection<'_>,
    boundaries: &BoundaryData,
    visible: Option<&VisibleBoundaries>,
    mut emit: impl FnMut(BorderKind, [Pos2; 2]),
) {
    let (basis, rect) = (view.basis, view.rect);
    let min_sq: f32 = if view.altitude_km > SIMPLIFY_BORDER_ALTITUDE_KM {
        MIN_BORDER_SEGMENT_POINTS * MIN_BORDER_SEGMENT_POINTS
    } else {
        0.0
    };
    let mut dash_phase = 0.0_f32;
    let mut dash_last_end: Option<Pos2> = None;
    let dashes = view.dashes;
    let mut emit_segment = |kind: BorderKind, segment: [Pos2; 2]| match dashes.of_kind(kind) {
        Some((on, off)) => emit_dashed(
            &mut emit,
            kind,
            segment,
            on,
            off,
            &mut dash_phase,
            &mut dash_last_end,
        ),
        None => emit(kind, segment),
    };
    let mut draw_line = |line: &[(f64, f64)], kind: BorderKind, max_step_deg: f64| {
        {
            {
                let mut anchor: Option<Pos2> = None;
                for pair in line.array_windows::<2>() {
                    let (lat1, lon1) = pair[0];
                    let (lat2, lon2) = pair[1];
                    let dlat = lat2 - lat1;
                    let mut dlon = lon2 - lon1;
                    if dlon > 180.0 {
                        dlon -= 360.0;
                    }
                    if dlon < -180.0 {
                        dlon += 360.0;
                    }
                    let start = basis.project(basis.surface_position(lat1, lon1), rect);
                    let end = basis.project(basis.surface_position(lat2, lon2), rect);
                    if let (Some(a), Some(b)) = (start, end) {
                        // Common case: both endpoints are visible, so no extra sampling needed.
                        // The anchor stays on the last emitted vertex: short steps accumulate
                        // against it until the path strays `min_sq` away, then one chord is
                        // emitted. Adjacent polyline pairs overlap, so measuring each step
                        // against the previous vertex instead would drop fine runs entirely
                        // and leave dotted borders.
                        let from = anchor.unwrap_or(a);
                        if (b - from).length_sq() >= min_sq {
                            emit_segment(kind, [from, b]);
                            anchor = Some(b);
                        } else if anchor.is_none() {
                            anchor = Some(a);
                        }
                        continue;
                    }
                    anchor = None;
                    let span = dlat.abs().max(dlon.abs());
                    // Below one subdivision step the chord is already the right
                    // resolution; dropping it instead would leave a gap when
                    // only one short hop crosses the horizon.
                    let steps =
                        ((span / max_step_deg).ceil() as usize).clamp(1, MAX_BORDER_SUBDIVISIONS);
                    let mut prev = start;
                    for i in 1..=steps {
                        let t = i as f64 / steps as f64;
                        let lon = wrap_lon(lon1 + dlon * t);
                        let lat = lat1 + dlat * t;
                        let cur = basis.project(basis.surface_position(lat, lon), rect);
                        if let (Some(a), Some(b)) = (prev, cur) {
                            emit_segment(kind, [a, b]);
                        }
                        prev = cur;
                    }
                }
            }
        }
    };
    let close_zoom = view.altitude_km <= CLOSE_BORDER_ALTITUDE_KM;
    let mut draw = |lines: &[globecraft_data::boundaries::Line], kind: BorderKind, step: f64| {
        for line in lines {
            if visible.is_none_or(|set| set.contains_line(line)) {
                draw_line(line, kind, step);
            }
        }
    };
    let states_far = STATE_BIG_VISIBLE_MAX_ALTITUDE_KM.max(view.state_visible_max_km);
    let osm = &boundaries.osm_borders;
    if osm.is_empty() {
        // Until the first border tile is loaded (or offline): Natural Earth's bundled
        // land borders, as country lines, or closing the states with only those on.
        let kind = if view.layers.countries {
            Some(BorderKind::Country)
        } else if view.layers.states && view.altitude_km < states_far {
            Some(BorderKind::StateBig)
        } else {
            None
        };
        if let Some(kind) = kind {
            let step = if close_zoom { 0.25 } else { 2.0 };
            draw(&boundaries.countries, kind, step);
        }
        return;
    }
    // Borders from OpenStreetMap vector tiles: one line per border, already chosen for
    // the zoom level. The buttons keep their meaning: a line belongs to the lowest level
    // it bounds, and with that level's layer off it is drawn in the style of the next
    // layer that is on, so no area is left open.
    let step = if close_zoom { 0.12 } else { 1.0 };
    let municipal = view.layers.cities
        && view.altitude_km <= MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM
        && osm.zoom >= OSM_MUNICIPAL_MIN_ZOOM
        && view.altitude_km <= boundaries.municipal_detail_max_km;
    if view.layers.states {
        if view.altitude_km < STATE_BIG_VISIBLE_MAX_ALTITUDE_KM {
            draw(&osm.state_big, BorderKind::StateBig, step);
        }
        if view.altitude_km < view.state_visible_max_km {
            draw(&osm.state, BorderKind::State, step);
        }
        if !view.layers.countries && view.altitude_km < states_far {
            draw(&osm.country, BorderKind::StateBig, step);
            draw(&osm.disputed, BorderKind::StateBig, step);
        }
    }
    if municipal {
        draw(&osm.municipal, BorderKind::City, step);
        if !view.layers.states {
            draw(&osm.state_big, BorderKind::City, step);
            draw(&osm.state, BorderKind::City, step);
            if !view.layers.countries {
                draw(&osm.country, BorderKind::City, step);
                draw(&osm.disputed, BorderKind::City, step);
            }
        }
    }
    if view.layers.countries {
        draw(&osm.country, BorderKind::Country, step);
        draw(&osm.disputed, BorderKind::Disputed, step);
    }
    // At night the imagery no longer shows where land ends, so the country outline
    // along the sea is drawn there (roadmap 13.4), in the style of the outermost
    // border layer that is on. Daylit coasts stay undrawn, as the user asked.
    let coast_kind = if view.layers.countries {
        Some(BorderKind::Country)
    } else if view.layers.states && view.altitude_km < states_far {
        Some(BorderKind::StateBig)
    } else {
        None
    };
    if let (Some((sun_lat, sun_lon)), Some(kind)) = (view.layers.sun, coast_kind) {
        let sun = globecraft_core::math::ll_to_v3(sun_lat, sun_lon);
        for line in osm.coast.iter() {
            if visible.is_some_and(|set| !set.contains_line(line)) {
                continue;
            }
            for run in night_runs(line, sun) {
                draw_line(&line[run], kind, step);
            }
        }
    }
}

/// The stretches of `line` on the night side: runs of segments with at least one end
/// in the dark, so each run reaches the terminator.
fn night_runs(line: &[(f64, f64)], sun: glam::DVec3) -> Vec<std::ops::RangeInclusive<usize>> {
    let night: Vec<bool> = line
        .iter()
        .map(|&(lat, lon)| {
            globecraft_core::math::ll_to_v3(lat, lon).dot(sun) < NIGHT_COAST_MAX_SUN_DOT
        })
        .collect();
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..line.len().saturating_sub(1) {
        match (night[i] || night[i + 1], start) {
            (true, None) => start = Some(i),
            (false, Some(first)) => {
                runs.push(first..=i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start {
        runs.push(first..=line.len() - 1);
    }
    runs
}

pub struct ProjectedLabel<'a> {
    pub position: Pos2,
    pub label: &'a MapLabel,
    pub source_index: usize,
}
/// Whether a country's name may render: the country must span at least
/// `COUNTRY_LABEL_MIN_EXTENT_PX` on screen, so dots never carry names.
/// Countries without retained region data stay visible, matching the
/// behavior before apparent-size gating existed.
fn country_label_apparent(
    label: &MapLabel,
    basis: &Basis,
    rect: Rect,
    boundaries: &BoundaryData,
) -> bool {
    let Some(bounds) = boundaries.country_bounds_at(label.lat, label.lon) else {
        return true;
    };
    let corners = [
        (bounds.min_lat, bounds.min_lon),
        (bounds.min_lat, bounds.max_lon),
        (bounds.max_lat, bounds.min_lon),
        (bounds.max_lat, bounds.max_lon),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut measured = false;
    for (lat, lon) in corners {
        if let Some(p) = basis.project(basis.surface_position(lat, lon), rect) {
            measured = true;
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
        }
    }
    if !measured {
        return true;
    }
    (max_x - min_x).max(max_y - min_y) >= COUNTRY_LABEL_MIN_EXTENT_PX
}
/// A country's name only appears once the country spans this many pixels on
/// screen; smaller shapes would render a name for a dot-sized object.
const COUNTRY_LABEL_MIN_EXTENT_PX: f32 = 24.0;
/// Two labels with the same name this close (degrees) name the same place.
const SAME_NAME_DEG: f64 = 0.3;
pub fn project_labels<'a>(
    view: &OverlayProjection<'_>,
    boundaries: &'a BoundaryData,
) -> Vec<ProjectedLabel<'a>> {
    if !view.layers.labels {
        return Vec::new();
    }
    let (basis, rect) = (view.basis, view.rect);
    let mut cands: Vec<(u8, u8, Pos2, &MapLabel, usize)> = Vec::new();
    for (source_index, label) in boundaries.labels.iter().enumerate() {
        // States are gated entirely here because their threshold is the app's
        // user-tunable setting; `visible_at` only knows the shipped default.
        let enabled = match label.kind {
            LabelKind::Country => {
                label.visible_at(view.altitude_km)
                    && country_label_apparent(label, basis, rect, boundaries)
            }
            LabelKind::City => {
                label.visible_with_detail(view.altitude_km, boundaries.municipal_detail_max_km)
            }
            LabelKind::State => {
                view.layers.states
                    && if label.big {
                        view.altitude_km < STATE_BIG_VISIBLE_MAX_ALTITUDE_KM
                    } else {
                        view.altitude_km < view.state_visible_max_km
                    }
            }
        };
        if !enabled {
            continue;
        }
        if let Some(pos) = basis.project(basis.surface_position(label.lat, label.lon), rect)
            && rect.contains(pos)
        {
            let prio = match label.kind {
                LabelKind::Country => 0,
                LabelKind::State => 1,
                LabelKind::City => 2,
            };
            cands.push((prio, label.rank, pos, label, source_index));
        }
    }
    cands.sort_by_key(|c| (c.0, c.1));
    // A tile's place name is skipped where a label already shown carries the same name
    // nearby (the city from Natural Earth), or an earlier copy of the place (an island
    // tagged as both a point and an outline).
    let mut shown: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    cands
        .into_iter()
        .filter(|(_, _, _, label, _)| {
            if label.kind != LabelKind::City {
                return true;
            }
            let near = shown.entry(label.en.to_lowercase()).or_default();
            if label.from_tiles
                && near.iter().any(|&(lat, lon)| {
                    (lat - label.lat).abs() < SAME_NAME_DEG
                        && (lon - label.lon).abs() < SAME_NAME_DEG
                })
            {
                return false;
            }
            near.push((label.lat, label.lon));
            true
        })
        .map(|(_, _, position, label, source_index)| ProjectedLabel {
            position,
            label,
            source_index,
        })
        .collect()
}

pub mod ruler;

#[cfg(test)]
mod tests {
    use super::*;

    fn x(segments: &[[Pos2; 2]]) -> Vec<[f32; 2]> {
        segments.iter().map(|[a, b]| [a.x, b.x]).collect()
    }

    fn dashed_x(segments: &[[Pos2; 2]]) -> Vec<[f32; 2]> {
        let mut out: Vec<[Pos2; 2]> = Vec::new();
        let mut phase = 0.0_f32;
        let mut last_end = None;
        for segment in segments {
            emit_dashed(
                &mut |_kind, piece| out.push(piece),
                BorderKind::State,
                *segment,
                4.0,
                2.0,
                &mut phase,
                &mut last_end,
            );
        }
        x(&out)
    }

    #[test]
    fn coasts_are_drawn_from_the_terminator_into_the_night() {
        // The sun over (0°, 0°): night lies beyond 90° east and west.
        let sun = globecraft_core::math::ll_to_v3(0.0, 0.0);
        let line: Vec<(f64, f64)> = (0..=8).map(|i| (0.0, 40.0 + 15.0 * f64::from(i))).collect();
        // 40, 55, 70, 85 are lit; 100 onwards is dark. The run starts at 85°.
        assert_eq!(night_runs(&line, sun), vec![3..=8]);
        let day: Vec<(f64, f64)> = vec![(10.0, -20.0), (10.0, 20.0)];
        assert!(night_runs(&day, sun).is_empty());
    }

    #[test]
    fn dashes_keep_phase_across_joined_segments_and_restart_on_breaks() {
        // 4 pt on / 2 pt off across two joined 6 pt segments: the second segment
        // must continue the pattern instead of restarting it.
        assert_eq!(
            dashed_x(&[
                [Pos2::new(0.0, 0.0), Pos2::new(6.0, 0.0)],
                [Pos2::new(6.0, 0.0), Pos2::new(12.0, 0.0)],
            ]),
            vec![[0.0, 4.0], [6.0, 10.0]]
        );
        // A disjoint segment starts a fresh dash.
        assert_eq!(
            dashed_x(&[[Pos2::new(50.0, 0.0), Pos2::new(53.0, 0.0)]]),
            vec![[50.0, 53.0]]
        );
    }

    #[test]
    fn dashes_follow_the_style_of_each_line_type() {
        let dashes = BorderDashes::default();
        assert_eq!(dashes.of_kind(BorderKind::Country), None);
        assert!(dashes.of_kind(BorderKind::State).is_some());
        assert!(dashes.of_kind(BorderKind::City).is_some());
        let mut style = MapStyle::default();
        style.state.dash = DashStyle::Dotted;
        style.municipal.dash = DashStyle::Solid;
        let dashes = BorderDashes::of(&style);
        assert_eq!(
            dashes.of_kind(BorderKind::StateBig),
            dash_points(DashStyle::Dotted)
        );
        assert_eq!(dashes.of_kind(BorderKind::City), None);
        assert!(dashes.of_kind(BorderKind::Disputed).is_some());
    }

    #[test]
    fn place_names_do_not_repeat_a_name_already_shown_nearby() {
        use globecraft_core::camera::Cam;
        let label = |en: &str, lat: f64, lon: f64, rank: u8| MapLabel {
            lat,
            lon,
            en: en.into(),
            pt: en.into(),
            kind: LabelKind::City,
            rank,
            big: false,
            from_tiles: rank >= globecraft_data::boundaries::PLACE_TOWN_RANK,
        };
        let mut data = BoundaryData::default();
        data.labels = vec![
            // Natural Earth's town, then the tiles' town and a village (ranks 11, 12).
            label("Fergana", 40.38, 71.78, 6),
            label("Fergana", 40.39, 71.79, 11),
            label("Qishloq", 40.42, 71.74, 12),
        ];
        let basis = Cam {
            lat: 40.4,
            lon: 71.77,
            heading: 0.0,
            alt: 30.0,
        }
        .basis(1.0);
        let view = OverlayProjection {
            basis: &basis,
            rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
            altitude_km: 30.0,
            state_visible_max_km: 1609.0,
            dashes: BorderDashes::default(),
            layers: OverlayLayers {
                countries: true,
                states: true,
                cities: false,
                labels: true,
                sun: None,
            },
        };
        let names: Vec<(&str, u8)> = project_labels(&view, &data)
            .iter()
            .map(|projected| (projected.label.en.as_str(), projected.label.rank))
            .collect();
        assert_eq!(names, [("Fergana", 6), ("Qishloq", 12)]);
    }

    #[test]
    fn tile_borders_keep_the_button_rules_and_the_bundled_ones_stand_in() {
        use globecraft_core::camera::Cam;
        use std::sync::Arc;
        let line = |lat: f64| Arc::new(vec![(lat, 8.9), (lat, 9.1)]);
        // Natural Earth's bundled line, which the tiles replace.
        let mut data = BoundaryData::default();
        data.countries = vec![line(48.03)];
        data.municipal_detail_max_km = MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM;
        data.osm_borders = Arc::new(globecraft_data::boundaries::OsmBorderLines {
            country: vec![line(48.0)],
            disputed: vec![line(48.05)],
            state: vec![line(48.01)],
            municipal: vec![line(48.02)],
            zoom: OSM_MUNICIPAL_MIN_ZOOM + 1,
            ..Default::default()
        });
        let basis = Cam {
            lat: 48.0,
            lon: 9.0,
            heading: 0.0,
            alt: 30.0,
        }
        .basis(1.0);
        let draw = |data: &BoundaryData, countries: bool, states: bool, cities: bool| {
            let view = OverlayProjection {
                basis: &basis,
                rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
                altitude_km: 30.0,
                state_visible_max_km: 1609.0,
                dashes: BorderDashes::default(),
                layers: OverlayLayers {
                    countries,
                    states,
                    cities,
                    labels: false,
                    sun: None,
                },
            };
            // Which kinds draw the lines at each latitude, by hundredths of a degree.
            let mut kinds = std::collections::BTreeSet::<(i64, &str)>::new();
            project_borders(&view, data, None, |kind, [a, _]| {
                let lat = basis
                    .pick(view.rect, a)
                    .map_or(-1, |(lat, _)| ((lat - 48.0) * 100.0).round() as i64);
                let name = match kind {
                    BorderKind::Country => "Country",
                    BorderKind::State => "State",
                    BorderKind::StateBig => "StateBig",
                    BorderKind::City => "City",
                    BorderKind::Disputed => "Disputed",
                };
                kinds.insert((lat, name));
            });
            kinds.into_iter().collect::<Vec<_>>()
        };
        assert_eq!(
            draw(&data, true, true, true),
            vec![(0, "Country"), (1, "State"), (2, "City"), (5, "Disputed")],
            "each tile line once, in its own style; nothing from the other sources"
        );
        assert_eq!(
            draw(&data, false, false, true),
            vec![(0, "City"), (1, "City"), (2, "City"), (5, "City")],
            "municipalities alone are closed by the higher borders"
        );
        assert_eq!(
            draw(&data, false, true, true),
            vec![(0, "StateBig"), (1, "State"), (2, "City"), (5, "StateBig")],
            "states close at country borders"
        );
        // Before the first tile: the bundled borders, as country or closing the states.
        data.osm_borders = Arc::default();
        assert_eq!(draw(&data, true, true, true), vec![(3, "Country")]);
        assert_eq!(draw(&data, false, true, false), vec![(3, "StateBig")]);
    }
}
