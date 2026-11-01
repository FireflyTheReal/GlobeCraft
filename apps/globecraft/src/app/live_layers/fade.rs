//! Fades for the map-data layers (roads, railways, airports, rivers, parks and their
//! names), as the borders fade: each tile's drawing fades in when the tile is shown,
//! and a replaced tile fades out after its replacement has faded in, also while the
//! view moves (`vector_tiles::TileFade`). The altitude fade of each kind is part of the
//! shapes' colours already.
use eframe::egui::{self, Shape};
use globecraft_data::settings::map_style::LineStyle;
use globecraft_data::vector_tiles::TileFade;
use std::time::Instant;

/// A layer's shapes, grouped by the fade of the tile they came from.
#[derive(Default)]
pub(in crate::app) struct TiledShapes {
    groups: Vec<(TileFade, Vec<Shape>)>,
}

impl TiledShapes {
    /// Groups `shapes` by fade (keeping their order within a group) and tessellates
    /// the lines of each group once.
    pub(in crate::app) fn build(
        painter: &egui::Painter,
        shapes: impl IntoIterator<Item = (TileFade, Shape)>,
    ) -> Self {
        let mut groups: Vec<(TileFade, Vec<Shape>)> = Vec::new();
        for (fade, shape) in shapes {
            match groups.iter_mut().find(|(f, _)| *f == fade) {
                Some((_, group)) => group.push(shape),
                None => groups.push((fade, vec![shape])),
            }
        }
        Self {
            groups: groups
                .into_iter()
                .map(|(fade, shapes)| {
                    (
                        fade,
                        super::super::live_geometry::mesh_shapes(painter, shapes),
                    )
                })
                .collect(),
        }
    }

    /// Draws each group at its fade's opacity; asks for another frame while one runs.
    pub(in crate::app) fn draw(&self, painter: &egui::Painter, now: Instant) {
        let mut animating = false;
        for (fade, shapes) in &self.groups {
            animating |= fade.animating(now);
            extend_with_opacity(painter, shapes, fade.opacity(now));
        }
        if animating {
            painter.ctx().request_repaint();
        }
    }
}

fn extend_with_opacity(painter: &egui::Painter, shapes: &[Shape], opacity: f32) {
    if opacity <= 0.0 || shapes.is_empty() {
        return;
    }
    if opacity >= 1.0 {
        painter.extend(shapes.iter().cloned());
    } else {
        let mut faded = painter.clone();
        faded.multiply_opacity(opacity);
        faded.extend(shapes.iter().cloned());
    }
}

/// A line in a user's style (colour, width, dash) at `scale`, `opacity` times its
/// own colour's alpha.
pub(in crate::app) fn styled_line(
    shapes: &mut Vec<Shape>,
    points: &[egui::Pos2],
    style: &LineStyle,
    scale: f32,
    opacity: f32,
) {
    if points.len() < 2 || opacity <= 0.0 {
        return;
    }
    let [r, g, b, a] = style.color.0;
    let color = egui::Color32::from_rgba_unmultiplied(r, g, b, a).gamma_multiply(opacity);
    let stroke = egui::Stroke::new(style.width_points * scale, color);
    for run in stroke_runs(points) {
        match globecraft_render::overlays::dash_points(style.dash) {
            Some((on, off)) => {
                shapes.extend(Shape::dashed_line(&run, stroke, on * scale, off * scale))
            }
            None => shapes.push(Shape::line(run, stroke)),
        }
    }
}

/// Below this cosine between one step of a line and the next, the line turns back
/// on itself (about 165°) and is drawn as two strokes.
const REVERSAL_COSINE: f32 = -0.966;

/// `points` as the polylines egui can stroke: repeated points dropped, and split
/// where the line turns back on itself. egui's corner join for a line that doubles
/// back divides by a length that rounds to nearly zero and throws a vertex thousands
/// of points away along the line, which drew straight lines across the whole map at
/// some camera positions (user report 2026-11-01). Tile clipping and simplification
/// leave such back-and-forth steps in roads.
pub(in crate::app) fn stroke_runs(points: &[egui::Pos2]) -> Vec<Vec<egui::Pos2>> {
    let mut runs = Vec::new();
    let mut run: Vec<egui::Pos2> = Vec::with_capacity(points.len());
    for &point in points {
        if run.last() == Some(&point) {
            continue;
        }
        if let [.., before, last] = run[..] {
            let (a, b) = ((last - before).normalized(), (point - last).normalized());
            if a.dot(b) < REVERSAL_COSINE {
                runs.push(std::mem::replace(&mut run, vec![last]));
            }
        }
        run.push(point);
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn lines_that_turn_back_stay_near_their_points() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::EVERYTHING);
        let style = LineStyle {
            color: globecraft_data::settings::map_style::Rgba([200, 200, 200, 255]),
            width_points: 3.0,
            dash: globecraft_data::settings::map_style::DashStyle::Solid,
        };
        let p = egui::pos2;
        // Lines out and partly back along themselves: egui alone throws some corner
        // vertices thousands of points away along the line.
        let far = |shapes: Vec<Shape>, near: egui::Pos2| {
            super::super::super::live_geometry::mesh_shapes(&painter, shapes)
                .iter()
                .any(|shape| match shape {
                    Shape::Mesh(mesh) => mesh
                        .vertices
                        .iter()
                        .any(|v| !v.pos.is_finite() || (v.pos - near).length() > 600.0),
                    _ => false,
                })
        };
        let (mut raw_spikes, mut spikes) = (0, 0);
        for i in 0..4000_u16 {
            let f = f32::from(i);
            let a = p(100.0 + (f * 7.31) % 400.0, 100.0 + (f * 3.17) % 400.0);
            let b = p(100.0 + (f * 5.03) % 400.0, 100.0 + (f * 9.71) % 400.0);
            let line = [a, b, a + (b - a) * ((f * 0.137) % 1.0)];
            let raw = Shape::line(line.to_vec(), egui::Stroke::new(3.0, egui::Color32::WHITE));
            raw_spikes += usize::from(far(vec![raw], b));
            let mut shapes = Vec::new();
            styled_line(&mut shapes, &line, &style, 1.0, 1.0);
            spikes += usize::from(far(shapes, b));
        }
        assert!(
            raw_spikes > 0,
            "egui no longer spikes: the split may be unnecessary"
        );
        assert_eq!(spikes, 0);
        // Repeats go, ordinary corners stay one stroke.
        let runs = stroke_runs(&[p(0.0, 0.0), p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]);
        assert_eq!(runs, vec![vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]]);
    }

    #[test]
    fn shapes_group_by_the_fade_of_their_tile() {
        let ctx = egui::Context::default();
        // Tessellation needs the fonts, which the first frame loads.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::EVERYTHING);
        let now = Instant::now();
        let fresh = TileFade {
            shown: Some(now),
            retired: None,
        };
        let dot = || Shape::circle_filled(egui::Pos2::ZERO, 1.0, egui::Color32::WHITE);
        let shapes = TiledShapes::build(
            &painter,
            [
                (TileFade::default(), dot()),
                (fresh, dot()),
                (TileFade::default(), dot()),
            ],
        );
        assert_eq!(shapes.groups.len(), 2);
        assert_eq!(fresh.opacity(now), 0.0);
        assert_eq!(fresh.opacity(now + Duration::from_secs(1)), 1.0);
        let replaced = TileFade {
            shown: None,
            retired: Some(now),
        };
        assert_eq!(
            replaced.opacity(now + Duration::from_millis(100)),
            1.0,
            "stays while the new one fades in"
        );
        assert_eq!(replaced.opacity(now + Duration::from_secs(1)), 0.0);
    }
}
