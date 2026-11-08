//! The mini globe's geometry: where the Earth appears in the navigator's centre, and
//! how much of it the view shows.
use eframe::egui::{self, Pos2, Stroke, Vec2};
use glam::DVec3;
use globecraft_core::camera::Basis;
use globecraft_core::math::{ll_to_v3, v3_to_ll};

/// Boundary samples per side of the view rectangle for the tilted footprint.
const VIEW_EDGE_SAMPLES: usize = 12;

/// Where the mini globe puts the Earth: centred on the camera's ground point, turned
/// by its heading, seen from far away (orthographic).
pub(super) struct MiniGlobe {
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
    pub(super) fn new(centre: Pos2, radius: f32, lat: f64, lon: f64, heading: f64) -> Self {
        let up = ll_to_v3(lat, lon);
        let north = (ll_to_v3(lat + 0.01, lon) - ll_to_v3(lat - 0.01, lon)).normalize();
        let east = ll_to_v3(lat, lon + 0.01) - ll_to_v3(lat, lon - 0.01);
        let east = (east - north * east.dot(north)).normalize();
        let (north_screen, east_screen) = screen_axes(heading);
        Self {
            centre,
            radius,
            up,
            north,
            east,
            north_screen,
            east_screen,
        }
    }

    /// The screen position of a point on the Earth, if it faces the viewer.
    pub(super) fn project(&self, lat: f64, lon: f64) -> Option<Pos2> {
        let p = ll_to_v3(lat, lon);
        (p.dot(self.up) > 0.0).then(|| {
            let (x, y) = (p.dot(self.east) as f32, p.dot(self.north) as f32);
            self.centre + (self.east_screen * x + self.north_screen * y) * self.radius
        })
    }

    /// A line of latitude or longitude, cut where it turns to the far side.
    pub(super) fn polyline(
        &self,
        points: impl Iterator<Item = (f64, f64)>,
        stroke: Stroke,
        out: &mut Vec<egui::Shape>,
    ) {
        let mut run: Vec<Pos2> = Vec::new();
        for (lat, lon) in points {
            match self.project(lat, lon) {
                Some(p) => run.push(p),
                None if run.len() >= 2 => {
                    for part in crate::app::live_layers::stroke_runs(&std::mem::take(&mut run)) {
                        out.push(egui::Shape::line(part, stroke));
                    }
                }
                None => run.clear(),
            }
        }
        for part in crate::app::live_layers::stroke_runs(&run) {
            out.push(egui::Shape::line(part, stroke));
        }
    }

    /// The ground area the view shows, as a closed polygon on the mini globe. A
    /// tilted view reaches forward to the horizon, so its boundary follows the
    /// horizon where the view looks past it; a top-down view keeps the caller's
    /// circle instead.
    pub(super) fn view_outline(&self, basis: &Basis, rect: egui::Rect) -> Vec<Pos2> {
        let mut points = Vec::with_capacity(4 * VIEW_EDGE_SAMPLES);
        let edges = [
            (rect.left_bottom(), rect.right_bottom()),
            (rect.right_bottom(), rect.right_top()),
            (rect.right_top(), rect.left_top()),
            (rect.left_top(), rect.left_bottom()),
        ];
        for (from, to) in edges {
            for i in 0..VIEW_EDGE_SAMPLES {
                let t = i as f32 / VIEW_EDGE_SAMPLES as f32;
                let screen = from + (to - from) * t;
                let ground = basis
                    .pick(rect, screen)
                    .or_else(|| horizon_point(basis, rect, screen));
                if let Some((lat, lon)) = ground
                    && let Some(point) = self.project(lat, lon)
                    && points.last() != Some(&point)
                {
                    points.push(point);
                }
            }
        }
        if points.first() == points.last() {
            points.pop();
        }
        points
    }
}

/// The horizon point in the direction of a view ray that misses the Earth: the
/// tangent point on the camera's horizon circle at the ray's azimuth. It works for
/// rays above the camera's horizon too, where the closest approach lies behind the
/// camera, and keeps the footprint on the horizon circle rather than the limb.
pub(super) fn horizon_point(basis: &Basis, rect: egui::Rect, screen: Pos2) -> Option<(f64, f64)> {
    let d = basis.ray(rect, screen);
    let up = basis.c.normalize();
    let horizontal = d - up * d.dot(up);
    let azimuth = horizontal.try_normalize()?;
    let distance = basis.c.length();
    if distance <= 1.0 {
        return None;
    }
    let cos_horizon = 1.0 / distance;
    let sin_horizon = (1.0 - cos_horizon * cos_horizon).sqrt();
    Some(v3_to_ll(up * cos_horizon + azimuth * sin_horizon))
}

/// Screen directions of north and east for a heading (degrees clockwise from north).
pub(super) fn screen_axes(heading: f64) -> (Vec2, Vec2) {
    let h = heading.to_radians() as f32;
    (Vec2::new(-h.sin(), -h.cos()), Vec2::new(h.cos(), -h.sin()))
}

/// The sine of the angular radius of the Earth cap the view shows across its shorter
/// side: that cap's radius on the orthographic mini globe, as a fraction of it.
pub(super) fn view_cap_fraction(altitude_km: f64, aspect: f64) -> f64 {
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
pub(super) fn nearest_heading(heading: f64, target: f64) -> f64 {
    heading + (target - heading + 540.0).rem_euclid(360.0) - 180.0
}
