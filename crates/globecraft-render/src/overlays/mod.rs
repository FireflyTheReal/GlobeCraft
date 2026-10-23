//! Geographic overlay projection and occupancy, independent of a widget toolkit.
use emath::{Pos2, Rect};
use globecraft_core::{camera::Basis, math::wrap_lon};
use globecraft_data::boundaries::{
    BoundaryData, LabelKind, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM, MapLabel,
    STATE_BIG_VISIBLE_MAX_ALTITUDE_KM,
};
use globecraft_data::settings::map_style::{DashStyle, LineType, MapStyle};
use std::collections::HashMap;
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
    /// Municipal segments on state borders: the accurate version of the state lines,
    /// drawn from the municipal altitude down in place of the stretches they cover.
    CityMajor,
    /// Interior municipal lines, drawn only below the data's detail altitude.
    City,
    /// Municipal country borders, drawn by one side per border, in the country style in
    /// place of the Natural Earth country line they cover.
    CityCountry,
    /// Disputed borders and lines of control, in their own style. They are never
    /// replaced by municipal lines; municipal country borders give way to them instead.
    Disputed,
}

impl BorderKind {
    /// The user-styled line type this kind draws with. Municipal state and country
    /// borders replace the coarse lines up close, so they take those styles.
    pub fn line_type(self) -> LineType {
        match self {
            Self::Country | Self::CityCountry => LineType::Country,
            Self::StateBig | Self::State | Self::CityMajor => LineType::State,
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
    // Municipal outlines only make sense at close range: they are drawn from
    // `MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM` down.
    let municipal_visible = view.altitude_km <= MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM;
    // Where municipal state borders were loaded, the coarse state lines hand over to
    // them: the covered parts are cut out (see `handover`). Elsewhere the coarse lines
    // stay, since municipal data rarely covers the whole view.
    // The sharper state borders belong to the state layer, not to the municipal one.
    let state_cells = Some(&*boundaries.municipal_major_cells)
        .filter(|cells| municipal_visible && view.layers.states && !cells.is_empty());
    // Likewise for country lines, with the country layer. With the country layer off
    // and the state layer on, country borders close the states (drawn in the state
    // style), and the same handover applies to them.
    let states_close_countries = view.layers.states && !view.layers.countries;
    let country_cells = Some(&*boundaries.municipal_country_cells).filter(|cells| {
        municipal_visible && (view.layers.countries || states_close_countries) && !cells.is_empty()
    });
    let cells_for = |kind: BorderKind| match kind {
        BorderKind::State | BorderKind::StateBig => state_cells,
        _ => None,
    };
    let disputed = |index: usize| boundaries.country_disputed.get(index) == Some(&true);
    // Municipal country borders give way to disputed lines, which stay whole.
    let disputed_cells = Some(&*boundaries.disputed_cells).filter(|cells| !cells.is_empty());
    // A Natural Earth country line is cut by the municipal version of its own border.
    let country_line_cells = |index: usize| {
        country_cells
            .map(|cells| cells.for_pair(boundaries.country_pairs.get(index).copied().flatten()))
    };
    // `cells`: where municipal borders replace these lines (see `handover`).
    let mut draw_cut = |lines: &[globecraft_data::boundaries::Line],
                        kind: BorderKind,
                        cells: Option<&globecraft_data::boundaries::MajorCells>,
                        max_step_deg: f64| {
        for line in lines {
            if visible.is_some_and(|set| !set.contains_line(line)) {
                continue;
            }
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
            draw_cut(
                &boundaries.states_big,
                BorderKind::StateBig,
                cells_for(BorderKind::StateBig),
                step,
            );
        }
        // State/province lines appear only from the same altitude as their
        // labels; the app fades them in across a short band below it. The
        // threshold is the app's user-tunable setting.
        if view.altitude_km < view.state_visible_max_km {
            draw_cut(
                &boundaries.states,
                BorderKind::State,
                cells_for(BorderKind::State),
                step,
            );
        }
        // Without the country layer, the country borders close the states, from the
        // altitude the big countries' provinces appear.
        if states_close_countries
            && view.altitude_km < STATE_BIG_VISIBLE_MAX_ALTITUDE_KM.max(view.state_visible_max_km)
        {
            // Cut like country lines: the municipal country borders replace them.
            for (index, line) in boundaries.countries.iter().enumerate() {
                let cells = (!disputed(index))
                    .then(|| country_line_cells(index))
                    .flatten();
                draw_cut(
                    std::slice::from_ref(line),
                    BorderKind::StateBig,
                    cells,
                    step,
                );
            }
        }
    }
    if municipal_visible {
        let step = if close_zoom { 0.12 } else { 1.0 };
        // State borders traced by municipalities come first; the interior lines
        // (most of the line count) wait until the municipalities are a reasonable
        // size on screen.
        if view.layers.states {
            draw_cut(
                &boundaries.world_cities_major,
                BorderKind::CityMajor,
                None,
                step,
            );
            // Without the country layer, municipal country borders close the states.
            if !view.layers.countries {
                draw_cut(
                    &boundaries.world_cities_country,
                    BorderKind::CityMajor,
                    disputed_cells,
                    step,
                );
            }
        }
        if view.layers.cities && view.altitude_km <= boundaries.municipal_detail_max_km {
            draw_cut(
                &boundaries.cities,
                BorderKind::City,
                cells_for(BorderKind::City),
                step,
            );
            draw_cut(
                &boundaries.world_cities,
                BorderKind::City,
                cells_for(BorderKind::City),
                step,
            );
            // With the state or country layer off, the municipal segments on those
            // borders are ordinary municipal lines; without them the municipalities
            // would have gaps.
            if !view.layers.states {
                draw_cut(
                    &boundaries.world_cities_major,
                    BorderKind::City,
                    cells_for(BorderKind::City),
                    step,
                );
            }
            if !view.layers.countries && !view.layers.states {
                draw_cut(
                    &boundaries.world_cities_country,
                    BorderKind::City,
                    None,
                    step,
                );
            }
        }
        if view.layers.countries {
            draw_cut(
                &boundaries.world_cities_country,
                BorderKind::CityCountry,
                disputed_cells,
                step,
            );
        }
    }
    // Country outlines are drawn last so they stay legible above any
    // subnational layer.
    if view.layers.countries {
        let step = if close_zoom { 0.25 } else { 2.0 };
        for (index, line) in boundaries.countries.iter().enumerate() {
            if disputed(index) {
                draw_cut(std::slice::from_ref(line), BorderKind::Disputed, None, step);
            } else {
                draw_cut(
                    std::slice::from_ref(line),
                    BorderKind::Country,
                    country_line_cells(index),
                    step,
                );
            }
        }
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
            // Municipal names belong to the municipal layer; its data also loads for
            // the sharper state and country borders.
            LabelKind::City => {
                (view.layers.cities || !label.is_municipal())
                    && label
                        .visible_with_detail(view.altitude_km, boundaries.municipal_detail_max_km)
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
    // An OpenStreetMap place name is skipped where a label already shown carries the
    // same name nearby (the town from Natural Earth or the municipality), or an earlier
    // copy of the place (an island tagged as both a point and an outline).
    let mut shown: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    cands
        .into_iter()
        .filter(|(_, _, _, label, _)| {
            if label.kind != LabelKind::City {
                return true;
            }
            let near = shown.entry(label.en.to_lowercase()).or_default();
            if label.is_place()
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
        let project = |layers: OverlayLayers, data: &BoundaryData| {
            let view = OverlayProjection {
                basis: &basis,
                rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
                altitude_km: 300.0,
                state_visible_max_km: 1609.0,
                dashes: BorderDashes::default(),
                layers,
            };
            let mut state_points = 0.0_f32;
            project_borders(&view, data, None, |kind, [a, b]| {
                if matches!(kind, BorderKind::State) {
                    state_points += (b - a).length();
                }
            });
            state_points
        };
        // The covered western half is cut out; the eastern half stays. The upgrade
        // belongs to the state layer: it happens with the municipal layer off too.
        let whole = project(layers, &{
            let mut plain = data.clone();
            plain.municipal_major_cells = Arc::default();
            plain
        });
        let handed_over = project(layers, &data);
        assert_eq!(
            handed_over,
            project(
                OverlayLayers {
                    cities: false,
                    ..layers
                },
                &data
            )
        );
        assert!(handed_over > 0.0, "the uncovered half stays");
        let kept = handed_over / whole;
        assert!((0.4..0.6).contains(&kept), "{kept} of the line kept");
    }

    #[test]
    fn disputed_lines_stay_whole_and_municipal_borders_give_way_to_them() {
        use globecraft_core::camera::Cam;
        use std::sync::Arc;
        let mut data = BoundaryData::default();
        // A disputed line along 48° N and an ordinary one along 48.5° N; a municipal
        // country border runs 2 km from each.
        let along = |lat: f64| -> globecraft_data::boundaries::Line {
            Arc::new((0..=10).map(|i| (lat, 8.5 + i as f64 * 0.1)).collect())
        };
        data.countries = vec![along(48.0), along(48.5)];
        data.country_disputed = vec![true, false];
        data.country_pairs = vec![Some([*b"CHN", *b"IND"]), Some([*b"DEU", *b"CZE"])];
        data.world_cities_country = vec![along(48.02), along(48.52)];
        data.municipal_country_cells = Arc::new(globecraft_data::boundaries::CountryCells::of(
            &data.world_cities_country,
            &[Some([*b"CHN", *b"IND"]), Some([*b"CZE", *b"DEU"])],
        ));
        data.disputed_cells = Arc::new(globecraft_data::boundaries::MajorCells::with_cover(
            &data.countries[..1],
            0.1,
        ));
        let basis = Cam {
            lat: 48.25,
            lon: 9.0,
            heading: 0.0,
            alt: 300.0,
        }
        .basis(1.0);
        let view = OverlayProjection {
            basis: &basis,
            rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
            altitude_km: 300.0,
            state_visible_max_km: 1609.0,
            dashes: BorderDashes::default(),
            layers: OverlayLayers {
                countries: true,
                states: false,
                cities: false,
                labels: false,
            },
        };
        let mut lengths = std::collections::BTreeMap::<&str, f32>::new();
        project_borders(&view, &data, None, |kind, [a, b]| {
            let name = match kind {
                BorderKind::Disputed => "disputed",
                BorderKind::Country => "country",
                BorderKind::CityCountry => "municipal country",
                _ => "other",
            };
            *lengths.entry(name).or_default() += (b - a).length();
        });
        let whole = lengths["disputed"];
        // The disputed line is drawn whole; the ordinary one is replaced by its municipal
        // version, and the municipal version of the disputed border is not drawn.
        assert!(whole > 100.0, "{lengths:?}");
        assert!(!lengths.contains_key("country"), "{lengths:?}");
        assert!(
            (lengths["municipal country"] - whole).abs() < whole * 0.05,
            "only one municipal line remains: {lengths:?}"
        );
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
        };
        let mut data = BoundaryData::default();
        data.labels = vec![
            // Natural Earth's town, then OpenStreetMap's town and a village (ranks 11, 12).
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
            },
        };
        let names: Vec<(&str, u8)> = project_labels(&view, &data)
            .iter()
            .map(|projected| (projected.label.en.as_str(), projected.label.rank))
            .collect();
        assert_eq!(names, [("Fergana", 6), ("Qishloq", 12)]);
    }

    #[test]
    fn country_borders_close_the_states_when_only_states_are_on() {
        use globecraft_core::camera::Cam;
        use std::sync::Arc;
        let mut data = BoundaryData::default();
        // Natural Earth's country line along 48° N; the municipal country border
        // replaces its western half.
        data.countries = vec![Arc::new(
            (0..=20).map(|i| (48.0, 8.0 + i as f64 * 0.1)).collect(),
        )];
        data.world_cities_country = vec![Arc::new(vec![(48.0, 8.0), (48.0, 9.0)])];
        data.municipal_country_cells = Arc::new(globecraft_data::boundaries::CountryCells::of(
            &data.world_cities_country,
            &[],
        ));
        let basis = Cam {
            lat: 48.0,
            lon: 9.0,
            heading: 0.0,
            alt: 300.0,
        }
        .basis(1.0);
        let draw = |countries: bool, states: bool, cities: bool| {
            let view = OverlayProjection {
                basis: &basis,
                rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
                altitude_km: 300.0,
                state_visible_max_km: 1609.0,
                dashes: BorderDashes::default(),
                layers: OverlayLayers {
                    countries,
                    states,
                    cities,
                    labels: false,
                },
            };
            let mut kinds = std::collections::BTreeMap::<&str, f32>::new();
            project_borders(&view, &data, None, |kind, [a, b]| {
                let name = match kind {
                    BorderKind::Country => "Country",
                    BorderKind::State => "State",
                    BorderKind::StateBig => "StateBig",
                    BorderKind::City => "City",
                    BorderKind::CityMajor => "CityMajor",
                    BorderKind::CityCountry => "CityCountry",
                    BorderKind::Disputed => "Disputed",
                };
                *kinds.entry(name).or_default() += (b - a).length();
            });
            kinds
        };
        let states_only = draw(false, true, false);
        assert!(states_only.contains_key("CityMajor"), "{states_only:?}");
        assert!(!states_only.contains_key("Country") && !states_only.contains_key("City"));
        // The coarse line is drawn in the state style, minus the half the municipal
        // border replaces, exactly as in the country style (the two styles dash
        // differently, so each is compared with its own municipal half).
        let both = draw(true, true, false);
        let ratio = |coarse: f32, fine: f32| coarse / fine;
        assert!(
            (ratio(states_only["StateBig"], states_only["CityMajor"]) - 1.0).abs() < 0.05
                && (ratio(both["Country"], both["CityCountry"]) - 1.0).abs() < 0.05,
            "{states_only:?} {both:?}"
        );
        assert!(
            !both.contains_key("CityMajor"),
            "drawn once, as country: {both:?}"
        );
        // With states on, the municipal layer does not draw the same segments again.
        let with_cities = draw(false, true, true);
        assert!(!with_cities.contains_key("City"), "{with_cities:?}");
    }

    #[test]
    fn municipal_state_borders_close_the_municipalities_when_states_are_off() {
        use globecraft_core::camera::Cam;
        use std::sync::Arc;
        let mut data = BoundaryData::default();
        data.world_cities_major = vec![Arc::new(vec![(48.0, 8.5), (48.0, 9.5)])];
        data.world_cities = vec![Arc::new(vec![(48.2, 8.5), (48.2, 9.5)])];
        data.municipal_detail_max_km = 400.0;
        let basis = Cam {
            lat: 48.0,
            lon: 9.0,
            heading: 0.0,
            alt: 300.0,
        }
        .basis(1.0);
        let kinds = |states: bool, cities: bool| {
            let view = OverlayProjection {
                basis: &basis,
                rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 800.0)),
                altitude_km: 300.0,
                state_visible_max_km: 1609.0,
                dashes: BorderDashes::default(),
                layers: OverlayLayers {
                    countries: true,
                    states,
                    cities,
                    labels: false,
                },
            };
            let (mut major, mut city) = (0, 0);
            project_borders(&view, &data, None, |kind, _| match kind {
                BorderKind::CityMajor => major += 1,
                BorderKind::City => city += 1,
                _ => {}
            });
            (major, city)
        };
        let (major, city_with_states) = kinds(true, true);
        assert!(major > 0);
        // States off: the state-border segments join the municipal lines.
        let (major_off, city_without_states) = kinds(false, true);
        assert_eq!(major_off, 0);
        assert!(city_without_states > city_with_states);
        // Municipalities off too: nothing municipal at all.
        assert_eq!(kinds(false, false), (0, 0));
    }
}
