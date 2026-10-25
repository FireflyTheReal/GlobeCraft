//! The navigator (roadmap 13.5), after the user's Fusion 360-style mockup: a small
//! panel on the right of the globe with
//! - a mini globe that turns with the camera; dragging it turns the Earth under the
//!   camera, and a blue disc marks how much of it the view shows;
//! - a ring around it with the four cardinal buttons: dragging the ring rotates the
//!   view, a button turns that direction up;
//! - a logarithmic zoom slider (zoom in at the top) that also takes the scroll wheel.
use super::super::App;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use glam::DVec3;
use globecraft_core::math::{ll_to_v3, wrap_lon};

const MIN_CONTROL_SCALE: f32 = 0.8;
const MAX_CONTROL_SCALE: f32 = 1.8;
const PANEL_MARGIN_POINTS: f32 = 16.0;
const PANEL_CORNER_POINTS: f32 = 8.0;
const HEAD_HEIGHT_POINTS: f32 = 30.0;
const HEAD_PADDING_POINTS: f32 = 11.0;
const BODY_PADDING_X_POINTS: f32 = 12.0;
const BODY_PADDING_TOP_POINTS: f32 = 11.0;
const BODY_PADDING_BOTTOM_POINTS: f32 = 12.0;
/// The ring's outer diameter.
const SPHERE_SIZE_POINTS: f32 = 160.0;
const RING_WIDTH_POINTS: f32 = 4.0;
/// From the ring's outer edge to the mini globe.
const GLOBE_INSET_POINTS: f32 = 20.0;
/// How far inside the ring's outer edge a press grabs the ring rather than the globe.
const RING_GRAB_POINTS: f32 = 32.0;
const CARDINAL_SIZE_POINTS: f32 = 21.0;
const CARDINAL_CORNER_POINTS: f32 = 5.0;
const CARDINAL_HOVER_SCALE: f32 = 1.12;
const ZOOM_GAP_POINTS: f32 = 28.0;
/// The slider's track is half the sphere's diameter.
const ZOOM_LENGTH_RATIO: f32 = 0.5;
const ZOOM_TRACK_WIDTH_POINTS: f32 = 4.0;
const ZOOM_THUMB_POINTS: f32 = 13.0;
const ZOOM_ICON_POINTS: f32 = 12.0;
const ZOOM_ICON_GAP_POINTS: f32 = 9.0;
/// One wheel notch (about 50 points of scroll) zooms by 10 %.
const WHEEL_ZOOM_PER_POINT: f64 = 0.1 / 50.0;
/// Heading snaps ease in: the remaining turn shrinks to this fraction each second.
const SNAP_REMAINING_PER_SECOND: f64 = 0.0001;
const SNAP_DONE_DEG: f64 = 0.05;
const GRATICULE_STEP_DEG: i32 = 30;
const GRATICULE_SAMPLES: usize = 64;
const SPHERE_RINGS: usize = 16;
const SPHERE_SEGMENTS: usize = 64;

// Mockup colours that are not interface tokens: the globe and the view disc.
const GLOBE_BASE: [f32; 3] = [0x93 as f32, 0xb8 as f32, 0xcd as f32];
const GLOBE_FILL_LIGHT: [f32; 3] = [0x8f as f32, 0xb4 as f32, 0xcc as f32];
const GRATICULE: Color32 = Color32::from_rgba_unmultiplied_const(0x2f, 0x3b, 0x46, 82);
const HALO: Color32 = Color32::from_rgba_unmultiplied_const(0xbf, 0xe0, 0xf5, 23);
const VIEW_FILL: Color32 = Color32::from_rgba_unmultiplied_const(0x3f, 0x7f, 0xe0, 46);
const VIEW_RING: Color32 = Color32::from_rgba_unmultiplied_const(0x2f, 0x6f, 0xd0, 230);
const VIEW_RING_POINTS: f32 = 3.0;
const THUMB_FILL: Color32 = Color32::from_rgb(0xe6, 0xeb, 0xf2);

/// The four cardinal buttons: heading that turns them up, label, tooltip.
const CARDINALS: [(f64, &str, &str); 4] = [
    (0.0, "compass-north", "navigation-north-tooltip"),
    (90.0, "compass-east", "navigation-east-tooltip"),
    (180.0, "compass-south", "navigation-south-tooltip"),
    (270.0, "compass-west", "navigation-west-tooltip"),
];

/// Where the mini globe puts the Earth: centred on the camera's ground point, turned
/// by its heading. Orthographic, like the far side of a small sphere.
struct MiniGlobe {
    centre: Pos2,
    radius: f32,
    /// Unit vectors at the camera's ground point.
    up: DVec3,
    north: DVec3,
    east: DVec3,
    /// Screen directions of north and east.
    north_screen: Vec2,
    east_screen: Vec2,
}

impl MiniGlobe {
    fn new(centre: Pos2, radius: f32, lat: f64, lon: f64, heading: f64) -> Self {
        let up = ll_to_v3(lat, lon);
        let north = (ll_to_v3(lat + 0.01, lon) - ll_to_v3(lat - 0.01, lon)).normalize();
        let east = ll_to_v3(lat, lon + 0.01) - ll_to_v3(lat, lon - 0.01);
        let east = (east - north * east.dot(north)).normalize();
        let h = heading.to_radians() as f32;
        Self {
            centre,
            radius,
            up,
            north,
            east,
            north_screen: Vec2::new(-h.sin(), -h.cos()),
            east_screen: Vec2::new(h.cos(), -h.sin()),
        }
    }
    /// The screen position of a point on the Earth, if it faces the viewer.
    fn project(&self, lat: f64, lon: f64) -> Option<Pos2> {
        let p = ll_to_v3(lat, lon);
        (p.dot(self.up) > 0.0).then(|| {
            let (x, y) = (p.dot(self.east) as f32, p.dot(self.north) as f32);
            self.centre + (self.east_screen * x + self.north_screen * y) * self.radius
        })
    }
    /// A line of latitude or longitude, cut where it turns to the far side.
    fn polyline(
        &self,
        points: impl Iterator<Item = (f64, f64)>,
        stroke: Stroke,
    ) -> Vec<egui::Shape> {
        let mut shapes = Vec::new();
        let mut run: Vec<Pos2> = Vec::new();
        for (lat, lon) in points {
            match self.project(lat, lon) {
                Some(p) => run.push(p),
                None if run.len() >= 2 => {
                    shapes.push(egui::Shape::line(std::mem::take(&mut run), stroke))
                }
                None => run.clear(),
            }
        }
        if run.len() >= 2 {
            shapes.push(egui::Shape::line(run, stroke));
        }
        shapes
    }
}

/// The lit sphere: a disc of vertices shaded by a key light from the upper left and a
/// weak fill light from behind, like the mockup's three.js lights.
fn sphere_mesh(centre: Pos2, radius: f32) -> egui::Mesh {
    let key = DVec3::new(3.0, 4.0, 5.0).normalize();
    let fill = DVec3::new(-4.0, -2.0, -3.0).normalize();
    let shade = |x: f64, y_up: f64| {
        let z = (1.0 - x * x - y_up * y_up).max(0.0).sqrt();
        let normal = DVec3::new(x, y_up, z);
        let k = 0.62 + 0.5 * normal.dot(key).max(0.0) as f32;
        let f = 0.12 * normal.dot(fill).max(0.0) as f32;
        let channel =
            |i: usize| (GLOBE_BASE[i] * k + GLOBE_FILL_LIGHT[i] * f).clamp(0.0, 255.0) as u8;
        Color32::from_rgb(channel(0), channel(1), channel(2))
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, shade(0.0, 0.0));
    for ring in 1..=SPHERE_RINGS {
        let r = ring as f64 / SPHERE_RINGS as f64;
        for segment in 0..SPHERE_SEGMENTS {
            let a = segment as f64 / SPHERE_SEGMENTS as f64 * std::f64::consts::TAU;
            let (x, y) = (r * a.cos(), r * a.sin());
            mesh.colored_vertex(
                centre + Vec2::new(x as f32, y as f32) * radius,
                shade(x, -y),
            );
        }
    }
    let index = |ring: usize, segment: usize| {
        (1 + (ring - 1) * SPHERE_SEGMENTS + segment % SPHERE_SEGMENTS) as u32
    };
    for segment in 0..SPHERE_SEGMENTS {
        mesh.add_triangle(0, index(1, segment), index(1, segment + 1));
    }
    for ring in 1..SPHERE_RINGS {
        for segment in 0..SPHERE_SEGMENTS {
            let (a, b) = (index(ring, segment), index(ring, segment + 1));
            let (c, d) = (index(ring + 1, segment), index(ring + 1, segment + 1));
            mesh.add_triangle(a, c, d);
            mesh.add_triangle(a, d, b);
        }
    }
    mesh
}

/// The sine of the angular radius of the Earth cap the view shows across its shorter
/// side: the radius of that cap on the orthographic mini globe, as a fraction of it.
fn view_cap_fraction(altitude_km: f64, aspect: f64) -> f64 {
    let radius = globecraft_core::EARTH_RADIUS_KM;
    let distance = radius + altitude_km.max(0.0);
    let half_short = (globecraft_core::VERTICAL_FOV_HALF_TAN * aspect.min(1.0)).atan();
    let sin_horizon = radius / distance;
    let sin_edge = half_short.sin();
    if sin_edge >= sin_horizon {
        // The short side looks past the Earth: the whole visible side is in view.
        return sin_horizon.acos().sin();
    }
    // Where the edge ray meets the Earth; the law of sines gives the angle at the centre.
    let near = distance * half_short.cos()
        - (radius * radius - distance * distance * sin_edge * sin_edge).sqrt();
    (near * sin_edge / radius).clamp(0.0, 1.0)
}

/// The heading that turns `target` up, the short way round from `heading`.
fn nearest_heading(heading: f64, target: f64) -> f64 {
    heading + (target - heading + 540.0).rem_euclid(360.0) - 180.0
}

impl App {
    /// Draws the navigator and applies its input. Returns true while the user is
    /// operating it (or a heading snap is still turning).
    pub(in crate::app) fn nav_controls(&mut self, ui: &mut egui::Ui, rect: Rect, dt: f64) -> bool {
        if self.settings.hide_navigation_controls {
            self.ring_drag = false;
            self.nav_snap_heading = None;
            return false;
        }
        let lg = self.lang;
        let k = self
            .settings
            .ui_scale
            .clamp(MIN_CONTROL_SCALE, MAX_CONTROL_SCALE);
        let t = super::theme::Tokens::get(ui.ctx());
        let painter = ui.painter_at(rect);
        let base = ui.id().with("navigator");
        let mut active = false;

        // ── layout ──
        let zoom_length = SPHERE_SIZE_POINTS * ZOOM_LENGTH_RATIO * k;
        let size = Vec2::new(
            (SPHERE_SIZE_POINTS + 2.0 * BODY_PADDING_X_POINTS) * k,
            (HEAD_HEIGHT_POINTS
                + BODY_PADDING_TOP_POINTS
                + SPHERE_SIZE_POINTS
                + ZOOM_GAP_POINTS
                + BODY_PADDING_BOTTOM_POINTS)
                * k
                + zoom_length,
        );
        let top = (rect.center().y - size.y / 2.0).max(rect.top() + PANEL_MARGIN_POINTS * k);
        let panel = Rect::from_min_size(
            Pos2::new(rect.right() - PANEL_MARGIN_POINTS * k - size.x, top),
            size,
        );
        let head = Rect::from_min_max(
            panel.min,
            Pos2::new(panel.right(), panel.top() + HEAD_HEIGHT_POINTS * k),
        );
        let outer = SPHERE_SIZE_POINTS / 2.0 * k;
        let centre = Pos2::new(
            panel.center().x,
            head.bottom() + BODY_PADDING_TOP_POINTS * k + outer,
        );
        let globe_radius = outer - GLOBE_INSET_POINTS * k;
        let track = Rect::from_center_size(
            Pos2::new(
                centre.x,
                centre.y + outer + ZOOM_GAP_POINTS * k + zoom_length / 2.0,
            ),
            Vec2::new(ZOOM_TRACK_WIDTH_POINTS * k, zoom_length),
        );

        // ── input ──
        // The panel takes the pointer, so the globe below neither drags nor zooms.
        let backdrop = ui.interact(panel, base.with("panel"), Sense::click_and_drag());
        let sphere = ui.interact(
            Rect::from_center_size(centre, Vec2::splat(2.0 * outer + 20.0 * k)),
            base.with("sphere"),
            Sense::drag(),
        );
        let in_ring = |p: Pos2| {
            let d = (p - centre).length();
            d >= outer - RING_GRAB_POINTS * k && d <= outer + 10.0 * k
        };
        let ring_hovered = !sphere.dragged() && sphere.hover_pos().is_some_and(in_ring);
        if sphere.drag_started() {
            self.ring_drag = sphere.interact_pointer_pos().is_some_and(in_ring);
            self.nav_snap_heading = None;
        }
        if sphere.dragged()
            && let Some(cur) = sphere.interact_pointer_pos()
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
                // Grab the mini globe's surface: the camera moves the other way.
                let h = self.cam.heading.to_radians();
                let (north, east) = (
                    Vec2::new(-h.sin() as f32, -h.cos() as f32),
                    Vec2::new(h.cos() as f32, -h.sin() as f32),
                );
                let deg_per_point = f64::from(57.295_78 / globe_radius.max(1.0));
                let moved = -delta;
                let lat = self.cam.lat + f64::from(moved.dot(north)) * deg_per_point;
                let lat = lat.clamp(
                    -globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                    globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                );
                let coslat = self.cam.lat.to_radians().cos().max(0.05);
                self.cam.lon =
                    wrap_lon(self.cam.lon + f64::from(moved.dot(east)) * deg_per_point / coslat);
                self.cam.lat = lat;
            }
        }
        if sphere.drag_stopped() {
            self.ring_drag = false;
        }
        sphere.on_hover_text(lg.text(if self.ring_drag || ring_hovered {
            "navigation-ring-tooltip"
        } else {
            "navigation-globe-tooltip"
        }));

        // Cardinal buttons ride on the ring and turn with the view.
        let heading = self.cam.heading;
        let mut cardinal_hovered = [false; 4];
        for (i, &(target, _, tooltip)) in CARDINALS.iter().enumerate() {
            let angle = (target - heading).to_radians();
            let at = centre + Vec2::new(angle.sin() as f32, -angle.cos() as f32) * (outer - k);
            let response = ui.interact(
                Rect::from_center_size(at, Vec2::splat(CARDINAL_SIZE_POINTS * k)),
                base.with(("cardinal", i)),
                Sense::click(),
            );
            cardinal_hovered[i] = response.hovered();
            if response.on_hover_text(lg.text(tooltip)).clicked() {
                self.nav_snap_heading = Some(nearest_heading(self.cam.heading, target));
                self.fly = None;
                active = true;
            }
        }

        // Zoom: drag or click along the track, or scroll over it.
        let track_area = track.expand2(Vec2::new(10.0 * k, 6.0 * k));
        let zoom = hold_control(ui, track_area, base.with("zoom"));
        if (zoom.dragged() || zoom.is_pointer_button_down_on())
            && let Some(p) = zoom.interact_pointer_pos()
        {
            let fraction = ((p.y - track.top()) / track.height()).clamp(0.0, 1.0);
            self.cam.set_zoom_fraction(f64::from(fraction));
            self.fly = None;
            active = true;
        }
        if zoom.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom((-f64::from(scroll) * WHEEL_ZOOM_PER_POINT).exp());
                self.fly = None;
                active = true;
            }
        }
        let zoom_hot = zoom.hovered() || zoom.dragged();
        zoom.on_hover_text(lg.text("navigation-zoom-slider-tooltip"));
        let _ = backdrop;

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

        // ── drawing ──
        let corner = (PANEL_CORNER_POINTS * k).round() as u8;
        painter.rect_filled(panel, corner, t.pane);
        painter.rect_filled(
            head,
            egui::CornerRadius {
                nw: corner,
                ne: corner,
                sw: 0,
                se: 0,
            },
            t.pane_header,
        );
        painter.line_segment(
            [head.left_bottom(), head.right_bottom()],
            Stroke::new(1.0, t.border),
        );
        painter.rect_stroke(
            panel,
            corner,
            Stroke::new(1.0, t.border_strong),
            egui::StrokeKind::Inside,
        );
        painter.text(
            Pos2::new(head.left() + HEAD_PADDING_POINTS * k, head.center().y),
            Align2::LEFT_CENTER,
            lg.text("navigation-section-title"),
            FontId::proportional(12.0 * k),
            t.text,
        );
        painter.text(
            Pos2::new(head.right() - HEAD_PADDING_POINTS * k, head.center().y),
            Align2::RIGHT_CENTER,
            self.distance_units().altitude(self.cam.alt),
            FontId::monospace(10.5 * k),
            t.text_dim,
        );

        // The mini globe: halo, lit sphere, graticule, equator, view disc.
        let globe = MiniGlobe::new(
            centre,
            globe_radius,
            self.cam.lat,
            self.cam.lon,
            self.cam.heading,
        );
        painter.circle_filled(centre, globe_radius * 1.1, HALO);
        painter.add(egui::Shape::mesh(sphere_mesh(centre, globe_radius)));
        let graticule = Stroke::new(k, GRATICULE);
        let samples = |from: f64, to: f64| {
            (0..=GRATICULE_SAMPLES)
                .map(move |i| from + (to - from) * i as f64 / GRATICULE_SAMPLES as f64)
        };
        for lat in (-60..=60)
            .step_by(GRATICULE_STEP_DEG as usize)
            .filter(|&l| l != 0)
        {
            let lat = f64::from(lat);
            painter.extend(globe.polyline(samples(-180.0, 180.0).map(|lon| (lat, lon)), graticule));
        }
        for lon in (0..360).step_by(GRATICULE_STEP_DEG as usize) {
            let lon = f64::from(lon);
            painter.extend(globe.polyline(samples(-90.0, 90.0).map(|lat| (lat, lon)), graticule));
        }
        painter.extend(globe.polyline(
            samples(-180.0, 180.0).map(|lon| (0.0, lon)),
            Stroke::new(1.4 * k, t.accent.gamma_multiply(0.85)),
        ));
        let aspect = f64::from(rect.width() / rect.height().max(1.0));
        let seen = view_cap_fraction(self.cam.alt, aspect) as f32;
        let wash = (seen * seen * 0.14 * 255.0) as u8;
        painter.circle_filled(
            centre,
            globe_radius,
            Color32::from_rgba_unmultiplied(0x3f, 0x7f, 0xe0, wash),
        );
        let view_radius = (globe_radius * seen).max(1.5);
        painter.circle_filled(centre, view_radius, VIEW_FILL);
        painter.circle_stroke(
            centre,
            (view_radius - VIEW_RING_POINTS / 2.0).max(0.5),
            Stroke::new(VIEW_RING_POINTS, VIEW_RING),
        );
        painter.circle_stroke(centre, globe_radius, Stroke::new(1.0, t.border));

        // The ring.
        let ring_colour = if self.ring_drag {
            t.accent
        } else if ring_hovered {
            t.text_dim
        } else {
            t.border_strong
        };
        painter.circle_stroke(
            centre,
            outer - RING_WIDTH_POINTS / 2.0 * k,
            Stroke::new(RING_WIDTH_POINTS * k, ring_colour),
        );
        for (i, &(target, label, _)) in CARDINALS.iter().enumerate() {
            let angle = (target - heading).to_radians();
            let at = centre + Vec2::new(angle.sin() as f32, -angle.cos() as f32) * (outer - k);
            let grow = if cardinal_hovered[i] {
                CARDINAL_HOVER_SCALE
            } else {
                1.0
            };
            let button = Rect::from_center_size(at, Vec2::splat(CARDINAL_SIZE_POINTS * k * grow));
            painter.rect_filled(
                button,
                (CARDINAL_CORNER_POINTS * k).round() as u8,
                if cardinal_hovered[i] {
                    t.hover
                } else {
                    t.input
                },
            );
            painter.rect_stroke(
                button,
                (CARDINAL_CORNER_POINTS * k).round() as u8,
                Stroke::new(
                    1.0,
                    if cardinal_hovered[i] {
                        t.border_strong
                    } else {
                        t.border
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let ink = match (i, cardinal_hovered[i]) {
                (0, _) => t.accent,
                (_, true) => t.text,
                _ => t.text_dim,
            };
            painter.text(
                at,
                Align2::CENTER_CENTER,
                lg.text(label),
                FontId::proportional(9.5 * k * grow),
                ink,
            );
        }

        // The zoom slider: zoom in at the top, filled from the bottom up to the thumb.
        let fraction = self.cam.zoom_fraction();
        let thumb_y = track.top() + fraction * track.height();
        let track_corner = (3.0 * k).round() as u8;
        painter.rect_filled(track, track_corner, t.input);
        painter.rect_stroke(
            track,
            track_corner,
            Stroke::new(1.0, t.border_strong),
            egui::StrokeKind::Outside,
        );
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(track.left(), thumb_y), track.max),
            track_corner,
            t.accent,
        );
        let thumb = Rect::from_center_size(
            Pos2::new(track.center().x, thumb_y),
            Vec2::splat(ZOOM_THUMB_POINTS * k),
        );
        painter.rect_filled(
            thumb.translate(Vec2::new(0.0, k)),
            track_corner,
            Color32::from_black_alpha(90),
        );
        painter.rect_filled(thumb, track_corner, THUMB_FILL);
        painter.rect_stroke(
            thumb,
            track_corner,
            Stroke::new(
                2.0 * k,
                if zoom_hot {
                    t.accent.gamma_multiply(1.15)
                } else {
                    t.accent
                },
            ),
            egui::StrokeKind::Inside,
        );
        let icon_ink = if zoom_hot {
            t.text_dim
        } else {
            t.text_disabled
        };
        let icon_offset =
            (ZOOM_TRACK_WIDTH_POINTS / 2.0 + ZOOM_ICON_GAP_POINTS + ZOOM_ICON_POINTS / 2.0) * k;
        let icon = ZOOM_ICON_POINTS * k;
        magnifier(
            &painter,
            Pos2::new(track.center().x - icon_offset, track.top() + icon / 2.0),
            icon,
            true,
            icon_ink,
        );
        magnifier(
            &painter,
            Pos2::new(track.center().x + icon_offset, track.bottom() - icon / 2.0),
            icon,
            false,
            icon_ink,
        );
        active
    }
}

/// The mockup's zoom icons: a lens with a plus (in) or minus (out), on a 24-unit grid.
fn magnifier(painter: &egui::Painter, centre: Pos2, size: f32, plus: bool, ink: Color32) {
    let unit = size / 24.0;
    let at = |x: f32, y: f32| centre + Vec2::new((x - 12.0) * unit, (y - 12.0) * unit);
    let stroke = Stroke::new(1.8 * unit.max(0.6), ink);
    painter.circle_stroke(at(10.5, 10.5), 6.5 * unit, stroke);
    painter.line_segment([at(15.3, 15.3), at(20.0, 20.0)], stroke);
    painter.line_segment([at(7.5, 10.5), at(13.5, 10.5)], stroke);
    if plus {
        painter.line_segment([at(10.5, 7.5), at(10.5, 13.5)], stroke);
    }
}

fn hold_control(ui: &mut egui::Ui, rect: Rect, id: egui::Id) -> egui::Response {
    // Click-only ownership expires during a long press. Retain drag ownership
    // until release so continuous controls also tolerate small pointer movements.
    ui.interact(rect, id, Sense::click_and_drag())
}

#[cfg(test)]
mod tests;
