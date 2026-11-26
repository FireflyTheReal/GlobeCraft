//! The tilt arc: a track around the left of the navigator's ring, from straight down
//! at its top through the steepest tilt to looking straight up into the sky at its end
//! (the whole lean `Cam::tilt_view` takes), as if the thumb were the camera circling
//! down around the point it looks at. Drag the thumb or click the track to tilt;
//! double-click for straight down. Far out, where the view is held level, the part out
//! of reach is drawn faint and the thumb stops at its edge. Shared by both navigator
//! styles.
use crate::app::App;
use eframe::egui::{self, Pos2, Sense, Stroke, Vec2};
use globecraft_core::camera::{MAX_LOOK_UP_DEG, MAX_TILT_DEG};

/// The arc's distance outside the ring's edge.
const ARC_GAP_POINTS: f32 = 10.0;
/// Where straight down sits on the arc, in degrees from the ring's top toward its
/// left, and how far round the arc goes to its end (it was the 85° of tilt alone
/// before looking up joined it, user 2026-11-19).
const ARC_START_DEG: f32 = 32.0;
const ARC_SPAN_DEG: f32 = 115.0;
/// The whole lean the arc covers, degrees from straight down.
const MAX_VIEW_TILT_DEG: f64 = MAX_TILT_DEG + MAX_LOOK_UP_DEG;
const ARC_SAMPLES: usize = 32;
/// The tick where the view leaves the ground for the sky, its length.
const SKY_TICK_POINTS: f32 = 5.0;
const TRACK_OUTER_WIDTH_POINTS: f32 = 7.0;
const TRACK_INNER_WIDTH_POINTS: f32 = 4.0;
const THUMB_RADIUS_POINTS: f32 = 6.5;
/// Hit boxes along the track (it curves around the ring's square drag area, so it
/// takes clicks in pieces) and their size.
const HIT_BOXES: usize = 12;
const HIT_BOX_POINTS: f32 = 15.0;
/// The lens dot in the thumb and its opacity while not hovered.
const LENS_POINTS: f32 = 2.4;
const LENS_OPACITY: f32 = 0.85;
/// Opacity of the stretch between straight down and the current tilt.
const TILTED_OPACITY: f32 = 0.55;

/// Where lean `degrees` sits on the arc around `centre`, `radius` out.
pub(in crate::app) fn arc_point(centre: Pos2, radius: f32, degrees: f32) -> Pos2 {
    let angle = (ARC_START_DEG + degrees / MAX_VIEW_TILT_DEG as f32 * ARC_SPAN_DEG).to_radians();
    centre + Vec2::new(-angle.sin(), -angle.cos()) * radius
}

/// The tilt under the pointer at `p`: its angle round the arc, kept on the track (a
/// pointer past either end takes the nearer end).
fn tilt_at(centre: Pos2, p: Pos2) -> f64 {
    let v = p - centre;
    // Degrees from the top toward the left, as `arc_point` measures them, from the
    // arc's start.
    let along = f64::from((-v.x).atan2(-v.y).to_degrees() - ARC_START_DEG).rem_euclid(360.0);
    let span = f64::from(ARC_SPAN_DEG);
    let past_the_end = span + (360.0 - span) / 2.0;
    if along > past_the_end {
        0.0
    } else {
        along.min(span) / span * MAX_VIEW_TILT_DEG
    }
}

impl App {
    /// Draws the tilt arc around the ring at `centre` (ring radius `r_ring`, controls
    /// scale `k`) and applies its input. Returns true while it is being operated.
    pub(super) fn tilt_arc(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        centre: Pos2,
        r_ring: f32,
        k: f32,
        id: egui::Id,
    ) -> bool {
        let lg = self.lang;
        let t = crate::app::ui::theme::Tokens::get(ui.ctx());
        let radius = r_ring + ARC_GAP_POINTS * k;
        #[cfg(test)]
        {
            self.nav.tilt_arc = Some((centre, radius));
        }
        let mut active = false;
        let mut hot = self.nav.tilt_drag;
        let mut tooltip_shown = false;
        let max = MAX_VIEW_TILT_DEG as f32;
        for i in 0..HIT_BOXES {
            let at = arc_point(centre, radius, max * i as f32 / (HIT_BOXES - 1) as f32);
            let response = ui.interact(
                egui::Rect::from_center_size(at, Vec2::splat(HIT_BOX_POINTS * k)),
                id.with(("tilt", i)),
                Sense::click_and_drag(),
            );
            hot |= response.hovered();
            if response.double_clicked() {
                self.cam.tilt_view(0.0);
                self.motion.fly = None;
                self.nav.tilt_drag = false;
                active = true;
            } else if (response.drag_started() || response.clicked())
                && let Some(p) = response.interact_pointer_pos()
            {
                self.nav.tilt_drag = response.drag_started();
                self.cam.tilt_view(tilt_at(centre, p));
                self.motion.fly = None;
                active = true;
            }
            if response.hovered() && !tooltip_shown {
                tooltip_shown = true;
                let degrees = self.cam.view_tilt().round() as i64;
                response.on_hover_text(
                    lg.format("navigation-tilt-tooltip", &[("degrees", degrees.into())]),
                );
            }
        }
        // A drag follows the pointer anywhere until the button is released.
        if self.nav.tilt_drag {
            match ui.input(|i| i.pointer.primary_down().then(|| i.pointer.interact_pos())) {
                Some(Some(p)) => {
                    self.cam.tilt_view(tilt_at(centre, p));
                    self.motion.fly = None;
                    active = true;
                }
                Some(None) => active = true,
                None => self.nav.tilt_drag = false,
            }
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // The track, as the zoom slider's, and the stretch tilted so far. Out of reach
        // here (the view levels as it zooms out) it is only its dark edge.
        let along = |from: f32, to: f32| -> Vec<Pos2> {
            (0..=ARC_SAMPLES)
                .map(|i| {
                    arc_point(
                        centre,
                        radius,
                        from + (to - from) * i as f32 / ARC_SAMPLES as f32,
                    )
                })
                .collect()
        };
        let fill = t.ribbon.gamma_multiply(super::CONTROL_FILL_OPACITY);
        let ring_bg = t.pane_header.gamma_multiply(super::RING_OPACITY);
        painter.add(egui::Shape::line(
            along(0.0, max),
            Stroke::new(TRACK_OUTER_WIDTH_POINTS * k, ring_bg),
        ));
        let reach = (self.cam.max_view_tilt() as f32).clamp(0.0, max);
        if reach > 0.0 {
            painter.add(egui::Shape::line(
                along(0.0, reach),
                Stroke::new(TRACK_INNER_WIDTH_POINTS * k, fill),
            ));
        }
        // Where the view leaves the ground for the sky.
        let sky = MAX_TILT_DEG as f32;
        let inward = (centre - arc_point(centre, radius, sky)).normalized();
        let tick = arc_point(centre, radius, sky);
        painter.line_segment(
            [
                tick - inward * SKY_TICK_POINTS * k,
                tick + inward * SKY_TICK_POINTS * k,
            ],
            Stroke::new(1.0, t.text_dim.gamma_multiply(TILTED_OPACITY)),
        );
        let tilt = (self.cam.view_tilt() as f32).clamp(0.0, max);
        let samples = ((ARC_SAMPLES as f32 * tilt / max).ceil() as usize).max(1);
        let tilted: Vec<Pos2> = (0..=samples)
            .map(|i| arc_point(centre, radius, tilt * i as f32 / samples as f32))
            .collect();
        painter.add(egui::Shape::line(
            tilted,
            Stroke::new(2.0 * k, t.accent.gamma_multiply(TILTED_OPACITY)),
        ));
        // The thumb, with a small triangle pointing at the ring's centre: the camera
        // looking at the point it circles.
        let thumb = arc_point(centre, radius, tilt);
        painter.circle_filled(
            thumb,
            THUMB_RADIUS_POINTS * k,
            if hot { t.hover } else { fill },
        );
        painter.circle_stroke(
            thumb,
            THUMB_RADIUS_POINTS * k,
            Stroke::new(1.0, if hot { t.accent } else { t.border_strong }),
        );
        // A lens in the thumb: it is the camera, circling the point it looks at.
        painter.circle_filled(
            thumb,
            LENS_POINTS * k,
            if hot {
                t.accent
            } else {
                t.accent.gamma_multiply(LENS_OPACITY)
            },
        );
        active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arc_maps_tilts_and_back() {
        let centre = Pos2::new(100.0, 100.0);
        for degrees in [0.0, 20.0, 45.0, 80.0, 120.0, 175.0] {
            let p = arc_point(centre, 50.0, degrees);
            assert!((tilt_at(centre, p) - f64::from(degrees)).abs() < 1e-3);
        }
        // Straight down is up and to the left of the centre, the steepest tilt on the
        // left, straight up into the sky below the middle on the left.
        let top = arc_point(centre, 50.0, 0.0);
        assert!(top.x < centre.x && top.y < centre.y);
        let steep = arc_point(centre, 50.0, MAX_TILT_DEG as f32);
        assert!(steep.x < centre.x);
        let up = arc_point(centre, 50.0, MAX_VIEW_TILT_DEG as f32);
        assert!(up.x < centre.x && up.y > centre.y);
        // Beyond either end the pointer keeps the nearest end.
        assert_eq!(tilt_at(centre, Pos2::new(100.0, 40.0)), 0.0);
        assert_eq!(tilt_at(centre, Pos2::new(110.0, 160.0)), MAX_VIEW_TILT_DEG);
    }
}
