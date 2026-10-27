//! The on-globe navigator, top right. Two styles (Settings → Navigation):
//! - **Globe** (default): the classic rotation ring, cardinal letters and zoom
//!   slider, with a mini globe in the ring's centre in place of the look and move
//!   discs. Drag the mini globe to turn the Earth; it marks the area in view. Drag
//!   the ring to rotate the view, click a letter to turn that direction up.
//! - **Classic**: the original Google Earth-style ring, move pad and zoom slider
//!   (`classic.rs`).
//!
//! Both draw a few dozen shapes in the interface's own colour tokens.
use super::super::App;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use globecraft_core::math::wrap_lon;
use globecraft_data::settings::navigation::NavigatorStyle;
use mini_globe::{MiniGlobe, nearest_heading, screen_axes, view_cap_fraction};

mod classic;
mod mini_globe;
mod tilt;

// Geometry in egui points before UI scaling, matching the classic navigator.
const MIN_CONTROL_SCALE: f32 = 0.8;
const MAX_CONTROL_SCALE: f32 = 1.8;
const CONTROL_FILL_OPACITY: f32 = 0.94;
const RING_OPACITY: f32 = 0.82;
const CONTROL_RIGHT_MARGIN_POINTS: f32 = 56.0;
const CONTROL_TOP_MARGIN_POINTS: f32 = 52.0;
const ROTATION_RING_RADIUS_POINTS: f32 = 40.0;
const GLOBE_RADIUS_POINTS: f32 = 27.0;
/// Cardinal letters sit this far inside the ring's edge.
const CARDINAL_INSET_POINTS: f32 = 6.5;
const CARDINAL_HIT_POINTS: f32 = 16.0;
const CARDINAL_HOVER_SCALE: f32 = 1.12;
const ZOOM_GAP_POINTS: f32 = 16.0;
const ZOOM_BUTTON_RADIUS_POINTS: f32 = 11.0;
const ZOOM_TRACK_LENGTH_POINTS: f32 = 110.0;
/// Hold-to-zoom speed of the + and − buttons (natural log of the factor per second).
const ZOOM_HOLD_RATE: f64 = 1.6;
/// Scrolling over the navigator: 10 % per wheel notch (about 50 points).
const WHEEL_ZOOM_PER_POINT: f64 = 0.1 / 50.0;
/// Heading snaps ease in: the remaining turn shrinks to this fraction each second.
const SNAP_REMAINING_PER_SECOND: f64 = 0.0001;
const SNAP_DONE_DEG: f64 = 0.05;
/// Mini-globe drag: one globe radius moves the camera this share of the view's reach.
const GLOBE_DRAG_REACH_RATIO: f64 = 0.75;
/// The old fixed speed was a radian (57°) per radius; far out the drag now tops out here.
const MAX_GLOBE_DRAG_DEG_PER_RADIUS: f64 = 25.0;
const GRATICULE_STEP_DEG: usize = 30;
const GRATICULE_SAMPLES: usize = 48;
const GRID_OPACITY: f32 = 0.45;
const EQUATOR_OPACITY: f32 = 0.7;
const VIEW_FILL_OPACITY: f32 = 0.16;
const ZOOM_LEVEL_OPACITY: f32 = 0.55;

/// How far one mini-globe radius of drag moves the camera, in degrees of arc: about
/// the angle the view reaches from its centre, so the drag feels the same at every
/// altitude and gets fine close in (user, 2026-10-25: it moved too quickly), and at
/// most `MAX_GLOBE_DRAG_DEG_PER_RADIUS` far out.
fn globe_drag_deg_per_radius(altitude_km: f64, aspect: f64) -> f64 {
    let reach = view_cap_fraction(altitude_km, aspect)
        .clamp(0.0, 1.0)
        .asin()
        .to_degrees();
    (reach * GLOBE_DRAG_REACH_RATIO).min(MAX_GLOBE_DRAG_DEG_PER_RADIUS)
}

/// The cardinal letters: heading that turns them up, label, tooltip.
const CARDINALS: [(f64, &str, &str); 4] = [
    (0.0, "compass-north", "navigation-north-tooltip"),
    (90.0, "compass-east", "navigation-east-tooltip"),
    (180.0, "compass-south", "navigation-south-tooltip"),
    (270.0, "compass-west", "navigation-west-tooltip"),
];

impl App {
    /// Draws the chosen navigator and applies its input. Returns true while the user
    /// is operating it (or a heading snap is still turning).
    pub(in crate::app) fn nav_controls(&mut self, ui: &mut egui::Ui, rect: Rect, dt: f64) -> bool {
        if self.settings.hide_navigation_controls {
            self.ring_drag = false;
            self.tilt_drag = false;
            self.nav_snap_heading = None;
            return false;
        }
        match self.settings.navigation.navigator {
            NavigatorStyle::Globe => self.globe_nav_controls(ui, rect, dt),
            NavigatorStyle::Classic => {
                self.nav_snap_heading = None;
                self.classic_nav_controls(ui, rect, dt)
            }
        }
    }

    fn globe_nav_controls(&mut self, ui: &mut egui::Ui, rect: Rect, dt: f64) -> bool {
        let lg = self.lang;
        let k = self
            .settings
            .ui_scale
            .clamp(MIN_CONTROL_SCALE, MAX_CONTROL_SCALE);
        let painter = ui.painter_at(rect);
        let base = ui.id().with("nav");
        let mut active = false;

        // The classic navigator's palette.
        let t = super::theme::Tokens::get(ui.ctx());
        let fill = t.ribbon.gamma_multiply(CONTROL_FILL_OPACITY);
        let edge = t.border_strong;
        let ink = t.icon;
        let ring_bg = t.pane_header.gamma_multiply(RING_OPACITY);

        let centre = Pos2::new(
            rect.right() - CONTROL_RIGHT_MARGIN_POINTS * k,
            rect.top() + CONTROL_TOP_MARGIN_POINTS * k,
        );
        let (r_ring, r_globe) = (ROTATION_RING_RADIUS_POINTS * k, GLOBE_RADIUS_POINTS * k);

        // ── the ring and the mini globe share one drag area ──
        let dial = ui.interact(
            Rect::from_center_size(centre, Vec2::splat(2.0 * r_ring)),
            base.with("dial"),
            Sense::drag(),
        );
        let on_ring = |p: Pos2| (p - centre).length() >= r_globe;
        if dial.drag_started() && !self.tilt_drag {
            self.ring_drag = dial.interact_pointer_pos().is_some_and(on_ring);
            self.nav_snap_heading = None;
        }
        if dial.dragged()
            && let Some(cur) = dial.interact_pointer_pos()
        {
            let delta = ui.input(|i| i.pointer.delta());
            self.fly = None;
            active = true;
            if self.ring_drag {
                let prev = cur - delta;
                let angle = |v: Vec2| v.y.atan2(v.x);
                let mut d = angle(cur - centre) - angle(prev - centre);
                if d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                }
                if d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                self.cam.heading = wrap_lon(self.cam.heading - f64::from(d.to_degrees()));
            } else {
                // Grab the mini globe's surface: the camera moves the other way, by
                // about the view's reach per globe radius of drag.
                let (north, east) = screen_axes(self.cam.heading);
                let aspect = f64::from(rect.width() / rect.height().max(1.0));
                let deg_per_point =
                    globe_drag_deg_per_radius(self.cam.alt, aspect) / f64::from(r_globe.max(1.0));
                let moved = -delta;
                let coslat = self.cam.lat.to_radians().cos().max(0.05);
                self.cam.lat = (self.cam.lat + f64::from(moved.dot(north)) * deg_per_point).clamp(
                    -globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                    globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                );
                self.cam.lon =
                    wrap_lon(self.cam.lon + f64::from(moved.dot(east)) * deg_per_point / coslat);
            }
        }
        if dial.drag_stopped() {
            self.ring_drag = false;
        }
        let ring_hot = self.ring_drag || (!dial.dragged() && dial.hover_pos().is_some_and(on_ring));
        let globe_hot = !ring_hot && (dial.dragged() || dial.hovered());
        if globe_hot {
            ui.ctx().set_cursor_icon(if dial.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
        let dial_hovered = dial.hovered();
        dial.on_hover_text(lg.text(if ring_hot {
            "navigation-ring-tooltip"
        } else {
            "navigation-globe-tooltip"
        }));

        // Cardinal letters ride on the ring: a click turns that direction up.
        let heading = self.cam.heading;
        let letter_at = |target: f64| {
            let angle = (target - heading).to_radians();
            centre
                + Vec2::new(angle.sin() as f32, -angle.cos() as f32)
                    * (r_ring - CARDINAL_INSET_POINTS * k)
        };
        let mut letter_hot = [false; 4];
        for (i, &(target, _, tooltip)) in CARDINALS.iter().enumerate() {
            let response = ui.interact(
                Rect::from_center_size(letter_at(target), Vec2::splat(CARDINAL_HIT_POINTS * k)),
                base.with(("cardinal", i)),
                Sense::click(),
            );
            letter_hot[i] = response.hovered();
            if response.on_hover_text(lg.text(tooltip)).clicked() {
                self.nav_snap_heading = Some(nearest_heading(self.cam.heading, target));
                self.fly = None;
                active = true;
            }
        }

        // The tilt arc beside the ring.
        active |= self.tilt_arc(ui, &painter, centre, r_ring, k, base);

        // ── zoom: the classic + and − buttons and slider ──
        let r_btn = ZOOM_BUTTON_RADIUS_POINTS * k;
        let track_len = ZOOM_TRACK_LENGTH_POINTS * k;
        let y0 = centre.y + r_ring + ZOOM_GAP_POINTS * k + r_btn;
        let (t_top, t_bot) = (y0 + r_btn + 8.0 * k, y0 + r_btn + 8.0 * k + track_len);
        let mut zoom_hovered = false;
        for (c, plus) in [
            (Pos2::new(centre.x, y0), true),
            (Pos2::new(centre.x, t_bot + 8.0 * k + r_btn), false),
        ] {
            let r = hold_control(
                ui,
                Rect::from_center_size(c, Vec2::splat(2.0 * r_btn)),
                base.with(("zoombtn", plus)),
            );
            let held = r.is_pointer_button_down_on();
            zoom_hovered |= r.hovered();
            painter.circle_filled(c, r_btn, if held || r.hovered() { t.hover } else { fill });
            painter.circle_stroke(c, r_btn, Stroke::new(1.0, edge));
            let l = 5.0 * k;
            painter.line_segment(
                [c - Vec2::new(l, 0.0), c + Vec2::new(l, 0.0)],
                Stroke::new(2.2 * k, ink),
            );
            if plus {
                painter.line_segment(
                    [c - Vec2::new(0.0, l), c + Vec2::new(0.0, l)],
                    Stroke::new(2.2 * k, ink),
                );
            }
            r.on_hover_text(lg.text(if plus {
                "navigation-zoom-in-tooltip"
            } else {
                "navigation-zoom-out-tooltip"
            }));
            if held {
                self.fly = None;
                let rate = if plus {
                    -ZOOM_HOLD_RATE
                } else {
                    ZOOM_HOLD_RATE
                };
                self.zoom((rate * dt).exp());
                active = true;
            }
        }
        let track = ui.interact(
            Rect::from_min_max(
                Pos2::new(centre.x - 11.0 * k, t_top - 4.0 * k),
                Pos2::new(centre.x + 11.0 * k, t_bot + 4.0 * k),
            ),
            base.with("track"),
            Sense::click_and_drag(),
        );
        if (track.dragged() || track.clicked() || track.is_pointer_button_down_on())
            && let Some(p) = track.interact_pointer_pos()
        {
            self.cam
                .set_zoom_fraction(f64::from(((p.y - t_top) / track_len).clamp(0.0, 1.0)));
            self.fly = None;
            active = true;
        }
        let track_hot = track.hovered() || track.dragged();
        zoom_hovered |= track.hovered();
        track.on_hover_text(lg.text("navigation-zoom-slider-tooltip"));
        // The wheel zooms over the whole navigator, as it does over the globe.
        if dial_hovered || zoom_hovered {
            let scroll = f64::from(ui.input(|i| i.smooth_scroll_delta.y));
            if scroll != 0.0 {
                let direction = if self.settings.navigation.invert_scroll {
                    -1.0
                } else {
                    1.0
                };
                self.zoom((-scroll * WHEEL_ZOOM_PER_POINT * direction).exp());
                self.fly = None;
                active = true;
            }
        }

        // Ease a cardinal snap in.
        if let Some(target) = self.nav_snap_heading {
            let remaining = target - self.cam.heading;
            if remaining.abs() < SNAP_DONE_DEG {
                self.cam.heading = wrap_lon(target);
                self.nav_snap_heading = None;
            } else {
                let step = 1.0 - SNAP_REMAINING_PER_SECOND.powf(dt.clamp(0.0, 0.05));
                self.cam.heading += remaining * step;
                self.nav_snap_heading = Some(target);
            }
            active = true;
        }

        // ── drawing: the ring, as in the classic navigator, lit while turned ──
        painter.circle_filled(centre, r_ring, ring_bg);
        let ring_edge = if self.ring_drag {
            t.accent
        } else if ring_hot {
            t.text_dim
        } else {
            edge
        };
        painter.circle_stroke(centre, r_ring, Stroke::new(k, ring_edge));
        for (i, &(target, label, _)) in CARDINALS.iter().enumerate() {
            let colour = if letter_hot[i] {
                t.accent
            } else if i == 0 {
                super::compass::NORTH_COLOR
            } else {
                t.text
            };
            let grow = if letter_hot[i] {
                CARDINAL_HOVER_SCALE
            } else {
                1.0
            };
            painter.text(
                letter_at(target),
                Align2::CENTER_CENTER,
                lg.text(label),
                FontId::proportional(super::compass::CARDINAL_FONT_POINTS * k * grow),
                colour,
            );
        }

        // The mini globe in the ring's centre: a disc in the controls' fill with the
        // graticule, the equator and the area in view.
        let globe = MiniGlobe::new(
            centre,
            r_globe,
            self.cam.lat,
            self.cam.lon,
            self.cam.heading,
        );
        painter.circle_filled(centre, r_globe, if globe_hot { t.hover } else { fill });
        let mut lines = Vec::new();
        let grid = Stroke::new(0.8 * k, t.text_dim.gamma_multiply(GRID_OPACITY));
        let samples = |from: f64, to: f64| {
            (0..=GRATICULE_SAMPLES)
                .map(move |i| from + (to - from) * i as f64 / GRATICULE_SAMPLES as f64)
        };
        for lat in (-60_i32..=60)
            .step_by(GRATICULE_STEP_DEG)
            .filter(|&l| l != 0)
        {
            let lat = f64::from(lat);
            globe.polyline(
                samples(-180.0, 180.0).map(|lon| (lat, lon)),
                grid,
                &mut lines,
            );
        }
        for lon in (0_i32..360).step_by(GRATICULE_STEP_DEG) {
            let lon = f64::from(lon);
            globe.polyline(samples(-90.0, 90.0).map(|lat| (lat, lon)), grid, &mut lines);
        }
        globe.polyline(
            samples(-180.0, 180.0).map(|lon| (0.0, lon)),
            Stroke::new(1.1 * k, t.accent.gamma_multiply(EQUATOR_OPACITY)),
            &mut lines,
        );
        painter.extend(lines);
        let aspect = f64::from(rect.width() / rect.height().max(1.0));
        let view_radius = (r_globe * view_cap_fraction(self.cam.alt, aspect) as f32).max(2.0 * k);
        painter.circle_filled(
            centre,
            view_radius,
            t.accent.gamma_multiply(VIEW_FILL_OPACITY),
        );
        painter.circle_stroke(centre, view_radius, Stroke::new(1.2 * k, t.accent));
        painter.circle_stroke(
            centre,
            r_globe,
            Stroke::new(1.0, if globe_hot { t.text_dim } else { edge }),
        );

        // The zoom track, the zoom level along it, and the thumb.
        let (top, bottom) = (Pos2::new(centre.x, t_top), Pos2::new(centre.x, t_bot));
        painter.line_segment([top, bottom], Stroke::new(7.0 * k, ring_bg));
        painter.line_segment([top, bottom], Stroke::new(4.0 * k, fill));
        let hy = t_top + self.cam.zoom_fraction() * track_len;
        painter.line_segment(
            [Pos2::new(centre.x, hy), bottom],
            Stroke::new(2.0 * k, t.accent.gamma_multiply(ZOOM_LEVEL_OPACITY)),
        );
        let thumb = Rect::from_center_size(Pos2::new(centre.x, hy), Vec2::new(20.0 * k, 11.0 * k));
        painter.rect_filled(thumb, 4.0 * k, if track_hot { t.hover } else { fill });
        painter.rect_stroke(
            thumb,
            4.0 * k,
            Stroke::new(1.0, if track_hot { t.accent } else { edge }),
            egui::StrokeKind::Middle,
        );
        painter.line_segment(
            [
                Pos2::new(centre.x - 4.5 * k, hy),
                Pos2::new(centre.x + 4.5 * k, hy),
            ],
            Stroke::new(2.0 * k, ink),
        );
        active
    }
}

fn hold_control(ui: &mut egui::Ui, rect: Rect, id: egui::Id) -> egui::Response {
    // Click-only ownership expires during a long press. Retain drag ownership
    // until release so continuous controls also tolerate small pointer movements.
    ui.interact(rect, id, Sense::click_and_drag())
}

#[cfg(test)]
mod tests;
