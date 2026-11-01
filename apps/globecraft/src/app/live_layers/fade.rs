//! Fades for the map-data layers (roads, railways, airports, rivers, parks and their
//! names), as the borders fade: a layer that appears fades in, and when new tiles
//! arrive for the same view the new set fades in over the old one, which then fades
//! out. The altitude fade of each kind is part of the shapes' colours already.
use eframe::egui::{self, Shape};
use globecraft_data::settings::map_style::LineStyle;
use std::time::{Duration, Instant};

/// How long a layer takes to fade in once it has something to show.
const ARRIVAL_FADE: Duration = Duration::from_millis(600);
/// How long a new set of tiles takes to replace the one before it.
const EXCHANGE_FADE: Duration = Duration::from_millis(500);

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn progress(since: Instant, now: Instant, length: Duration) -> f32 {
    now.saturating_duration_since(since).as_secs_f32() / length.as_secs_f32()
}

/// Opacities of the incoming and outgoing sets at `t` (0..1) of an exchange: the new
/// set fades in over the first half, then the old one fades out.
fn exchange_opacities(t: f32) -> (f32, f32) {
    (smoothstep(2.0 * t), 1.0 - smoothstep(2.0 * t - 1.0))
}

/// One layer's shapes and the fades running on them.
#[derive(Default)]
pub(in crate::app) struct FadingShapes {
    current: Vec<Shape>,
    /// When the layer went from nothing to something.
    arrived: Option<Instant>,
    /// The set shown before, fading out, and when it was replaced.
    outgoing: Option<(Vec<Shape>, Instant)>,
}

impl FadingShapes {
    /// New shapes for the layer. `new_data`: they come from new tiles for the same
    /// view (the old set is still in place and crossfades); otherwise the view moved
    /// and the old set, drawn for the old view, is dropped.
    pub(in crate::app) fn replace(&mut self, shapes: Vec<Shape>, new_data: bool, now: Instant) {
        let old = std::mem::replace(&mut self.current, shapes);
        if self.current.is_empty() {
            self.arrived = None;
            self.outgoing = None;
        } else if old.is_empty() {
            self.arrived = Some(now);
            self.outgoing = None;
        } else if new_data {
            self.outgoing = Some((old, now));
        } else {
            self.outgoing = None;
        }
    }

    /// Draws the layer; requests another frame while a fade runs.
    pub(in crate::app) fn draw(&mut self, painter: &egui::Painter, now: Instant) {
        let arrival = match self.arrived {
            Some(at) if progress(at, now, ARRIVAL_FADE) < 1.0 => {
                smoothstep(progress(at, now, ARRIVAL_FADE))
            }
            _ => {
                self.arrived = None;
                1.0
            }
        };
        let (incoming, outgoing) = match &self.outgoing {
            Some((_, at)) if progress(*at, now, EXCHANGE_FADE) < 1.0 => {
                exchange_opacities(progress(*at, now, EXCHANGE_FADE))
            }
            _ => {
                self.outgoing = None;
                (1.0, 0.0)
            }
        };
        if let Some((old, _)) = &self.outgoing
            && outgoing > 0.0
        {
            extend_with_opacity(painter, old, outgoing);
        }
        extend_with_opacity(painter, &self.current, arrival * incoming);
        if self.arrived.is_some() || self.outgoing.is_some() {
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
    match globecraft_render::overlays::dash_points(style.dash) {
        Some((on, off)) => {
            shapes.extend(Shape::dashed_line(points, stroke, on * scale, off * scale))
        }
        None => shapes.push(Shape::line(points.to_vec(), stroke)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes(count: usize) -> Vec<Shape> {
        (0..count)
            .map(|_| Shape::circle_filled(egui::Pos2::ZERO, 1.0, egui::Color32::WHITE))
            .collect()
    }

    #[test]
    fn a_layer_that_appears_fades_in_and_new_tiles_crossfade() {
        let start = Instant::now();
        let mut layer = FadingShapes::default();
        layer.replace(shapes(2), true, start);
        assert_eq!(layer.arrived, Some(start), "appearing starts the fade-in");
        assert!(layer.outgoing.is_none());

        layer.arrived = None;
        layer.replace(shapes(3), true, start);
        assert!(
            layer
                .outgoing
                .as_ref()
                .is_some_and(|(old, _)| old.len() == 2),
            "new tiles for the same view keep the old set for the crossfade"
        );
        layer.replace(shapes(3), false, start);
        assert!(layer.outgoing.is_none(), "a moved view drops the old set");
        layer.replace(Vec::new(), true, start);
        assert!(layer.arrived.is_none() && layer.outgoing.is_none());
    }

    #[test]
    fn the_exchange_brings_the_new_set_in_before_the_old_one_goes() {
        assert_eq!(exchange_opacities(0.0), (0.0, 1.0));
        assert_eq!(exchange_opacities(0.5), (1.0, 1.0));
        assert_eq!(exchange_opacities(1.0), (1.0, 0.0));
    }
}
