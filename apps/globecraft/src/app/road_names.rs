//! Street and road names written along the roads, as on a road map: each name sits on
//! a nearly straight stretch of its road long enough to hold it, turned to the road's
//! direction and kept upright. Main roads are named first and from higher up; names
//! that would overlap one already placed, or repeat the same name close by, are left
//! out.
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Vec2};
use globecraft_data::roads::Road;
use globecraft_render::live_layers::ProjectedRoad;
use std::sync::Arc;

/// Main roads are named below this altitude, smaller roads and streets below the next.
const MAJOR_NAME_MAX_ALTITUDE_KM: f64 = 40.0;
const MINOR_NAME_MAX_ALTITUDE_KM: f64 = 8.0;
/// The most names one view writes.
const MAX_NAMES: usize = 160;
/// The same name is written again only this far (points) from its last placement.
const REPEAT_DISTANCE_POINTS: f32 = 260.0;
/// Free space kept around a name (points).
const NAME_MARGIN_POINTS: f32 = 3.0;
/// How far the road may bend away from the straight line under a name, as a share of
/// the text height.
const MAX_BEND_SHARE: f32 = 0.45;
const TEXT_COLOR: Color32 = Color32::from_rgb(244, 246, 250);
const HALO_COLOR: Color32 = Color32::from_rgba_premultiplied(10, 14, 20, 210);

fn major(road: &Road) -> bool {
    matches!(
        road.kind.as_str(),
        "motorway" | "trunk" | "primary" | "secondary"
    )
}

/// The opacity of a road's name at this altitude (0: not named).
fn name_opacity(road: &Road, altitude_km: f64) -> f32 {
    let max = if major(road) {
        MAJOR_NAME_MAX_ALTITUDE_KM
    } else {
        MINOR_NAME_MAX_ALTITUDE_KM
    };
    globecraft_render::live_layers::altitude_fade(max, altitude_km)
}

/// Where a name `width` long goes along `line`: the centre of the straightest
/// stretch near the middle of the line that holds it, and the angle (radians) of the
/// stretch, turned so the text reads left to right.
pub(super) fn anchor(line: &[Pos2], width: f32, height: f32) -> Option<(Pos2, f32)> {
    if line.len() < 2 {
        return None;
    }
    let mut lengths = Vec::with_capacity(line.len());
    let mut total = 0.0;
    lengths.push(0.0);
    for pair in line.windows(2) {
        total += pair[0].distance(pair[1]);
        lengths.push(total);
    }
    if total < width {
        return None;
    }
    // The point `s` along the line.
    let at = |s: f32| {
        let i = lengths.partition_point(|&l| l < s).clamp(1, line.len() - 1);
        let (a, b) = (line[i - 1], line[i]);
        let span = lengths[i] - lengths[i - 1];
        let t = if span > 0.0 {
            (s - lengths[i - 1]) / span
        } else {
            0.0
        };
        a + (b - a) * t
    };
    // Candidate centres: the middle first, then further out.
    let half = width / 2.0;
    let free = total - width;
    for share in [0.5, 0.35, 0.65, 0.2, 0.8, 0.05, 0.95] {
        let start = free * share;
        let (from, to) = (at(start), at(start + width));
        let chord = to - from;
        if chord.length() < width * 0.92 {
            continue;
        }
        let direction = chord.normalized();
        let normal = direction.rot90();
        let bend = (0..line.len())
            .filter(|&i| lengths[i] > start && lengths[i] < start + width)
            .map(|i| (line[i] - from).dot(normal).abs())
            .fold(0.0, f32::max);
        if bend > height * MAX_BEND_SHARE {
            continue;
        }
        let mut angle = direction.angle();
        if angle > std::f32::consts::FRAC_PI_2 {
            angle -= std::f32::consts::PI;
        } else if angle < -std::f32::consts::FRAC_PI_2 {
            angle += std::f32::consts::PI;
        }
        return Some((at(start + half), angle));
    }
    None
}

/// The corners of a `size` box centred on `centre`, turned by `angle`, as a bounding
/// rectangle (enough to keep names apart).
fn turned_bounds(centre: Pos2, size: Vec2, angle: f32) -> Rect {
    let (sin, cos) = angle.sin_cos();
    let half = size / 2.0;
    let extent = Vec2::new(
        (half.x * cos).abs() + (half.y * sin).abs(),
        (half.x * sin).abs() + (half.y * cos).abs(),
    );
    Rect::from_center_size(centre, extent * 2.0)
}

/// A galley drawn with its centre on `centre`, turned by `angle`.
fn turned_text(centre: Pos2, galley: Arc<egui::Galley>, angle: f32, opacity: f32) -> Shape {
    let half = galley.size() / 2.0;
    let rotation = egui::emath::Rot2::from_angle(angle);
    let position = centre - rotation * half;
    Shape::Text(
        egui::epaint::TextShape::new(position, galley, TEXT_COLOR)
            .with_angle(angle)
            .with_opacity_factor(opacity),
    )
}

/// The name shapes for the roads on screen.
pub(super) fn names(
    painter: &egui::Painter,
    roads: &[Road],
    projected: &[ProjectedRoad],
    viewport: Rect,
    altitude_km: f64,
    size_points: f32,
    scale: f32,
) -> Vec<Shape> {
    let mut candidates: Vec<(&ProjectedRoad, &Road, f32, f32)> = projected
        .iter()
        .filter_map(|p| {
            let road = roads.get(p.source_index)?;
            if road.name.is_empty() {
                return None;
            }
            let opacity = name_opacity(road, altitude_km);
            let length: f32 = p
                .segments
                .iter()
                .flat_map(|line| line.windows(2))
                .map(|pair| pair[0].distance(pair[1]))
                .sum();
            (opacity > 0.0).then_some((p, road, opacity, length))
        })
        .collect();
    // Main roads first, then the longest on screen.
    candidates.sort_by(|a, b| major(b.1).cmp(&major(a.1)).then(b.3.total_cmp(&a.3)));
    let font = crate::app::fonts::map_font(painter.ctx(), size_points * scale);
    let margin = NAME_MARGIN_POINTS * scale;
    let repeat = REPEAT_DISTANCE_POINTS * scale;
    let mut placed: Vec<Rect> = Vec::new();
    let mut written: Vec<(&str, Pos2)> = Vec::new();
    let mut shapes = Vec::new();
    for (projected, road, opacity, _) in candidates {
        if placed.len() >= MAX_NAMES {
            break;
        }
        let galley = painter.layout_no_wrap(road.name.clone(), font.clone(), TEXT_COLOR);
        let size = galley.size();
        for line in &projected.segments {
            let Some((centre, angle)) = anchor(line, size.x + 2.0 * margin, size.y) else {
                continue;
            };
            if !viewport.shrink(margin).contains(centre)
                || written
                    .iter()
                    .any(|(name, at)| *name == road.name && at.distance(centre) < repeat)
            {
                continue;
            }
            let bounds = turned_bounds(centre, size + Vec2::splat(2.0 * margin), angle);
            if placed.iter().any(|other| other.intersects(bounds)) {
                continue;
            }
            let halo = painter.layout_no_wrap(road.name.clone(), font.clone(), HALO_COLOR);
            let normal = Vec2::angled(angle).rot90();
            for offset in [normal, -normal, Vec2::angled(angle), -Vec2::angled(angle)] {
                shapes.push(turned_text(
                    centre + offset * scale,
                    halo.clone(),
                    angle,
                    opacity,
                ));
            }
            shapes.push(turned_text(centre, galley.clone(), angle, opacity));
            placed.push(bounds);
            written.push((road.name.as_str(), centre));
            break;
        }
    }
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_sits_on_the_straight_middle_of_its_road() {
        let line = [Pos2::new(0.0, 100.0), Pos2::new(400.0, 100.0)];
        let (centre, angle) = anchor(&line, 100.0, 12.0).unwrap();
        assert_eq!(centre, Pos2::new(200.0, 100.0));
        assert_eq!(angle, 0.0);
    }

    #[test]
    fn names_read_left_to_right_whichever_way_the_road_was_drawn() {
        let line = [Pos2::new(400.0, 100.0), Pos2::new(0.0, 100.0)];
        let (_, angle) = anchor(&line, 100.0, 12.0).unwrap();
        assert!(angle.abs() < 1e-5, "{angle}");
        let down = [Pos2::new(0.0, 0.0), Pos2::new(0.0, 300.0)];
        let (_, angle) = anchor(&down, 100.0, 12.0).unwrap();
        assert!((angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn short_or_winding_roads_get_no_name() {
        let short = [Pos2::new(0.0, 0.0), Pos2::new(50.0, 0.0)];
        assert!(anchor(&short, 100.0, 12.0).is_none());
        let zigzag: Vec<Pos2> = (0..20)
            .map(|i| Pos2::new(i as f32 * 10.0, if i % 2 == 0 { 0.0 } else { 30.0 }))
            .collect();
        assert!(anchor(&zigzag, 100.0, 12.0).is_none());
    }

    #[test]
    fn main_roads_are_named_from_higher_up() {
        let road = |kind: &str| Road {
            name: "Name".into(),
            kind: kind.into(),
            restricted: false,
            points: Vec::new(),
        };
        assert!(name_opacity(&road("primary"), 15.0) > 0.0);
        assert_eq!(name_opacity(&road("residential"), 15.0), 0.0);
        assert!(name_opacity(&road("residential"), 3.0) > 0.0);
    }
}
