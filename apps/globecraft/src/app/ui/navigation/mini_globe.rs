//! The mini globe's geometry: where the Earth appears in the navigator's centre, and
//! how much of it the view shows.
use eframe::egui::{self, Pos2, Stroke, Vec2};
use glam::DVec3;
use globecraft_core::camera::Basis;
use globecraft_core::math::{ll_to_v3, v3_to_ll};

/// Boundary samples per side of the view rectangle for the footprint.
const VIEW_EDGE_SAMPLES: usize = 32;
/// A footprint point this far out (in globe radii) lies on the rim.
const RIM_FRACTION: f32 = 0.999;
/// The step along the rim where the footprint follows it.
const RIM_STEP_RAD: f32 = 0.05;
/// Footprint points closer than this (in globe radii) count as one.
const HULL_EPSILON: f32 = 1e-4;

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
        (p.dot(self.up) > 0.0)
            .then(|| self.to_screen(Vec2::new(p.dot(self.east) as f32, p.dot(self.north) as f32)))
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

    /// The ground area the view shows, as a convex polygon on the mini globe, for
    /// every tilt. The view rectangle's edge is sampled; a ray that misses the Earth
    /// takes the horizon in its direction. Ground on the mini globe's far side is
    /// pushed onto its rim, the outline is the convex hull of the samples, and a
    /// stretch along the rim follows the rim. So it is always convex and stays
    /// inside the mini globe, however the view is tilted or turned.
    pub(super) fn view_outline(&self, basis: &Basis, rect: egui::Rect) -> Vec<Pos2> {
        let corners = [
            rect.left_bottom(),
            rect.right_bottom(),
            rect.right_top(),
            rect.left_top(),
        ];
        let mut points = Vec::with_capacity(4 * VIEW_EDGE_SAMPLES);
        for (i, &from) in corners.iter().enumerate() {
            let to = corners[(i + 1) % corners.len()];
            for step in 0..VIEW_EDGE_SAMPLES {
                let screen = from + (to - from) * (step as f32 / VIEW_EDGE_SAMPLES as f32);
                let ground = basis
                    .pick(rect, screen)
                    .or_else(|| horizon_point(basis, rect, screen));
                if let Some((lat, lon)) = ground
                    && let Some(local) = self.local(ll_to_v3(lat, lon))
                {
                    points.push(local);
                }
            }
        }
        let hull = convex_hull(points);
        let mut outline = Vec::with_capacity(hull.len() * 2);
        for (i, &a) in hull.iter().enumerate() {
            let b = hull[(i + 1) % hull.len()];
            outline.push(self.to_screen(a));
            // Both ends on the rim: the area runs round the far side between them.
            if hull.len() > 2 && a.length() > RIM_FRACTION && b.length() > RIM_FRACTION {
                let (from, to) = (a.angle(), b.angle());
                let turn = (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let steps = (turn.abs() / RIM_STEP_RAD).ceil() as usize;
                for step in 1..steps {
                    let angle = from + turn * step as f32 / steps as f32;
                    outline.push(self.to_screen(Vec2::angled(angle)));
                }
            }
        }
        outline
    }

    /// A point of the Earth (unit vector) on the mini globe; on the far side, on the
    /// rim in its direction.
    pub(super) fn place(&self, p: DVec3) -> Option<Pos2> {
        self.local(p).map(|local| self.to_screen(local))
    }

    /// A point of the Earth (unit vector) on the mini globe, in globe radii with x
    /// east and y north; the far side is pushed onto the rim.
    fn local(&self, p: DVec3) -> Option<Vec2> {
        let local = Vec2::new(p.dot(self.east) as f32, p.dot(self.north) as f32);
        if p.dot(self.up) >= 0.0 {
            Some(local)
        } else {
            (local.length_sq() > 0.0).then(|| local.normalized())
        }
    }

    fn to_screen(&self, local: Vec2) -> Pos2 {
        self.centre + (self.east_screen * local.x + self.north_screen * local.y) * self.radius
    }
}

/// The convex hull of `points` (Andrew's monotone chain), counter-clockwise, without
/// repeated or collinear points.
fn convex_hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| (*a - *b).length_sq() < HULL_EPSILON * HULL_EPSILON);
    if points.len() < 3 {
        return points;
    }
    let mut hull: Vec<Vec2> = Vec::with_capacity(points.len() + 1);
    half_hull(&mut hull, points.iter().copied());
    half_hull(&mut hull, points.iter().rev().copied());
    hull
}

/// Adds one chain of the monotone-chain hull (lower or upper) to `hull`, without its
/// last point, which starts the other chain.
fn half_hull(hull: &mut Vec<Vec2>, points: impl Iterator<Item = Vec2>) {
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).x * (b - o).y - (a - o).y * (b - o).x;
    let start = hull.len();
    for p in points {
        while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
        {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
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
