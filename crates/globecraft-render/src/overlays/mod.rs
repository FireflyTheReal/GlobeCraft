//! Geographic overlay projection and occupancy, independent of a widget toolkit.
use emath::{Pos2, Rect};
use globecraft_core::{camera::Basis, math::wrap_lon};
use globecraft_data::boundaries::{
    BoundaryData, LabelKind, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM, MapLabel,
    STATE_BIG_VISIBLE_MAX_ALTITUDE_KM,
};
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
const MEDIUM_BORDER_ALTITUDE_KM: f64 = 5000.0;
/// Screen-space dash pattern for state/province borders: on/off in points.
/// Municipal outlines use the same pattern and the app draws them with the
/// same color and width, so the states→municipal handoff keeps one visual.
const STATE_DASH_ON_POINTS: f32 = 7.0;
const STATE_DASH_OFF_POINTS: f32 = 5.0;
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
}
#[derive(Clone, Copy)]
pub enum BorderKind {
    Country,
    /// Provinces of the big countries; drawn earlier than `State` and faded
    /// across its own band, but with the same color, width and dash.
    StateBig,
    State,
    /// Municipal segments on state borders: the accurate version of the state lines,
    /// drawn from the municipal altitude down.
    CityMajor,
    /// Interior municipal lines, drawn only below the data's detail altitude.
    City,
}

/// Dash pattern in points for a border kind. Country borders are solid.
fn dash_pattern(kind: BorderKind) -> Option<(f32, f32)> {
    match kind {
        BorderKind::State | BorderKind::StateBig => {
            Some((STATE_DASH_ON_POINTS, STATE_DASH_OFF_POINTS))
        }
        BorderKind::City | BorderKind::CityMajor => {
            Some((STATE_DASH_ON_POINTS, STATE_DASH_OFF_POINTS))
        }
        BorderKind::Country => None,
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
    let mut emit_segment = |kind: BorderKind, segment: [Pos2; 2]| match dash_pattern(kind) {
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
    let mut draw_lines =
        |lines: &[globecraft_data::boundaries::Line], kind: BorderKind, max_step_deg: f64| {
            for line in lines {
                if visible.is_some_and(|set| !set.contains_line(line)) {
                    continue;
                }
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
        };
    let close_zoom = view.altitude_km <= CLOSE_BORDER_ALTITUDE_KM;
    // Municipal outlines only make sense at close range; data loads below
    // `MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM` and the app fades them in from there
    // to `MUNICIPAL_FADE_FULL_ALTITUDE_KM`.
    let municipal_visible = view.altitude_km <= MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM;
    // State/province lines stay under the municipal outlines. Municipal data now
    // streams per area, so it rarely covers the whole view; hiding the states
    // wherever any municipal line was loaded left the rest of the view without
    // them. Both layers share one visual, so the overlap reads as one linework.
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
    fn solid_borders_are_not_dashed() {
        assert_eq!(dash_pattern(BorderKind::Country), None);
        assert!(dash_pattern(BorderKind::State).is_some());
        assert!(dash_pattern(BorderKind::City).is_some());
    }
}
