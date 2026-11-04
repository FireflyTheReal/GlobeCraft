//! The points of a feature layer (roadmap 4.I): markers sized by the layer's value
//! (magnitude, population…), points that would cover each other gathered into a
//! cluster badge with their count, names in order of value, and what the pointer
//! finds for the hover card.
use super::{FeatureTime, LABEL_PADDING_POINTS, MARKER_POINTS, NamedPoint};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};
use globecraft_core::camera::Basis;
use globecraft_core::language::Lang;
use globecraft_data::user_data::{FeatureSet, UserLayer, format_time_utc};
use globecraft_render::overlays::clusters::{ClusterPoint, cluster};

/// Markers sized by value run from this radius to the next (map points).
const MIN_SIZED_POINTS: f32 = 3.0;
const MAX_SIZED_POINTS: f32 = 11.0;
/// Values spanning more than this factor are sized by area (square root), as
/// counts and populations are; narrower ones (magnitudes) linearly.
const AREA_SCALE_SPAN: f64 = 50.0;
/// Points whose centres are closer than this (map points) become one cluster. Twice
/// the largest badge, so two badges never cover each other.
const CLUSTER_RADIUS_POINTS: f32 = 2.0 * CLUSTER_MAX_POINTS;
/// A cluster badge's radius grows with the count from the first to the second.
const CLUSTER_MIN_POINTS: f32 = 8.0;
const CLUSTER_MAX_POINTS: f32 = 13.0;
/// The count at which a badge reaches its largest size.
const CLUSTER_FULL_COUNT: f64 = 1000.0;
const CLUSTER_TEXT_POINTS: f32 = 10.0;
/// The hover finds a marker within its radius plus this (map points).
const HOVER_SLACK_POINTS: f32 = 3.0;
/// Names listed on a cluster's hover card.
const CLUSTER_HOVER_NAMES: usize = 3;
/// Timed points younger than this (before the app's time) pulse, the youngest
/// strongest: the day's earthquakes stand out from the week's.
const PULSE_RECENT_SECS: f64 = 24.0 * 3600.0;
/// At most this many pulse at once, the largest values first.
const MAX_PULSES: usize = 40;
/// One ring's run, seconds; two rings half a run apart.
const PULSE_PERIOD_SECS: f64 = 2.4;
/// A ring grows from the marker to this many times its radius, so larger values
/// (magnitudes) send out larger rings.
const PULSE_REACH: f32 = 3.5;
/// Redraws while pulses run, about 30 a second.
const PULSE_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

/// A marker or badge that pulses: where, its radius, colour and how strongly (1 for
/// a point at the app's moment, falling to 0 a day before).
#[derive(Clone, Copy, Debug)]
pub(super) struct Pulse {
    pub at: Pos2,
    pub radius: f32,
    pub color: Color32,
    pub strength: f32,
    /// Spreads the rings' phases, so neighbours do not beat together.
    pub phase: f32,
}

/// A marker or badge the pointer can find: where, how big, and which points (the
/// most important first).
pub(super) struct Hit {
    pub at: Pos2,
    pub radius: f32,
    pub members: Vec<usize>,
}

/// A layer's points for one view.
pub(super) struct LayerPoints {
    /// Markers and badges, to tessellate.
    pub markers: Vec<Shape>,
    /// Badge counts: where, the text, and the opacity (laid out when drawn).
    pub counts: Vec<(Pos2, String, f32)>,
    /// The room every marker and badge takes.
    pub rooms: Vec<Rect>,
    /// Points with names, most important first.
    pub named: Vec<NamedPoint>,
    pub hits: Vec<Hit>,
    pub pulses: Vec<Pulse>,
}

/// The radius (before the map scale) of a point's marker.
fn marker_radius(value: Option<f64>, range: Option<(f64, f64)>) -> f32 {
    let (Some(value), Some((low, high))) = (value, range) else {
        return MARKER_POINTS;
    };
    if high <= low {
        return (MIN_SIZED_POINTS + MAX_SIZED_POINTS) / 2.0;
    }
    let mut t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    if low >= 0.0 && high > AREA_SCALE_SPAN * low.max(1.0) {
        t = t.sqrt();
    }
    MIN_SIZED_POINTS + (MAX_SIZED_POINTS - MIN_SIZED_POINTS) * t as f32
}

fn badge_radius(count: usize) -> f32 {
    let t = ((count as f64).log10() / CLUSTER_FULL_COUNT.log10()).clamp(0.0, 1.0) as f32;
    CLUSTER_MIN_POINTS + (CLUSTER_MAX_POINTS - CLUSTER_MIN_POINTS) * t
}

fn count_text(count: usize) -> String {
    if count < 1000 {
        count.to_string()
    } else {
        format!("{}k", count / 1000)
    }
}

pub(super) fn layer_points(
    basis: &Basis,
    rect: Rect,
    features: &FeatureSet,
    layer: &UserLayer,
    scale: f32,
    time: &FeatureTime,
) -> LayerPoints {
    let [r, g, b] = layer.rgb();
    let color = Color32::from_rgb(r, g, b).gamma_multiply(layer.opacity);
    let outline = Color32::from_rgb(20, 24, 30).gamma_multiply(layer.opacity);
    let rim = Color32::WHITE.gamma_multiply(layer.opacity);
    let padding = LABEL_PADDING_POINTS * scale;
    // The points on screen, and where.
    let mut on_screen: Vec<(usize, Pos2)> = Vec::new();
    for (index, point) in features.points.iter().enumerate() {
        // Timed points show inside the app time's window, up to its moment.
        if let Some(ms) = point.time_ms
            && !time.time.shows(ms as f64 / 1000.0, time.wall)
        {
            continue;
        }
        let Some(at) = basis.project(
            globecraft_render::overlays::ground_position(basis, None, point.lat, point.lon),
            rect,
        ) else {
            continue;
        };
        if rect.contains(at) {
            on_screen.push((index, at));
        }
    }
    let inputs: Vec<ClusterPoint> = on_screen
        .iter()
        .map(|&(index, position)| ClusterPoint {
            position,
            weight: features.points[index].value.unwrap_or(0.0),
        })
        .collect();
    let mut out = LayerPoints {
        markers: Vec::new(),
        counts: Vec::new(),
        rooms: Vec::new(),
        named: Vec::new(),
        hits: Vec::new(),
        pulses: Vec::new(),
    };
    let scene = time.time.now(time.wall);
    let recency = |index: usize| {
        let ms = features.points[index].time_ms?;
        let age = scene - ms as f64 / 1000.0;
        (0.0..PULSE_RECENT_SECS)
            .contains(&age)
            .then(|| 1.0 - (age / PULSE_RECENT_SECS) as f32)
    };
    let mut pulses: Vec<(f64, Pulse)> = Vec::new();
    for group in cluster(&inputs, CLUSTER_RADIUS_POINTS * scale) {
        let (leader, at) = on_screen[group.leader];
        let members: Vec<usize> = group.members.iter().map(|&m| on_screen[m].0).collect();
        let radius = if members.len() == 1 {
            let point = &features.points[leader];
            let radius = marker_radius(point.value, features.value_range) * scale;
            out.markers.push(Shape::circle_filled(at, radius, color));
            out.markers.push(Shape::circle_stroke(
                at,
                radius,
                Stroke::new(1.0 * scale, outline),
            ));
            if !point.name.is_empty() {
                out.named.push(NamedPoint {
                    at,
                    offset: radius + 4.0 * scale,
                    name: point.name.clone(),
                    opacity: layer.opacity,
                });
            }
            radius
        } else {
            let radius = badge_radius(members.len()) * scale;
            out.markers.push(Shape::circle_filled(at, radius, color));
            out.markers.push(Shape::circle_stroke(
                at,
                radius,
                Stroke::new(1.5 * scale, rim),
            ));
            out.counts
                .push((at, count_text(members.len()), layer.opacity));
            radius
        };
        if let Some(strength) = members.iter().filter_map(|&m| recency(m)).reduce(f32::max) {
            let value = members
                .iter()
                .filter_map(|&m| features.points[m].value)
                .reduce(f64::max)
                .unwrap_or(0.0);
            pulses.push((
                value,
                Pulse {
                    at,
                    radius,
                    color,
                    strength,
                    phase: (leader as f32 * 0.618_034).fract(),
                },
            ));
        }
        out.rooms
            .push(Rect::from_center_size(at, Vec2::splat(2.0 * radius)).expand(padding));
        out.hits.push(Hit {
            at,
            radius,
            members,
        });
    }
    pulses.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.pulses = pulses
        .into_iter()
        .take(MAX_PULSES)
        .map(|(_, pulse)| pulse)
        .collect();
    out
}

/// The rings of the pulsing markers at the interface's clock `now` (seconds), and a
/// redraw while any run.
pub(super) fn paint_pulses(painter: &egui::Painter, pulses: &[Pulse], scale: f32, now: f64) {
    if pulses.is_empty() {
        return;
    }
    for pulse in pulses {
        for ring in [0.0, 0.5] {
            let t = ((now / PULSE_PERIOD_SECS) as f32 + pulse.phase + ring).fract();
            let radius = pulse.radius * (1.0 + (PULSE_REACH - 1.0) * t);
            let alpha = (1.0 - t) * (1.0 - t) * pulse.strength * 0.9;
            painter.circle_stroke(
                pulse.at,
                radius,
                Stroke::new(1.6 * scale, pulse.color.gamma_multiply(alpha)),
            );
        }
    }
    painter.ctx().request_repaint_after(PULSE_FRAME);
}

/// The badges' counts, laid out now (a kept layout can point at glyphs a refilled
/// font atlas has moved).
pub(super) fn paint_counts(painter: &egui::Painter, counts: &[(Pos2, String, f32)], scale: f32) {
    let font = crate::app::fonts::map_font(painter.ctx(), CLUSTER_TEXT_POINTS * scale);
    for (at, text, opacity) in counts {
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), Color32::WHITE);
        let top_left = *at - galley.size() / 2.0;
        painter.add(Shape::Text(
            egui::epaint::TextShape::new(top_left, galley, Color32::WHITE)
                .with_opacity_factor(*opacity),
        ));
    }
}

/// The hit under `pointer`, if any.
pub(super) fn hit_at(hits: &[Hit], pointer: Pos2, scale: f32) -> Option<&Hit> {
    hits.iter()
        .filter(|hit| hit.at.distance(pointer) <= hit.radius + HOVER_SLACK_POINTS * scale)
        .min_by(|a, b| {
            a.at.distance_sq(pointer)
                .total_cmp(&b.at.distance_sq(pointer))
        })
}

/// A value as people write it: whole numbers without decimals, others with up to two.
fn format_value(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// The hover card's text for a marker or badge.
pub(super) fn hover_text(lang: Lang, features: &FeatureSet, hit: &Hit) -> String {
    let field = features.value_field.as_deref();
    if let [index] = hit.members[..] {
        let point = &features.points[index];
        let mut lines = vec![if point.name.is_empty() {
            "—".to_owned()
        } else {
            point.name.clone()
        }];
        if let (Some(field), Some(value)) = (field, point.value) {
            lines.push(format!("{field}: {}", format_value(value)));
        }
        if let Some(ms) = point.time_ms {
            lines.push(lang.format("user-point-time", &[("time", format_time_utc(ms).into())]));
        }
        if !point.details.is_empty() {
            lines.push(point.details.clone());
        }
        return lines.join("\n");
    }
    let mut lines = vec![lang.format("user-cluster-hover", &[("count", hit.members.len().into())])];
    for &index in hit.members.iter().take(CLUSTER_HOVER_NAMES) {
        let point = &features.points[index];
        let name = if point.name.is_empty() {
            "—"
        } else {
            &point.name
        };
        lines.push(match (field, point.value) {
            (Some(_), Some(value)) => format!("• {name} ({})", format_value(value)),
            _ => format!("• {name}"),
        });
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_grow_with_their_value_and_badges_with_their_count() {
        let magnitudes = Some((2.5, 7.0));
        assert_eq!(marker_radius(None, magnitudes), MARKER_POINTS);
        assert_eq!(marker_radius(Some(2.5), magnitudes), MIN_SIZED_POINTS);
        assert_eq!(marker_radius(Some(7.0), magnitudes), MAX_SIZED_POINTS);
        let middle = marker_radius(Some(4.75), magnitudes);
        assert!((middle - (MIN_SIZED_POINTS + MAX_SIZED_POINTS) / 2.0).abs() < 1e-4);
        // Populations span a wide range: sized by area, so the middle is larger.
        let people = Some((1_000.0, 1_000_000.0));
        assert!(marker_radius(Some(250_000.0), people) > MIN_SIZED_POINTS + 3.0);
        assert!(badge_radius(2) < badge_radius(50));
        assert_eq!(badge_radius(5_000), CLUSTER_MAX_POINTS);
        assert_eq!(
            (count_text(12), count_text(12_345)),
            ("12".into(), "12k".into())
        );
    }

    #[test]
    fn hover_cards_name_the_point_or_the_cluster() {
        let json = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"title":"M 6.1 - Chile","mag":6.1,
              "time":1793785080000,"tsunami":1},
             "geometry":{"type":"Point","coordinates":[-71.0,-33.0]}},
            {"type":"Feature","properties":{"title":"M 2.5 - Chile","mag":2.5},
             "geometry":{"type":"Point","coordinates":[-71.1,-33.1]}}
        ]}"#;
        let features = globecraft_data::user_data::parse_features(
            globecraft_data::user_data::FeatureFormat::GeoJson,
            json,
            None,
        )
        .unwrap();
        let single = Hit {
            at: Pos2::ZERO,
            radius: 5.0,
            members: vec![0],
        };
        assert_eq!(
            hover_text(Lang::En, &features, &single),
            "M 6.1 - Chile\nmag: 6.1\nTime: 2026-11-04 09:38 UTC\ntsunami: 1"
        );
        let both = Hit {
            members: vec![0, 1],
            ..single
        };
        let text = hover_text(Lang::En, &features, &both);
        assert!(text.starts_with("2 points here"), "{text}");
        assert!(
            text.contains("• M 6.1 - Chile (6.1)\n• M 2.5 - Chile (2.5)"),
            "{text}"
        );
        assert!(hit_at(&[both], Pos2::new(6.0, 0.0), 1.0).is_some());
    }
}
