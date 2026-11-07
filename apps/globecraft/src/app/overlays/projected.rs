//! Cached administrative border projection and ordered geographic labels.
use super::OverlayStyle;
#[cfg(test)]
use super::OverlayView;
use super::cache::{ARRIVAL_FADE_STEPS, ArrivalFade};
use super::galleys::LabelGalleys;
use super::meshes::{BorderMeshes, MeshSettings, PreparedOverlay};
use super::place_memory::{PlaceNameKey, PlaceNameMemory};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Shape, Stroke, Vec2};
#[cfg(test)]
use globecraft_data::boundaries::BoundaryData;
use globecraft_data::boundaries::{
    LabelKind, MUNICIPAL_DETAIL_FADE_FULL_RATIO, STATE_BIG_VISIBLE_MAX_ALTITUDE_KM,
    STATE_VISIBLE_MAX_ALTITUDE_KM,
};
use globecraft_data::settings::map_style::MapStyle;
use globecraft_render::overlays::{BorderKind, LabelIndex, LabelSpace, MAX_VISIBLE_LABELS};
const LABEL_PADDING_POINTS: f32 = 2.0;
const LABEL_HIGHLIGHT_OFFSET_POINTS: f32 = -0.5;
const GRID_LABEL_FONT_POINTS: f32 = 12.0;
const GRID_LABEL_BACKGROUND_ALPHA: u8 = 180;

#[cfg(test)]
pub(super) fn build(
    painter: &egui::Painter,
    view: OverlayView,
    boundaries: &BoundaryData,
    style: OverlayStyle,
) -> Vec<Shape> {
    let projection = globecraft_render::overlays::OverlayProjection {
        basis: &view.basis,
        rect: view.rect,
        altitude_km: view.altitude_km,
        state_visible_max_km: view.state_max_km,
        layers: style.layers,
        dashes: globecraft_render::overlays::BorderDashes::of(&style.map_style),
        ground: None,
    };
    let mut borders = Vec::new();
    globecraft_render::overlays::project_borders(&projection, boundaries, None, |kind, points| {
        borders.push((kind, points))
    });
    let labels = globecraft_render::overlays::project_labels(&projection, boundaries);
    let policy = FadePolicy {
        altitude_km: view.altitude_km,
        state_max_km: view.state_max_km,
        municipal_detail_max_km: boundaries.municipal_detail_max_km,
        arrival: style.arrival,
    };
    let strokes = border_strokes(policy, &style.map_style);
    let mut shapes: Vec<Shape> = borders
        .iter()
        .map(|&(kind, points)| (points, strokes[kind as usize]))
        .filter(|(_, stroke)| stroke.color.a() > 0)
        .map(|(points, stroke)| Shape::line_segment(points, stroke))
        .collect();
    let grid_labels = view.grid_step_deg.map_or_else(Vec::new, |step| {
        globecraft_render::overlays::grid::project_grid_labels(&view.basis, view.rect, step)
    });
    // Rebuild until every new name is laid out, as successive frames would.
    let mut galleys = LabelGalleys::default();
    let memory = PlaceNameMemory::default();
    let borders = shapes.len();
    loop {
        shapes.truncate(borders);
        galleys.begin(painter.ctx());
        let built = build_labels(
            &mut shapes,
            &mut Vec::new(),
            LabelPass {
                painter,
                style,
                viewport: view.rect,
                policy,
                taken: &LabelSpace::default(),
                memory: &memory,
                only_remembered: false,
            },
            &mut galleys,
            &grid_labels,
            labels.iter().map(|label| (label.position, label.label)),
        );
        if built.complete {
            return shapes;
        }
    }
}

/// Overlay shapes for one view. `labels_pending` means some names still need a layout
/// (bounded per rebuild); the view should be rebuilt next frame to add them.
pub(super) struct BuiltOverlay {
    pub shapes: Vec<Shape>,
    /// The room the names and grid labels take on screen.
    pub label_rects: Vec<Rect>,
    pub labels_pending: bool,
    /// The place names this build drew, for `PlaceNameMemory`.
    pub drawn: Vec<PlaceNameKey>,
}

/// Shapes for prepared geometry: the worker's border meshes tinted with the current
/// fades (re-tessellated here only if the display settings changed since), then labels.
pub(super) fn build_prepared(
    painter: &egui::Painter,
    prepared: &PreparedOverlay,
    fading_out: Option<&PreparedOverlay>,
    galleys: &mut LabelGalleys,
    style: OverlayStyle,
    taken: &LabelSpace,
    memory: &PlaceNameMemory,
) -> BuiltOverlay {
    let geometry = &prepared.geometry;
    let policy = FadePolicy {
        altitude_km: geometry.view.altitude_km,
        state_max_km: geometry.view.state_visible_max_km,
        municipal_detail_max_km: geometry.data.municipal_detail_max_km,
        arrival: style.arrival,
    };
    let settings = MeshSettings::of(painter.ctx());
    let (incoming, outgoing) = exchange_opacities(style.arrival.exchange);
    let mut shapes = Vec::new();
    // The outgoing set under the incoming one: shared lines stay solid throughout.
    if let Some(old) = fading_out {
        let old_policy = FadePolicy {
            municipal_detail_max_km: old.geometry.data.municipal_detail_max_km,
            ..policy
        };
        let retessellated;
        let meshes = if old.borders.settings() == settings {
            &*old.borders
        } else {
            retessellated = BorderMeshes::tessellate(&old.geometry, settings);
            &retessellated
        };
        let mesh = meshes.tinted(
            border_strokes(old_policy, &style.map_style)
                .map(|stroke| with_opacity(stroke.color, outgoing)),
        );
        if !mesh.is_empty() {
            shapes.push(Shape::mesh(mesh));
        }
    }
    let retessellated;
    let meshes = if prepared.borders.settings() == settings {
        &*prepared.borders
    } else {
        retessellated = BorderMeshes::tessellate(geometry, settings);
        &retessellated
    };
    let incoming = if fading_out.is_some() { incoming } else { 1.0 };
    let mesh = meshes.tinted(
        border_strokes(policy, &style.map_style).map(|stroke| with_opacity(stroke.color, incoming)),
    );
    if !mesh.is_empty() {
        shapes.push(Shape::mesh(mesh));
    }
    galleys.begin(painter.ctx());
    let mut label_rects = Vec::new();
    let built = build_labels(
        &mut shapes,
        &mut label_rects,
        LabelPass {
            painter,
            style,
            viewport: geometry.view.rect,
            policy,
            taken,
            memory,
            only_remembered: false,
        },
        galleys,
        &geometry.grid_labels,
        geometry
            .labels
            .iter()
            .map(|label| (label.position, &geometry.data.labels[label.source_index])),
    );
    BuiltOverlay {
        shapes,
        label_rects,
        labels_pending: !built.complete,
        drawn: built.drawn,
    }
}

/// The room the place names already remembered take for this view, before the places
/// of interest are placed. They must be measured here, against the user's markers
/// only, so a lower layer that arrived first cannot keep them hidden. With
/// `include_new`, every place name on this view reserves its room: after a jump the
/// places of interest can be projected first, and place names that were never drawn
/// must be able to claim their room from names that arrived only moments before.
pub(super) fn remembered_rects(
    painter: &egui::Painter,
    prepared: &PreparedOverlay,
    galleys: &mut LabelGalleys,
    style: OverlayStyle,
    taken: &LabelSpace,
    memory: &PlaceNameMemory,
    include_new: bool,
) -> Vec<Rect> {
    let geometry = &prepared.geometry;
    let policy = FadePolicy {
        altitude_km: geometry.view.altitude_km,
        state_max_km: geometry.view.state_visible_max_km,
        municipal_detail_max_km: geometry.data.municipal_detail_max_km,
        arrival: style.arrival,
    };
    let mut shapes = Vec::new();
    let mut rects = Vec::new();
    galleys.begin(painter.ctx());
    let _ = build_labels(
        &mut shapes,
        &mut rects,
        LabelPass {
            painter,
            style,
            viewport: geometry.view.rect,
            policy,
            taken,
            memory,
            only_remembered: !include_new,
        },
        galleys,
        &[],
        geometry
            .labels
            .iter()
            .map(|label| (label.position, &geometry.data.labels[label.source_index])),
    );
    rects
}

/// State lines fade in from the user's state altitude down to this fraction of it, then stay
/// fully visible (1,609 → 805 km, about 1,000 → 500 mi, with the default setting). A ratio
/// keeps the band valid for any setting; a fixed floor inverted the fade below it.
const STATE_FADE_FULL_RATIO: f64 = 0.5;

/// Camera altitude and the user-tunable state-line threshold, carried together
/// through the shape-building path that fades layers by altitude.
#[derive(Clone, Copy)]
struct FadePolicy {
    altitude_km: f64,
    state_max_km: f64,
    /// `BoundaryData::municipal_detail_max_km`: where interior municipal lines start.
    municipal_detail_max_km: f64,
    arrival: ArrivalFade,
}

/// Altitude fade times the fade-in of data that arrived recently.
fn faded_opacity(kind: BorderKind, policy: FadePolicy) -> f32 {
    let arrived = match kind {
        BorderKind::State => policy.arrival.states,
        BorderKind::StateBig => policy.arrival.states_big,
        BorderKind::City => policy.arrival.municipal,
        BorderKind::Country | BorderKind::Disputed => ARRIVAL_FADE_STEPS,
    };
    let arrival = smoothstep(f64::from(arrived) / f64::from(ARRIVAL_FADE_STEPS));
    layer_opacity(kind, policy) * arrival
}

/// Opacities of the incoming and outgoing border sets at exchange step `step`: the
/// new set fades in over the first half, then the old one fades out.
fn exchange_opacities(step: u8) -> (f32, f32) {
    let t = f64::from(step) / f64::from(ARRIVAL_FADE_STEPS);
    (smoothstep(2.0 * t), 1.0 - smoothstep(2.0 * t - 1.0))
}

fn smoothstep(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

fn layer_opacity(kind: BorderKind, policy: FadePolicy) -> f32 {
    let FadePolicy {
        altitude_km,
        state_max_km,
        municipal_detail_max_km,
        ..
    } = policy;
    match kind {
        // Big-country provinces arrive first and are already fully opaque by
        // the altitude the other countries' provinces start fading in.
        BorderKind::StateBig => smoothstep(
            (STATE_BIG_VISIBLE_MAX_ALTITUDE_KM - altitude_km)
                / (STATE_BIG_VISIBLE_MAX_ALTITUDE_KM - STATE_VISIBLE_MAX_ALTITUDE_KM),
        ),
        BorderKind::State if state_max_km <= 0.0 => 0.0,
        BorderKind::State => smoothstep(
            (state_max_km - altitude_km) / (state_max_km * (1.0 - STATE_FADE_FULL_RATIO)),
        ),
        BorderKind::City if municipal_detail_max_km <= 0.0 => 0.0,
        BorderKind::City => smoothstep(
            (municipal_detail_max_km - altitude_km)
                / (municipal_detail_max_km * (1.0 - MUNICIPAL_DETAIL_FADE_FULL_RATIO)),
        ),
        BorderKind::Country | BorderKind::Disputed => 1.0,
    }
}

/// Every border kind, in `BorderKind` declaration order (so `kind as usize` indexes it).
pub(super) const BORDER_KINDS: [BorderKind; 5] = [
    BorderKind::Country,
    BorderKind::StateBig,
    BorderKind::State,
    BorderKind::City,
    BorderKind::Disputed,
];

/// The user's colour for a border kind (see `BorderKind::line_type`).
fn border_color(kind: BorderKind, style: &MapStyle) -> Color32 {
    let [r, g, b, a] = style.line(kind.line_type()).color.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub(super) fn border_width(kind: BorderKind, style: &MapStyle) -> f32 {
    style.line(kind.line_type()).width_points
}

/// The stroke of each border kind for one view, indexed by `kind as usize`.
fn border_strokes(policy: FadePolicy, style: &MapStyle) -> [Stroke; BORDER_KINDS.len()] {
    BORDER_KINDS.map(|kind| {
        Stroke::new(
            border_width(kind, style),
            with_opacity(border_color(kind, style), faded_opacity(kind, policy)),
        )
    })
}

fn with_opacity(color: Color32, opacity: f32) -> Color32 {
    if opacity >= 1.0 {
        return color;
    }
    let [r, g, b, a] = color.to_array();
    let alpha = (a as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// What one label pass draws for.
struct LabelPass<'p> {
    painter: &'p egui::Painter,
    style: OverlayStyle,
    viewport: Rect,
    policy: FadePolicy,
    /// Room taken before these names (the markers of the user's data, and the
    /// places of interest already on screen for the full pass).
    taken: &'p LabelSpace,
    memory: &'p PlaceNameMemory,
    /// Lay out only the place names already remembered, to reserve their room
    /// before a lower layer places its names.
    only_remembered: bool,
}

struct BuiltLabels {
    complete: bool,
    drawn: Vec<PlaceNameKey>,
}

/// Appends label shapes in priority order, and the room each takes to `rects`.
/// `complete` is false when the layout budget ran out: the remaining
/// (lower-priority) names are left for the next rebuild.
fn build_labels<'a>(
    shapes: &mut Vec<Shape>,
    rects: &mut Vec<Rect>,
    LabelPass {
        painter,
        style,
        viewport,
        policy,
        taken,
        memory,
        only_remembered,
    }: LabelPass<'_>,
    galleys: &mut LabelGalleys,
    grid_labels: &[globecraft_render::overlays::grid::GridLabel],
    cands: impl IntoIterator<Item = (egui::Pos2, &'a globecraft_data::boundaries::MapLabel)>,
) -> BuiltLabels {
    let mut complete = true;
    let mut drawn = Vec::new();
    let ms = style.map_scale;
    let mut placed = LabelIndex::default();
    if style.layers.labels {
        // The family lookup is the same for every label of the view.
        let map_family = crate::app::fonts::map_font(painter.ctx(), 1.0).family;
        // Countries, then states, then cities; within each kind the names already
        // drawn come before the new ones, and their projection order (rank) holds.
        let mut cands: Vec<_> = cands
            .into_iter()
            .map(|(pos, label)| {
                let key = PlaceNameKey::of(label);
                let tier = memory.get(&key).map_or(u8::MAX, |entry| memory.tier(entry));
                (pos, label, key, kind_priority(label.kind), tier)
            })
            .filter(|(_, _, _, _, tier)| !only_remembered || *tier != u8::MAX)
            .collect();
        // Stable, so the projection's rank order holds within a kind and tier.
        cands.sort_by_key(|(_, _, _, kind, tier)| (*kind, *tier));
        // Collect what is visible, then place by priority (countries > states > cities)
        // skipping any name that would overlap one already drawn.
        for (pos, label, key, _, _) in cands {
            if placed.len() >= MAX_VISIBLE_LABELS {
                break;
            }
            let sizes = style.map_style.labels;
            let (color, size) = match label.kind {
                LabelKind::Country => (Color32::WHITE, sizes.country_points),
                LabelKind::State => (
                    with_opacity(
                        Color32::from_rgb(180, 230, 255),
                        faded_opacity(
                            if label.big {
                                BorderKind::StateBig
                            } else {
                                BorderKind::State
                            },
                            policy,
                        ),
                    ),
                    sizes.state_points,
                ),
                LabelKind::City => (Color32::from_rgb(255, 245, 210), sizes.city_points),
            };
            if color.a() == 0 {
                continue;
            }
            let font = FontId::new(size * ms, map_family.clone());
            let text = label.text(style.lang);
            // One white layout per name, kept across rebuilds; shadow and text recolour it.
            let Some(galley) = galleys.get(painter, text, &font) else {
                complete = false;
                break;
            };
            let area = Rect::from_center_size(pos, galley.size()).expand(LABEL_PADDING_POINTS);
            if !taken.is_free(area) || !placed.try_insert(area) {
                continue;
            }
            rects.push(area);
            drawn.push(key);
            if galley.is_empty() {
                continue;
            }
            let top_left = Align2::CENTER_CENTER.anchor_size(pos, galley.size()).min;
            shapes.push(Shape::galley_with_override_text_color(
                top_left,
                galley.clone(),
                Color32::from_black_alpha(180),
            ));
            shapes.push(Shape::galley_with_override_text_color(
                top_left + Vec2::new(0.0, LABEL_HIGHLIGHT_OFFSET_POINTS),
                galley,
                color,
            ));
        }
    }
    if !only_remembered {
        for label in grid_labels {
            use globecraft_render::overlays::grid::GridAxis;
            let direction = match (label.axis, label.degrees >= 0.0) {
                (GridAxis::Latitude, true) => "compass-north",
                (GridAxis::Latitude, false) => "compass-south",
                (GridAxis::Longitude, true) => "compass-east",
                (GridAxis::Longitude, false) => "compass-west",
            };
            let value = format!("{:.3}", label.degrees.abs());
            let text = if label.degrees == 0.0 {
                "0°".into()
            } else {
                style.lang.format(
                    "grid-coordinate-label",
                    &[
                        (
                            "degrees",
                            value.trim_end_matches('0').trim_end_matches('.').into(),
                        ),
                        ("direction", style.lang.text(direction).into()),
                    ],
                )
            };
            let galley = painter.layout_no_wrap(
                text,
                FontId::monospace(GRID_LABEL_FONT_POINTS * ms),
                Color32::WHITE,
            );
            let half_size = galley.size() * 0.5 + Vec2::splat(LABEL_PADDING_POINTS);
            if half_size.x * 2.0 > viewport.width() || half_size.y * 2.0 > viewport.height() {
                continue;
            }
            let anchor = match label.edge {
                globecraft_render::overlays::grid::GridLabelEdge::Left => {
                    label.position + Vec2::new(half_size.x, 0.0)
                }
                globecraft_render::overlays::grid::GridLabelEdge::Bottom => {
                    label.position - Vec2::new(0.0, half_size.y)
                }
            };
            let position = egui::pos2(
                anchor.x.clamp(
                    viewport.left() + half_size.x,
                    viewport.right() - half_size.x,
                ),
                anchor.y.clamp(
                    viewport.top() + half_size.y,
                    viewport.bottom() - half_size.y,
                ),
            );
            let area = Rect::from_center_size(position, galley.size()).expand(LABEL_PADDING_POINTS);
            if !viewport.contains_rect(area) || !taken.is_free(area) || !placed.try_insert(area) {
                continue;
            }
            rects.push(area);
            shapes.push(Shape::rect_filled(
                area,
                LABEL_PADDING_POINTS,
                Color32::from_black_alpha(GRID_LABEL_BACKGROUND_ALPHA),
            ));
            shapes.push(Shape::galley(
                position - galley.size() * 0.5,
                galley,
                Color32::WHITE,
            ));
        }
    }
    BuiltLabels { complete, drawn }
}

/// Country names first, then states, then cities (the projection's priority).
fn kind_priority(kind: LabelKind) -> u8 {
    match kind {
        LabelKind::Country => 0,
        LabelKind::State => 1,
        LabelKind::City => 2,
    }
}

#[cfg(test)]
mod fade_tests {
    use super::*;

    #[test]
    fn a_border_exchange_fades_the_new_set_in_before_the_old_one_out() {
        assert_eq!(exchange_opacities(0), (0.0, 1.0));
        assert_eq!(exchange_opacities(ARRIVAL_FADE_STEPS / 2), (1.0, 1.0));
        assert_eq!(exchange_opacities(ARRIVAL_FADE_STEPS), (1.0, 0.0));
        // Never both faint at once: a line both sets share stays solid.
        for step in 0..=ARRIVAL_FADE_STEPS {
            let (incoming, outgoing) = exchange_opacities(step);
            assert!(incoming.max(outgoing) >= 1.0 - f32::EPSILON, "step {step}");
        }
    }

    #[test]
    fn border_kinds_index_their_own_stroke() {
        for (index, kind) in BORDER_KINDS.into_iter().enumerate() {
            assert_eq!(kind as usize, index);
        }
    }

    fn policy(altitude_km: f64, state_max_km: f64, municipal_detail_max_km: f64) -> FadePolicy {
        FadePolicy {
            altitude_km,
            state_max_km,
            municipal_detail_max_km,
            arrival: ArrivalFade::SETTLED,
        }
    }

    #[test]
    fn state_fade_is_monotonic_for_any_user_threshold() {
        for state_max_km in [0.0, 100.0, 805.0, 1609.0, 8000.0] {
            let mut previous = 0.0;
            // Descending: lines may only become more opaque as the camera lowers.
            for step in (0..=100).rev() {
                let altitude_km = 9000.0 * f64::from(step) / 100.0;
                let opacity =
                    layer_opacity(BorderKind::State, policy(altitude_km, state_max_km, 0.0));
                assert!((0.0..=1.0).contains(&opacity), "{state_max_km}: {opacity}");
                assert!(opacity >= previous, "{state_max_km} km at {altitude_km} km");
                previous = opacity;
            }
            assert_eq!(
                layer_opacity(
                    BorderKind::State,
                    policy(state_max_km + 1.0, state_max_km, 0.0)
                ),
                0.0
            );
        }
    }

    #[test]
    fn municipal_lines_fade_in_below_their_detail_altitude() {
        let detail_km = 75.0;
        let at = |kind, altitude_km| layer_opacity(kind, policy(altitude_km, 1609.0, detail_km));
        assert_eq!(at(BorderKind::City, 300.0), 0.0);
        assert_eq!(at(BorderKind::City, detail_km), 0.0);
        assert!(at(BorderKind::City, detail_km * 0.9) > 0.0);
        assert_eq!(
            at(
                BorderKind::City,
                detail_km * MUNICIPAL_DETAIL_FADE_FULL_RATIO
            ),
            1.0
        );
        // And they fade in when the tiles first bring them.
        let just_arrived = ArrivalFade {
            municipal: 0,
            ..ArrivalFade::SETTLED
        };
        assert_eq!(
            faded_opacity(
                BorderKind::City,
                FadePolicy {
                    arrival: just_arrived,
                    ..policy(30.0, 1609.0, 75.0)
                }
            ),
            0.0
        );
    }
}
