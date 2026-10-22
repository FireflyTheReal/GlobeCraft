//! Geographic overlay projection and occupancy, independent of a widget toolkit.
use emath::{Pos2, Rect};
use globecraft_core::{camera::Basis, math::wrap_lon};
use globecraft_data::boundaries::{
    BoundaryData, LabelKind, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM, MapLabel,
    STATE_BIG_VISIBLE_MAX_ALTITUDE_KM,
};
use globecraft_data::settings::map_style::{DashStyle, LineType, MapStyle};
pub mod grid;
mod handover;
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
const MEDIUM_BORDER_ALTITUDE_KM: f64 = 5000.0;
/// Screen-space dash patterns, on/off in points. States and municipalities use the
/// dashed pattern by default, so the states→municipal handoff keeps one visual.
const DASHED_ON_POINTS: f32 = 7.0;
const DASHED_OFF_POINTS: f32 = 5.0;
const DOTTED_ON_POINTS: f32 = 1.5;
const DOTTED_OFF_POINTS: f32 = 3.0;
pub const MAX_VISIBLE_LABELS: usize = 500;
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OverlayLayers {
    pub countries: bool,
    pub states: bool,
    pub cities: bool,
    pub labels: bool,
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
}
impl BorderDashes {
    pub fn of(style: &MapStyle) -> Self {
        Self {
            country: dash_points(style.country.dash),
            state: dash_points(style.state.dash),
            municipal: dash_points(style.municipal.dash),
        }
    }
    fn of_kind(self, kind: BorderKind) -> Option<(f32, f32)> {
        match kind.line_type() {
            LineType::Country => self.country,
            LineType::State => self.state,
            LineType::Municipal => self.municipal,
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
    /// Municipal segments on state borders: the accurate version of the state lines,
    /// drawn from the municipal altitude down in place of the stretches they cover.
    CityMajor,
    /// Interior municipal lines, drawn only below the data's detail altitude.
    City,
    /// Municipal segments on country borders, where both neighbours' municipalities
    /// are loaded: drawn in the country style in place of the Natural Earth country
    /// line they cover.
    CityCountry,
}

impl BorderKind {
    /// The user-styled line type this kind draws with. Municipal state borders replace
    /// the state lines up close, so they take the state style.
    pub fn line_type(self) -> LineType {
        match self {
            Self::Country | Self::CityCountry => LineType::Country,
            Self::StateBig | Self::State | Self::CityMajor => LineType::State,
            Self::City => LineType::Municipal,
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
    // Municipal outlines only make sense at close range: they are drawn from
    // `MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM` down.
    let municipal_visible = view.altitude_km <= MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM;
    // Where municipal state borders were loaded, the coarse state lines hand over to
    // them: the covered parts are cut out (see `handover`). Elsewhere the coarse lines
    // stay, since municipal data rarely covers the whole view.
    let municipal_drawn = view.layers.cities && municipal_visible;
    let state_cells = Some(&*boundaries.municipal_major_cells)
        .filter(|cells| municipal_drawn && !cells.is_empty());
    // Country lines hand over the same way, where municipal country borders replace them
    // (only while country borders are shown; otherwise those draw in the state style).
    let country_cells = Some(&*boundaries.municipal_country_cells)
        .filter(|cells| municipal_drawn && view.layers.countries && !cells.is_empty());
    let mut draw_lines =
        |lines: &[globecraft_data::boundaries::Line], kind: BorderKind, max_step_deg: f64| {
            for line in lines {
                if visible.is_some_and(|set| !set.contains_line(line)) {
                    continue;
                }
                let cells = match kind {
                    BorderKind::State | BorderKind::StateBig => state_cells,
                    BorderKind::Country => country_cells,
                    _ => None,
                };
                let Some(cells) = cells else {
                    draw_line(line, kind, max_step_deg);
                    continue;
                };
                for run in handover::uncovered_runs(line, cells) {
                    draw_line(&run, kind, max_step_deg);
                }
            }
        };
    // State/province lines, minus the stretches municipal state borders replace.
    if view.layers.states {
        let step = if close_zoom {
            0.12
        } else if view.altitude_km <= MEDIUM_BORDER_ALTITUDE_KM {
            0.5
        } else {
            2.0
        };

        // Provinces of the big countries read well much earlier than the rest;
        // both the lines and their labels use the same altitude policy.
        if view.altitude_km < STATE_BIG_VISIBLE_MAX_ALTITUDE_KM {
            draw_lines(&boundaries.states_big, BorderKind::StateBig, step);
        }
        // State/province lines appear only from the same altitude as their
        // labels; the app fades them in across a short band below it. The
        // threshold is the app's user-tunable setting.
        if view.altitude_km < view.state_visible_max_km {
            draw_lines(&boundaries.states, BorderKind::State, step);
        }
    }
    if view.layers.cities && municipal_visible {
        let step = if close_zoom { 0.12 } else { 1.0 };
        // State borders traced by municipalities come first; the interior lines
        // (most of the line count) wait until the municipalities are a reasonable
        // size on screen.
        draw_lines(&boundaries.world_cities_major, BorderKind::CityMajor, step);
        // Shared country borders: the country style while country borders are shown,
        // otherwise the state style like any other municipal border on a major line.
        let country_kind = if view.layers.countries {
            BorderKind::CityCountry
        } else {
            BorderKind::CityMajor
        };
        draw_lines(&boundaries.world_cities_country, country_kind, step);
        if view.altitude_km <= boundaries.municipal_detail_max_km {
            draw_lines(&boundaries.cities, BorderKind::City, step);
            draw_lines(&boundaries.world_cities, BorderKind::City, step);
        }
    }
    // Country outlines are drawn last so they stay legible above any
    // subnational layer.
    if view.layers.countries {
        draw_lines(
            &boundaries.countries,
            BorderKind::Country,
            if close_zoom { 0.25 } else { 2.0 },
        );
    }
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
    cands
        .into_iter()
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
        // Municipal state borders keep the state look, interior lines the municipal one.
        assert_eq!(
            dashes.of_kind(BorderKind::CityMajor),
            dash_points(DashStyle::Dotted)
        );
        assert_eq!(dashes.of_kind(BorderKind::City), None);
    }

    #[test]
    fn state_lines_hand_over_only_where_municipal_state_borders_were_loaded() {
        use globecraft_core::camera::Cam;
        use std::sync::Arc;
        // A state line along 48°N from 8°E to 10°E; municipal state borders cover
        // only its western half.
        let mut data = BoundaryData::default();
        data.states = vec![Arc::new(
            (0..=40)
                .map(|i| (48.0, 8.0 + f64::from(i) * 0.05))
                .collect(),
        )];
        data.municipal_major_cells =
            Arc::new(globecraft_data::boundaries::MajorCells::of(&[Arc::new(
                vec![(48.0, 8.0), (48.0, 9.0)],
            )]));
        let basis = Cam {
            lat: 48.0,
            lon: 9.0,
            heading: 0.0,
            alt: 300.0,
        }
        .basis(1.0);
        let layers = OverlayLayers {
            countries: false,
            states: true,
            cities: true,
            labels: false,
        };
        let project = |layers: OverlayLayers| {
            let view = OverlayProjection {
                basis: &basis,
                rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
                altitude_km: 300.0,
                state_visible_max_km: 1609.0,
                dashes: BorderDashes::default(),
                layers,
            };
            let mut state_points = 0.0_f32;
            project_borders(&view, &data, None, |kind, [a, b]| {
                if matches!(kind, BorderKind::State) {
                    state_points += (b - a).length();
                }
            });
            state_points
        };
        // The covered western half is cut out; the eastern half stays.
        let handed_over = project(layers);
        let whole = project(OverlayLayers {
            cities: false,
            ..layers
        });
        assert!(handed_over > 0.0, "the uncovered half stays");
        let kept = handed_over / whole;
        assert!((0.4..0.6).contains(&kept), "{kept} of the line kept");
    }

    #[test]
    fn country_lines_hand_over_to_shared_municipal_country_borders() {
        use globecraft_core::camera::Cam;
        use std::sync::Arc;
        // A Natural Earth country line along 48°N; both neighbours' municipalities are
        // loaded along its western half, which they draw as a country border.
        let mut data = BoundaryData::default();
        data.countries = vec![Arc::new(
            (0..=40)
                .map(|i| (48.0, 8.0 + f64::from(i) * 0.05))
                .collect(),
        )];
        let shared: globecraft_data::boundaries::Line = Arc::new(vec![(48.0, 8.0), (48.0, 9.0)]);
        data.world_cities_country = vec![shared.clone()];
        data.municipal_country_cells = Arc::new(globecraft_data::boundaries::MajorCells::of(
            std::slice::from_ref(&shared),
        ));
        let basis = Cam {
            lat: 48.0,
            lon: 9.0,
            heading: 0.0,
            alt: 300.0,
        }
        .basis(1.0);
        let layers = OverlayLayers {
            countries: true,
            states: true,
            cities: true,
            labels: false,
        };
        let project = |layers: OverlayLayers| {
            let view = OverlayProjection {
                basis: &basis,
                rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
                altitude_km: 300.0,
                state_visible_max_km: 1609.0,
                dashes: BorderDashes::default(),
                layers,
            };
            let (mut coarse, mut municipal, mut as_state) = (0.0_f32, 0.0_f32, 0.0_f32);
            project_borders(&view, &data, None, |kind, [a, b]| match kind {
                BorderKind::Country => coarse += (b - a).length(),
                BorderKind::CityCountry => municipal += (b - a).length(),
                BorderKind::CityMajor => as_state += (b - a).length(),
                _ => {}
            });
            (coarse, municipal, as_state)
        };
        let (whole, _, _) = project(OverlayLayers {
            cities: false,
            ..layers
        });
        let (coarse, municipal, _) = project(layers);
        // The western half is drawn once, by the municipal border; the rest stays.
        let kept = coarse / whole;
        assert!(
            (0.4..0.6).contains(&kept),
            "{kept} of the country line kept"
        );
        assert!(
            (municipal / whole - 0.5).abs() < 0.1,
            "{municipal} vs {whole}"
        );
        // With country borders hidden, the shared border is an ordinary major line.
        let (coarse, municipal, as_state) = project(OverlayLayers {
            countries: false,
            ..layers
        });
        assert_eq!((coarse, municipal), (0.0, 0.0));
        assert!(as_state > 0.0);
    }
}
