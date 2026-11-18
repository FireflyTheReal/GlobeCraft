use crate::earth_model::EarthModel;
use crate::math::{ll_to_v3, v3_to_ll, wrap_lon};
use crate::{
    EARTH_RADIUS_KM, MAX_CAMERA_ALTITUDE_KM, MIN_CAMERA_ALTITUDE_KM, VERTICAL_FOV_HALF_TAN,
};
use crate::{MAX_CAMERA_LATITUDE_DEG, METERS_PER_KM, METERS_PER_LATITUDE_DEGREE};
use emath::{Pos2, Rect};
use glam::{DMat2, DMat3, DMat4, DQuat, DVec2, DVec3, dcamera::rh};
mod look;
// Reject near-zero projection depths and rotation axes; bound pan amplification near poles.
const PROJECTION_DEPTH_EPSILON: f64 = 1e-9;
const ROTATION_AXIS_EPSILON: f64 = 1e-13;
const MIN_PAN_LATITUDE_COSINE: f64 = 0.05;
/// Pans that start or end closer to a pole than this follow great circles, and
/// from here to `POLE_HELD_LATITUDE_DEG` north stays up less and less.
const POLE_PAN_LATITUDE_DEG: f64 = 60.0;
/// From this latitude on the globe moves as held in the hand: north turns with it.
const POLE_HELD_LATITUDE_DEG: f64 = 85.0;
// The CPU renderer uses Earth radii. Projection depth is not used for clipping.
const PROJECTION_NEAR_EARTH_RADII: f64 = MIN_CAMERA_ALTITUDE_KM / EARTH_RADIUS_KM;

// ───────────────────────────── Camera ─────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Cam {
    /// The ground point at the centre of the view.
    pub lat: f64,
    pub lon: f64,
    pub heading: f64, // degrees: bearing the top of the screen points to
    /// km from the ground point to the camera (its altitude when looking straight down).
    pub alt: f64,
    /// Degrees the view leans from straight down toward the horizon (0 = top-down).
    pub tilt: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Basis {
    pub c: DVec3, // camera position (mean Earth radii)
    pub model: EarthModel,
    /// How far the view leans from straight down, in degrees. Overlays use it to
    /// skip terrain line-of-sight tests that cannot hide anything.
    pub tilt_deg: f64,
    center: (f64, f64),
    view_from_world: DMat3,
    world_from_view: DMat3,
    projection: DMat4,
    aspect: f64,
}

impl Cam {
    pub fn basis(&self, aspect: f64) -> Basis {
        self.basis_for_model(aspect, EarthModel::PerfectSphere)
    }
    pub fn basis_for_model(&self, aspect: f64, model: EarthModel) -> Basis {
        self.basis_over_ground(aspect, model, 0.0)
    }
    /// The view looking at the ground point raised `ground_km` above the surface (the
    /// terrain height there), from `alt` away: with relief the camera orbits the
    /// mountain it looks at instead of a point inside it.
    pub fn basis_over_ground(&self, aspect: f64, model: EarthModel, ground_km: f64) -> Basis {
        let n = ll_to_v3(self.lat, self.lon);
        let (la, lo) = (self.lat.to_radians(), self.lon.to_radians());
        let east = DVec3::new(-lo.sin(), lo.cos(), 0.0);
        let north = DVec3::new(-la.sin() * lo.cos(), -la.sin() * lo.sin(), la.cos());
        let h = self.heading.to_radians();
        let u = north * h.cos() + east * h.sin();
        // Tilt swings the camera back from the heading around the ground point it looks
        // at; the top of the screen stays in the heading's vertical plane.
        let t = self.tilt.clamp(0.0, MAX_TILT_DEG).to_radians();
        let back = n * t.cos() - u * t.sin();
        let f = -back;
        let up = u * t.cos() + n * t.sin();
        // Geographic world axes are right-handed. View space is X-right/Y-up/-Z-forward.
        let view_from_world = rh::view::look_to_mat3(f, up);
        let target = if model == EarthModel::PerfectSphere {
            n
        } else {
            model.geometry().position(self.lat, self.lon)
        };
        let target = target + n * (ground_km / EARTH_RADIUS_KM);
        Basis {
            c: target + back * (self.alt / EARTH_RADIUS_KM),
            model,
            tilt_deg: t.to_degrees(),
            center: (self.lat, self.lon),
            view_from_world,
            world_from_view: view_from_world.transpose(),
            // Y-up NDC matches our screen conversion. No far plane clips the globe.
            projection: rh::proj::directx::perspective_infinite(
                2.0 * VERTICAL_FOV_HALF_TAN.atan(),
                aspect,
                PROJECTION_NEAR_EARTH_RADII,
            ),
            aspect,
        }
    }
}

impl Basis {
    /// The ground point at the centre of the view.
    pub fn geographic_center(&self) -> (f64, f64) {
        self.center
    }
    #[inline]
    pub fn surface_position(&self, lat: f64, lon: f64) -> DVec3 {
        self.model.geometry().position(lat, lon)
    }

    /// Cached view-to-world rotation for batched CPU ray construction.
    pub fn rotation_from_view(&self) -> DMat3 {
        self.world_from_view
    }
    pub fn aspect_ratio(&self) -> f64 {
        self.aspect
    }
    pub fn dir(&self, nx: f64, ny: f64) -> DVec3 {
        let view_ray = DVec3::new(
            nx * self.aspect * VERTICAL_FOV_HALF_TAN,
            ny * VERTICAL_FOV_HALF_TAN,
            -1.0,
        );
        (self.world_from_view * view_ray).normalize()
    }
    pub fn project(&self, p: DVec3, rect: Rect) -> Option<Pos2> {
        self.project_precise(p, rect)
            .map(|at| Pos2::new(at.x as f32, at.y as f32))
    }
    /// [`Self::project`] in f64 screen points. A tilted close-up projects points
    /// beside or under the camera millions of points off screen, where f32 steps are
    /// tens of points: lines clipped from such endpoints jittered as the camera moved
    /// (roadmap 1.9). Clip in f64, then convert.
    pub fn project_precise(&self, p: DVec3, rect: Rect) -> Option<DVec2> {
        let horizon_camera = if self.model == EarthModel::PerfectSphere {
            self.c
        } else {
            self.c * self.model.geometry().inverse_axes_squared()
        };
        if p.dot(horizon_camera) < 1.0 {
            return None; // behind the horizon
        }
        self.screen_point(p, rect)
    }
    /// A point above the ground (an aircraft or a satellite, in mean Earth radii from
    /// the centre), in screen points; `None` when the Earth (this basis's model) stands
    /// between it and the camera or it is behind the camera.
    pub fn project_above(&self, p: DVec3, rect: Rect) -> Option<Pos2> {
        /// A point this close behind where its ray meets the ground is on it.
        const ON_THE_GROUND_RADII: f64 = 1e-6;
        let to = p - self.c;
        let length = to.length();
        if length <= 0.0 {
            return None;
        }
        let direction = to / length;
        if self
            .model
            .geometry()
            .intersect(self.c, direction)
            .is_some_and(|hit| (hit - self.c).length() < length - ON_THE_GROUND_RADII)
        {
            return None; // the Earth is in front of it
        }
        self.screen_point(p, rect)
            .map(|at| Pos2::new(at.x as f32, at.y as f32))
    }
    /// A point `km` above the ground at a latitude and longitude, in mean Earth radii.
    pub fn raised_position(&self, lat: f64, lon: f64, km: f64) -> DVec3 {
        let ground = self.surface_position(lat, lon);
        ground + ground.normalize() * (km / crate::EARTH_RADIUS_KM)
    }
    /// Any point in front of the camera, in f64 screen points, hidden or not.
    /// How far `p` lies in front of the camera along its view axis (mean Earth
    /// radii): the depth a depth buffer orders by.
    pub fn view_depth(&self, p: DVec3) -> f64 {
        -(self.view_from_world * (p - self.c)).z
    }
    fn screen_point(&self, p: DVec3, rect: Rect) -> Option<DVec2> {
        let v = p - self.c;
        let local = self.view_from_world * v;
        let z = -local.z;
        if z <= PROJECTION_DEPTH_EPSILON {
            return None;
        }
        let ndc = self.projection.project_point3(local);
        let centre = rect.center();
        Some(DVec2::new(
            f64::from(centre.x) + ndc.x * f64::from(rect.width()) * 0.5,
            f64::from(centre.y) - ndc.y * f64::from(rect.height()) * 0.5,
        ))
    }
    /// The view ray through screen point `pos`: its unit direction from [`Self::c`].
    pub fn ray(&self, rect: Rect, pos: Pos2) -> DVec3 {
        let nx = ((pos.x - rect.min.x) / rect.width()) as f64 * 2.0 - 1.0;
        let ny = 1.0 - ((pos.y - rect.min.y) / rect.height()) as f64 * 2.0;
        self.dir(nx, ny)
    }
    pub fn pick(&self, rect: Rect, pos: Pos2) -> Option<(f64, f64)> {
        let d = self.ray(rect, pos);
        if self.model != EarthModel::PerfectSphere {
            let geometry = self.model.geometry();
            return geometry
                .intersect(self.c, d)
                .map(|p| geometry.coordinates(p));
        }
        let b = self.c.dot(d);
        let disc = b * b - (self.c.length_squared() - 1.0);
        if disc < 0.0 || b > 0.0 {
            return None;
        }
        let t = -b - disc.sqrt();
        Some(v3_to_ll(self.c + d * t))
    }
}

/// The steepest tilt, near the ground: the horizon stays in view.
/// 80° until 2026-11-17, then 82°, then 85° (the user's asks the same day).
pub const MAX_TILT_DEG: f64 = 85.0;
/// Zooming out past this altitude levels the view: the tilt allowed eases to straight
/// down at `LEVEL_VIEW_ALTITUDE_KM`, as in Bing Maps (user, 2026-11-17).
const TILT_EASES_FROM_ALTITUDE_KM: f64 = 6_000.0;
const LEVEL_VIEW_ALTITUDE_KM: f64 = 15_000.0;
/// A tilted camera keeps at least this height above the ground point: eye level.
/// It was 300 m, which pushed the camera away on every tilt close up (user,
/// 2026-11-16); the app keeps the camera above the relief itself.
const MIN_TILTED_HEIGHT_KM: f64 = 0.002;

impl Cam {
    /// The steepest tilt at this distance: `MAX_TILT_DEG`, less when the camera would
    /// otherwise come closer to the ground than `MIN_TILTED_HEIGHT_KM`, and less far
    /// out, down to none at `LEVEL_VIEW_ALTITUDE_KM` (zooming out levels the view).
    pub fn max_tilt(&self) -> f64 {
        let height_ratio = (MIN_TILTED_HEIGHT_KM / self.alt.max(MIN_TILTED_HEIGHT_KM)).min(1.0);
        let far = ((self.alt - TILT_EASES_FROM_ALTITUDE_KM)
            / (LEVEL_VIEW_ALTITUDE_KM - TILT_EASES_FROM_ALTITUDE_KM))
            .clamp(0.0, 1.0);
        let far_limit = MAX_TILT_DEG * (1.0 - far * far * (3.0 - 2.0 * far));
        height_ratio
            .acos()
            .to_degrees()
            .min(MAX_TILT_DEG)
            .min(far_limit)
    }
    /// Sets the tilt, kept between straight down and `max_tilt`.
    pub fn set_tilt(&mut self, degrees: f64) {
        if degrees.is_finite() {
            self.tilt = degrees.clamp(0.0, self.max_tilt());
        }
    }
    /// Tilts to `degrees` (up to `MAX_TILT_DEG`), backing the camera off from the
    /// ground point when it is too close for that tilt: tilting by hand never
    /// locks, it zooms out as far as it needs.
    pub fn tilt_zooming_out(&mut self, degrees: f64) {
        /// Past the exact distance, so `max_tilt` does not round below the tilt.
        const MARGIN: f64 = 1.0 + 1e-9;
        if !degrees.is_finite() {
            return;
        }
        let tilt = degrees.clamp(0.0, MAX_TILT_DEG);
        let needed = MIN_TILTED_HEIGHT_KM / tilt.to_radians().cos() * MARGIN;
        if self.alt < needed {
            self.alt = needed.min(MAX_CAMERA_ALTITUDE_KM);
        }
        self.tilt = tilt.min(self.max_tilt());
    }
    /// Moves the looked-at point `rise_km` up along the line of sight, keeping the
    /// eye where it is: when the ground under the view centre turns out higher or
    /// lower (finer relief arrived, or a pan moved the centre over a slope), the view
    /// stays still instead of the camera moving with its target. Easing the target
    /// up and down moved the whole picture after every stop (user's recording,
    /// 2026-11-17). Returns the rise made, less when the camera reached its closest
    /// distance.
    pub fn slide_target(&mut self, rise_km: f64) -> f64 {
        let t = self.tilt.clamp(0.0, MAX_TILT_DEG).to_radians();
        // Steeper views move the target far along the ground for a little height.
        let cos_t = t.cos();
        let rise_km = rise_km.min((self.alt - MIN_CAMERA_ALTITUDE_KM).max(0.0) * cos_t);
        let along_km = rise_km / cos_t;
        self.alt -= along_km;
        // Toward the camera when it rises: back against the heading.
        let back_km = along_km * t.sin();
        (self.lat, self.lon) =
            crate::math::destination(self.lat, self.lon, self.heading + 180.0, back_km);
        self.lat = self
            .lat
            .clamp(-MAX_CAMERA_LATITUDE_DEG, MAX_CAMERA_LATITUDE_DEG);
        rise_km
    }
    /// How far [`Self::slide_target`] must raise the looked-at point (km; negative:
    /// lower it) for it to lie where the line of sight first meets the ground, now
    /// that it is `ground_km` high. `ground` gives the ground's height (km) at a place,
    /// `None` where it is not known. Searched from the eye outward: measuring the
    /// ground under the point and moving the point there never settled over a steep
    /// slope, and the point jumped kilometres along the line of sight from frame to
    /// frame, which jiggled the view and threw the zoom (user's recordings,
    /// 2026-11-17). `None` when the line meets no known ground, or meets it at once.
    pub fn sight_hit(
        &self,
        ground_km: f64,
        ground: impl Fn(f64, f64) -> Option<f64>,
    ) -> Option<f64> {
        /// Even steps from the eye out, then halvings between the last two.
        const STEPS: usize = 128;
        const REFINEMENTS: usize = 20;
        /// How far past the looked-at point the line is followed, in distances.
        const BEYOND: f64 = 2.0;
        let t = self.tilt.clamp(0.0, MAX_TILT_DEG).to_radians();
        let (cos_t, sin_t) = (t.cos(), t.sin());
        // Height over the ground `s` km from the point toward the eye (negative:
        // beyond it). The line is straight; the Earth curves away below it.
        let above = |s: f64| {
            let x = s * sin_t;
            let (lat, lon) = crate::math::destination(self.lat, self.lon, self.heading + 180.0, x);
            ground(lat, lon)
                .map(|g| ground_km + s * cos_t + x * x / (2.0 * crate::EARTH_RADIUS_KM) - g)
        };
        let near = (self.alt - MIN_CAMERA_ALTITUDE_KM).max(0.0);
        let far = -BEYOND * self.alt;
        let mut clear: Option<f64> = None;
        for step in 0..=STEPS {
            let s = near + (far - near) * step as f64 / STEPS as f64;
            let Some(height) = above(s) else {
                continue;
            };
            if height > 0.0 {
                clear = Some(s);
                continue;
            }
            // Ground right at the eye (it backed away from a slope as it turned): the
            // point stays. Moved up to the eye, it left the camera 30 m from its
            // centre, where zooming barely moved (2026-11-17).
            let mut clear = clear?;
            let mut under = s;
            for _ in 0..REFINEMENTS {
                let middle = 0.5 * (clear + under);
                match above(middle) {
                    Some(height) if height > 0.0 => clear = middle,
                    _ => under = middle,
                }
            }
            return Some(0.5 * (clear + under) * cos_t);
        }
        None
    }
    /// Keeps the tilt within `max_tilt` after the distance changed.
    pub fn clamp_tilt(&mut self) {
        /// Rounding in `max_tilt` at the zoom limit must not nibble at the tilt.
        const TILT_TOLERANCE_DEG: f64 = 1e-9;
        let max = self.max_tilt();
        if self.tilt > max + TILT_TOLERANCE_DEG || self.tilt < 0.0 {
            self.tilt = self.tilt.clamp(0.0, max);
        }
    }
    /// This camera with `clamp_tilt` applied.
    pub fn with_tilt_clamped(mut self) -> Self {
        self.clamp_tilt();
        self
    }
    fn deg_per_pt(&self, height_pts: f32) -> f64 {
        let mpp =
            self.alt * METERS_PER_KM * 2.0 * VERTICAL_FOV_HALF_TAN / height_pts.max(1.0) as f64;
        mpp / METERS_PER_LATITUDE_DEGREE
    }

    pub fn pan(&mut self, dx: f64, dy: f64, height_pts: f32) {
        let k = self.deg_per_pt(height_pts);
        let h = self.heading.to_radians();
        let movement = DMat2::from_angle(-h) * DVec2::new(-dx * k, dy * k);
        let (de, dn) = (movement.x, movement.y);
        if self.pans_over_a_pole(dn) {
            self.pan_great_circle(de, dn);
            return;
        }
        self.lat = (self.lat + dn).clamp(-MAX_CAMERA_LATITUDE_DEG, MAX_CAMERA_LATITUDE_DEG);
        self.lon =
            wrap_lon(self.lon + de / self.lat.to_radians().cos().max(MIN_PAN_LATITUDE_COSINE));
    }

    /// Whether a pan by `dn` degrees north starts or ends toward a pole, where steps
    /// in latitude and longitude turn the picture around the pole: a small drag
    /// sideways spun the view (at 87°, nineteen degrees of longitude for one of
    /// ground), and steps north stopped at the pole.
    fn pans_over_a_pole(&self, dn: f64) -> bool {
        self.lat.abs() > POLE_PAN_LATITUDE_DEG || (self.lat + dn).abs() > POLE_PAN_LATITUDE_DEG
    }

    /// A pan along the great circle the drag points in, `de` and `dn` degrees east and
    /// north: the view goes straight over the pole and down the other side. Near the
    /// pole it turns with the globe as it would held in the hand; farther away north
    /// keeps its place on screen more and more, until at `POLE_PAN_LATITUDE_DEG` the
    /// pan is the same as a step in latitude and longitude. (One switch at 87° from
    /// "north stays up" to "held in the hand" made the view lurch there.)
    fn pan_great_circle(&mut self, de: f64, dn: f64) {
        let degrees = de.hypot(dn);
        if degrees <= 0.0 {
            return;
        }
        /// A step that would end in the last tenth of a degree around the pole, where
        /// the camera may not stand, goes on along its circle until it is out again
        /// (clamped, small steps never got past the pole).
        const CAP_STEP_DEG: f64 = 0.01;
        const CAP_MAX_STEPS: usize = 100;
        let bearing = de.atan2(dn).to_degrees();
        let mut to = (self.lat, self.lon);
        for extra in 0..=CAP_MAX_STEPS {
            let arc = degrees + extra as f64 * CAP_STEP_DEG;
            to = crate::math::destination(
                self.lat,
                self.lon,
                bearing,
                arc.to_radians() * crate::EARTH_RADIUS_KM,
            );
            if to.0.abs() <= MAX_CAMERA_LATITUDE_DEG {
                break;
            }
        }
        let (heading, latitude) = (self.heading, self.lat.abs());
        self.rotate(ll_to_v3(self.lat, self.lon), ll_to_v3(to.0, to.1));
        let held = ((latitude.max(self.lat.abs()) - POLE_PAN_LATITUDE_DEG)
            / (POLE_HELD_LATITUDE_DEG - POLE_PAN_LATITUDE_DEG))
            .clamp(0.0, 1.0);
        let held = held * held * (3.0 - 2.0 * held);
        self.heading = wrap_lon(heading + wrap_lon(self.heading - heading) * held);
    }

    pub fn pan_for_model(&mut self, dx: f64, dy: f64, height_pts: f32, model: EarthModel) {
        if model == EarthModel::PerfectSphere {
            self.pan(dx, dy, height_pts);
            return;
        }
        let km_per_point = self.alt * 2.0 * VERTICAL_FOV_HALF_TAN / f64::from(height_pts.max(1.0));
        let movement =
            DMat2::from_angle(-self.heading.to_radians()) * DVec2::new(-dx, dy) * km_per_point;
        let (meridional, prime_vertical) = model.geometry().curvature_km(self.lat);
        if self.pans_over_a_pole((movement.y / meridional).to_degrees()) {
            // The sphere's step is within a third of a percent of the ellipsoid's.
            self.pan_great_circle(
                (movement.x / prime_vertical).to_degrees(),
                (movement.y / meridional).to_degrees(),
            );
            return;
        }
        self.lat = (self.lat + (movement.y / meridional).to_degrees())
            .clamp(-MAX_CAMERA_LATITUDE_DEG, MAX_CAMERA_LATITUDE_DEG);
        self.lon = wrap_lon(
            self.lon
                + (movement.x / prime_vertical).to_degrees()
                    / self.lat.to_radians().cos().max(MIN_PAN_LATITUDE_COSINE),
        );
    }
    pub fn zoom_at_for_model(
        &mut self,
        factor: f64,
        cursor: Option<Pos2>,
        rect: Rect,
        aspect: f64,
        model: EarthModel,
    ) {
        self.zoom_at_over_ground(factor, cursor, rect, aspect, model, 0.0);
    }
    /// `zoom_at_for_model` for a view raised `ground_km` (see `basis_over_ground`).
    pub fn zoom_at_over_ground(
        &mut self,
        factor: f64,
        cursor: Option<Pos2>,
        rect: Rect,
        aspect: f64,
        model: EarthModel,
        ground_km: f64,
    ) {
        if model == EarthModel::PerfectSphere && ground_km == 0.0 {
            self.zoom_at(factor, cursor, rect, aspect);
            return;
        }
        let pick = |cam: &Self, cursor| {
            cam.basis_over_ground(aspect, model, ground_km)
                .pick(rect, cursor)
        };
        let before = cursor.and_then(|c| pick(self, c));
        self.zoom(factor);
        if let (Some(p0), Some(cursor)) = (before, cursor) {
            self.anchor_keeping_heading(p0, |cam| pick(cam, cursor));
        }
    }

    /// Moves the camera so `pick` (the point under the cursor) returns `target` again,
    /// keeping the heading: zooming toward a point never turns north away (user,
    /// 2026-10-25). Each step turns the camera rigidly toward the target, then restores
    /// the heading; the next step corrects what that moved. Where a fixed heading cannot
    /// hold the point exactly (near the globe's edge), the closest match is kept: north
    /// stays locked and the point may slip by a little. Near a pole, or toward a point
    /// across it, a fixed heading would carry the view far round the globe (the user's
    /// recording of 2026-11-05 jumped from the Arctic to Japan): then the camera turns
    /// rigidly, as a globe held in the hand.
    fn anchor_keeping_heading(
        &mut self,
        target: (f64, f64),
        pick: impl Fn(&Self) -> Option<(f64, f64)>,
    ) {
        /// Within a degree of a pole north points every way: the camera turns rigidly.
        const FREE_HEADING_MIN_LATITUDE_DEG: f64 = 89.0;
        /// The heading is given up when keeping it moves the view's centre this many
        /// times further than turning freely does, or holds the point less well.
        const HEADING_DETOUR_FACTOR: f64 = 2.0;
        const HEADING_DETOUR_SLACK_KM: f64 = 1.0;
        let start = *self;
        let rigid = {
            let mut cam = start;
            let error = cam.anchor(target, &pick, false);
            (error, cam)
        };
        if start.lat.abs() >= FREE_HEADING_MIN_LATITUDE_DEG {
            *self = rigid.1;
            return;
        }
        let kept_error = self.anchor(target, &pick, true);
        let moved =
            |cam: &Self| crate::math::haversine_km((start.lat, start.lon), (cam.lat, cam.lon));
        let detour =
            moved(self) > HEADING_DETOUR_FACTOR * moved(&rigid.1) + HEADING_DETOUR_SLACK_KM;
        let slipped = kept_error > rigid.0 + HEADING_DETOUR_SLACK_KM;
        if detour || slipped {
            *self = rigid.1;
        }
    }

    /// The anchoring steps of `anchor_keeping_heading`, the heading kept or free; leaves
    /// the camera at its best step and returns that step's distance from the target
    /// (infinite when the point never showed).
    fn anchor(
        &mut self,
        target: (f64, f64),
        pick: &impl Fn(&Self) -> Option<(f64, f64)>,
        keep_heading: bool,
    ) -> f64 {
        const MAX_ANCHOR_ITERATIONS: usize = 24;
        const ANCHOR_TOLERANCE_KM: f64 = 1e-8;
        let heading = self.heading;
        let mut best: Option<(f64, Self)> = None;
        for _ in 0..MAX_ANCHOR_ITERATIONS {
            let Some(p1) = pick(self) else {
                break;
            };
            let error = crate::math::haversine_km(p1, target);
            if best.is_none_or(|(least, _)| error < least) {
                best = Some((error, *self));
            }
            if error <= ANCHOR_TOLERANCE_KM {
                break;
            }
            self.rotate(ll_to_v3(p1.0, p1.1), ll_to_v3(target.0, target.1));
            if keep_heading {
                self.heading = heading;
            }
        }
        match best {
            Some((error, cam)) => {
                *self = cam;
                error
            }
            None => f64::INFINITY,
        }
    }

    /// Changes the distance by `factor`. A tilted camera keeps its tilt: zooming in
    /// stops where the camera reaches `MIN_TILTED_HEIGHT_KM` above the ground (tilting
    /// back lets it go closer). Flattening the tilt instead lost it for good, and a
    /// zoom that undid the tilt looked like a frozen picture (user, 2026-10-25).
    pub fn zoom(&mut self, factor: f64) {
        let closest = if self.tilt > 0.0 {
            (MIN_TILTED_HEIGHT_KM / self.tilt.to_radians().cos()).max(MIN_CAMERA_ALTITUDE_KM)
        } else {
            MIN_CAMERA_ALTITUDE_KM
        };
        let wanted = self.alt * factor;
        self.alt = if factor < 1.0 && self.alt <= closest {
            // Already at (or, after an earlier tilt, below) the limit: stay.
            self.alt
        } else {
            wanted.clamp(closest.min(self.alt), MAX_CAMERA_ALTITUDE_KM)
        };
        self.clamp_tilt();
    }

    /// Rigidly rotates the camera (centre and "up" direction) by the rotation that takes `from` to `to`.
    fn rotate(&mut self, from: DVec3, to: DVec3) {
        let axis = from.cross(to);
        let sn = axis.length();
        if sn < ROTATION_AXIS_EPSILON {
            return;
        }
        let ang = sn.atan2(from.dot(to));
        let k = axis * (1.0 / sn);
        let rotation = DQuat::from_axis_angle(k, ang);
        let n = ll_to_v3(self.lat, self.lon);
        let (la, lo) = (self.lat.to_radians(), self.lon.to_radians());
        let east = DVec3::new(-lo.sin(), lo.cos(), 0.0);
        let north = DVec3::new(-la.sin() * lo.cos(), -la.sin() * lo.sin(), la.cos());
        let h = self.heading.to_radians();
        let up = north * h.cos() + east * h.sin();
        let (n2, u2) = ((rotation * n).normalize(), rotation * up);
        let (lat2, lon2) = v3_to_ll(n2);
        let (la2, lo2) = (lat2.to_radians(), lon2.to_radians());
        let east2 = DVec3::new(-lo2.sin(), lo2.cos(), 0.0);
        let north2 = DVec3::new(-la2.sin() * lo2.cos(), -la2.sin() * lo2.sin(), la2.cos());
        self.lat = lat2.clamp(-MAX_CAMERA_LATITUDE_DEG, MAX_CAMERA_LATITUDE_DEG);
        self.lon = wrap_lon(lon2);
        self.heading = wrap_lon(u2.dot(east2).atan2(u2.dot(north2)).to_degrees());
    }

    /// Zooms keeping the geographic point under `cursor` fixed on screen (like Google Earth).
    pub fn zoom_at(&mut self, factor: f64, cursor: Option<Pos2>, rect: Rect, aspect: f64) {
        let before = cursor.and_then(|c| self.basis(aspect).pick(rect, c));
        self.zoom(factor);
        if let (Some(p0), Some(c)) = (before, cursor) {
            self.anchor_keeping_heading(p0, |cam| cam.basis(aspect).pick(rect, c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::haversine_km;
    use emath::Vec2;

    #[test]
    fn zooming_far_out_levels_the_view() {
        let mut cam = Cam {
            lat: 46.5,
            lon: 10.7,
            heading: 0.0,
            alt: 5_000.0,
            tilt: 60.0,
        };
        cam.set_tilt(MAX_TILT_DEG);
        assert_eq!(cam.tilt, MAX_TILT_DEG);
        // Halfway between 6,000 and 15,000 km: half the steepest tilt at most.
        cam.alt = (TILT_EASES_FROM_ALTITUDE_KM + LEVEL_VIEW_ALTITUDE_KM) / 2.0;
        cam.zoom(1.0);
        assert!((cam.tilt - MAX_TILT_DEG / 2.0).abs() < 1e-9, "{}", cam.tilt);
        while cam.alt < LEVEL_VIEW_ALTITUDE_KM {
            cam.zoom(1.25);
        }
        assert_eq!(cam.tilt, 0.0);
        // Coming back in leaves it level, as Bing Maps does.
        cam.alt = 5_000.0;
        cam.zoom(1.0);
        assert_eq!(cam.tilt, 0.0);
    }

    #[test]
    fn the_looked_at_point_lands_where_the_line_of_sight_meets_the_ground_and_stays() {
        // A ridge running east–west 3 km north of the point, 1.5 km high, over
        // ground 0.2 km high; the camera looks north at it from the south.
        let ridge = |lat: f64, _lon: f64| {
            let north_km = (lat - 46.5) * 111.2;
            Some(0.2 + 1.3 * (-((north_km - 3.0) / 0.8).powi(2)).exp())
        };
        for tilt in [0.0, 50.0, 75.0] {
            let mut cam = Cam {
                lat: 46.5,
                lon: 10.7,
                heading: 0.0,
                alt: 8.0,
                tilt,
            };
            let mut ground_km = 0.0;
            for _ in 0..3 {
                let rise = cam
                    .sight_hit(ground_km, ridge)
                    .expect("the line meets the ground");
                ground_km += cam.slide_target(rise);
            }
            let here = ridge(cam.lat, cam.lon).unwrap();
            assert!(
                (ground_km - here).abs() < 0.01,
                "tilt {tilt}: {ground_km} km over {here} km"
            );
            // Settled: asked again, it stays.
            let again = cam.sight_hit(ground_km, ridge).unwrap();
            assert!(again.abs() < 0.002, "tilt {tilt}: moves {again} km more");
        }
        // Looking past the ridge's crest would be looking through it: the point is
        // on its near face, not on the flat beyond.
        let cam = Cam {
            lat: 46.5 + 4.0 / 111.2,
            lon: 10.7,
            heading: 0.0,
            alt: 8.0,
            tilt: 75.0,
        };
        let rise = cam.sight_hit(0.2, ridge).unwrap();
        assert!(rise > 0.3, "the ridge in front is found first: {rise}");
    }

    #[test]
    fn sliding_the_target_keeps_the_eye_and_the_line_of_sight() {
        for tilt in [0.0, 45.0, 75.0] {
            let mut cam = Cam {
                lat: 46.5,
                lon: 10.7,
                heading: 30.0,
                alt: 5.0,
                tilt,
            };
            let before = cam.basis_over_ground(1.5, EarthModel::Wgs84, 1.2);
            let rise = cam.slide_target(0.6);
            assert_eq!(rise, 0.6);
            let after = cam.basis_over_ground(1.5, EarthModel::Wgs84, 1.2 + rise);
            let moved_m = (after.c - before.c).length() * EARTH_RADIUS_KM * 1000.0;
            // The step is taken on the flat: over 2 km along the ground (75°) the
            // curve leaves a few metres, a thousandth of the distance.
            assert!(moved_m < 10.0, "tilt {tilt}: the eye moved {moved_m} m");
            let forward = |basis: &Basis| basis.world_from_view * DVec3::NEG_Z;
            let turned = forward(&before).angle_between(forward(&after)).to_degrees();
            assert!(turned < 0.05, "tilt {tilt}: the view turned {turned}°");
        }
        // It stops at the closest distance.
        let mut cam = Cam {
            lat: 46.5,
            lon: 10.7,
            heading: 0.0,
            alt: 0.5,
            tilt: 0.0,
        };
        let rise = cam.slide_target(2.0);
        assert!((cam.alt - MIN_CAMERA_ALTITUDE_KM).abs() < 1e-12);
        assert!((rise - (0.5 - MIN_CAMERA_ALTITUDE_KM)).abs() < 1e-12);
    }

    /// Dragging sideways at 86° moves the globe with the hand: the pole, in view
    /// above the centre, goes along by about the drag. Stepping in longitude left
    /// the pole where it was and spun everything else around it (user, 2026-11-16).
    #[test]
    fn a_sideways_drag_near_the_pole_moves_the_pole_with_it() {
        let (rect, aspect) = (
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0)),
            1.5,
        );
        let start = Cam {
            lat: 86.0,
            lon: 10.0,
            heading: 0.0,
            alt: 2_000.0,
            tilt: 0.0,
        };
        let mut cam = start;
        let pole = ll_to_v3(90.0, 0.0);
        let before = cam.basis(aspect).project(pole, rect).unwrap();
        cam.pan(60.0, 0.0, 800.0);
        let after = cam.basis(aspect).project(pole, rect).unwrap();
        let moved = after - before;
        assert!(
            (moved.x - 60.0).abs() < 6.0 && moved.y.abs() < 6.0,
            "{moved:?}"
        );
        // At 50° north stays up, as before.
        let mut cam = Cam { lat: 50.0, ..start };
        cam.pan(60.0, 0.0, 800.0);
        assert_eq!((cam.heading, cam.lat), (0.0, 50.0));
        // And between, the heading turns by less than the globe does.
        let mut cam = Cam { lat: 72.0, ..start };
        let mut held = cam;
        cam.pan(60.0, 0.0, 800.0);
        held.rotate(ll_to_v3(72.0, 10.0), ll_to_v3(cam.lat, cam.lon));
        assert!(cam.heading.abs() > 0.0 && cam.heading.abs() < held.heading.abs());
    }

    /// Zooming toward a point across the North Pole moves the view toward it in
    /// small steps, never round the globe (the user's recording of 2026-11-05).
    #[test]
    fn zooming_toward_a_point_across_the_pole_does_not_jump() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let rect = Rect::from_min_size(emath::Pos2::ZERO, Vec2::new(1200.0, 800.0));
            let aspect = 1.5;
            let mut cam = Cam {
                lat: 82.0,
                lon: -34.0,
                heading: 0.0,
                alt: 7_000.0,
                tilt: 0.0,
            };
            // Above the screen's middle, past the pole.
            let cursor = Pos2::new(640.0, 160.0);
            let target = cam
                .basis_for_model(aspect, model)
                .pick(rect, cursor)
                .expect("the cursor is on the globe");
            assert!(target.1.abs() > 90.0, "across the pole: {target:?}");
            for _ in 0..20 {
                let before = cam;
                cam.zoom_at_for_model(0.85, Some(cursor), rect, aspect, model);
                let step = crate::math::haversine_km((before.lat, before.lon), (cam.lat, cam.lon));
                assert!(step < 600.0, "{model:?}: jumped {step:.0} km to {cam:?}");
                let held = cam
                    .basis_for_model(aspect, model)
                    .pick(rect, cursor)
                    .expect("the point stays on screen");
                assert!(
                    crate::math::haversine_km(held, target) < 5.0,
                    "{model:?}: the point slipped to {held:?}"
                );
            }
        }
    }

    /// Dragging northwards goes straight over the North Pole and down the other side,
    /// on both Earth models, in small steps as a drag sends them.
    #[test]
    fn panning_crosses_the_pole() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let mut cam = Cam {
                lat: 88.0,
                lon: 10.0,
                heading: 0.0,
                alt: 500.0,
                tilt: 0.0,
            };
            let mut lowest_step = f64::MAX;
            let mut previous = cam;
            for _ in 0..80 {
                cam.pan_for_model(0.0, 5.0, 800.0, model);
                let step =
                    crate::math::haversine_km((previous.lat, previous.lon), (cam.lat, cam.lon));
                lowest_step = lowest_step.min(step);
                previous = cam;
            }
            // 80 steps of about 0.02°: not yet over. Keep going.
            for _ in 0..80 {
                cam.pan_for_model(0.0, 5.0, 800.0, model);
            }
            assert!(cam.lat < 89.0, "{model:?}: went past the pole, {cam:?}");
            assert!(
                (crate::math::wrap_lon(cam.lon - 190.0)).abs() < 1.0,
                "{model:?}: down the other side, {cam:?}"
            );
            assert!(
                (cam.heading.abs() - 180.0).abs() < 1.0,
                "{model:?}: {cam:?}"
            );
            assert!(lowest_step > 0.0, "{model:?}: never stuck");
        }
    }

    /// Satellites show above the horizon and over the limb, and hide behind the Earth.
    #[test]
    fn points_above_the_ground_hide_only_behind_the_earth() {
        let rect = Rect::from_min_size(emath::Pos2::ZERO, Vec2::new(1000.0, 1000.0));
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 30_000.0,
            tilt: 0.0,
        }
        .basis(1.0);
        let above = |lat: f64, lon: f64, km: f64| {
            basis.project_above(basis.raised_position(lat, lon, km), rect)
        };
        let overhead = above(0.0, 0.0, 400.0).unwrap();
        assert!((overhead - rect.center()).length() < 0.5, "{overhead:?}");
        // The far side is hidden; a geostationary satellite a quarter turn away is
        // past the horizon but stands out beside the globe.
        assert!(above(0.0, 180.0, 400.0).is_none());
        assert!(above(0.0, 140.0, 2_000.0).is_none());
        assert!(
            basis
                .project(basis.surface_position(0.0, 90.0), rect)
                .is_none()
        );
        let geostationary = above(0.0, 90.0, 35_786.0).unwrap();
        assert!(geostationary.x > overhead.x + 100.0, "{geostationary:?}");
        // On the ellipsoid a plane over the pole (inside the mean sphere) still shows.
        let polar = Cam {
            lat: 89.0,
            lon: 0.0,
            heading: 0.0,
            alt: 50.0,
            tilt: 0.0,
        }
        .basis_for_model(1.0, EarthModel::Wgs84);
        assert!(
            polar
                .project_above(polar.raised_position(89.1, 0.0, 10.0), rect)
                .is_some()
        );
        assert!(
            polar
                .project_above(polar.raised_position(89.0, 0.0, 0.0), rect)
                .is_some(),
            "a point on the ground is not hidden by the ground"
        );
    }

    /// Roadmap 1.9: at the closest camera, a centimetre of camera motion moves the
    /// ground on screen smoothly (no float steps), and picking a projected point
    /// lands within a centimetre of it, on both Earth models.
    #[test]
    fn street_level_projection_and_picking_have_centimetre_precision() {
        const CENTIMETRE_DEG: f64 = 1e-5 / 111.32;
        let rect = Rect::from_min_size(emath::Pos2::ZERO, Vec2::new(1600.0, 1000.0));
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            for tilt in [0.0, 60.0] {
                let base = Cam {
                    lat: 47.3769,
                    lon: 8.5417,
                    heading: 30.0,
                    alt: crate::MIN_CAMERA_ALTITUDE_KM,
                    tilt,
                };
                let target = base
                    .basis_for_model(1.6, model)
                    .pick(rect, rect.center())
                    .unwrap();
                let target = crate::math::destination(target.0, target.1, 30.0, 0.02);
                let screen = |step: f64| {
                    let mut cam = base;
                    cam.lon += step * CENTIMETRE_DEG;
                    let basis = cam.basis_for_model(1.6, model);
                    basis
                        .project(basis.surface_position(target.0, target.1), rect)
                        .unwrap()
                };
                // Equal camera steps give equal screen steps, to a hundredth of the step.
                let steps: Vec<f32> = (0..10)
                    .map(|i| screen(f64::from(i + 1)).x - screen(f64::from(i)).x)
                    .collect();
                let mean = steps.iter().sum::<f32>() / steps.len() as f32;
                assert!(mean.abs() > 0.01, "{model:?} {tilt}: {mean}");
                for step in &steps {
                    assert!(
                        (step - mean).abs() < mean.abs() * 0.01,
                        "{model:?} {tilt}: {steps:?}"
                    );
                }
                // Projecting and picking back returns the same ground point.
                let basis = base.basis_for_model(1.6, model);
                let at = basis
                    .project(basis.surface_position(target.0, target.1), rect)
                    .unwrap();
                let picked = basis.pick(rect, at).unwrap();
                assert!(model.distance_km(picked, target) < 1e-5, "{model:?} {tilt}");
            }
        }
    }

    #[test]
    fn a_tilted_camera_looks_at_its_ground_point_from_behind_and_above() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            for (heading, tilt) in [(0.0, 45.0), (120.0, 70.0), (-30.0, 20.0)] {
                let (cam, rect, aspect) = test_app_cam(Cam {
                    lat: 46.5,
                    lon: 8.0,
                    heading,
                    alt: 50.0,
                    tilt,
                });
                let basis = cam.basis_for_model(aspect, model);
                // The screen centre is still the ground point.
                let centre = basis.pick(rect, rect.center()).unwrap();
                assert!(model.distance_km(centre, (46.5, 8.0)) < 1e-6, "{model:?}");
                // Looking toward the heading: points ahead are higher on screen and
                // drawn smaller than points behind.
                let ahead = crate::math::destination(46.5, 8.0, heading, 20.0);
                let behind = crate::math::destination(46.5, 8.0, heading + 180.0, 20.0);
                let up = basis
                    .project(basis.surface_position(ahead.0, ahead.1), rect)
                    .unwrap();
                let down = basis
                    .project(basis.surface_position(behind.0, behind.1), rect)
                    .unwrap();
                assert!(up.y < rect.center().y && down.y > rect.center().y);
                assert!(rect.center().y - up.y < down.y - rect.center().y);
                // Projection and picking agree off-centre too.
                let spot = rect.center() + Vec2::new(150.0, -120.0);
                let picked = basis.pick(rect, spot).unwrap();
                let back = basis
                    .project(basis.surface_position(picked.0, picked.1), rect)
                    .unwrap();
                assert!(back.distance(spot) < 0.01, "{model:?} {back:?}");
                // The camera is about range × cos(tilt) above the ground.
                let ground = basis.surface_position(46.5, 8.0).length();
                let height_km = (basis.c.length() - ground) * EARTH_RADIUS_KM;
                let expected = 50.0 * tilt.to_radians().cos();
                assert!((height_km - expected).abs() < 2.0, "{height_km} {expected}");
            }
        }
    }

    #[test]
    fn tilt_is_limited_near_the_ground_and_zooming_keeps_it() {
        let mut cam = Cam {
            lat: 10.0,
            lon: 10.0,
            heading: 0.0,
            alt: 100.0,
            tilt: 0.0,
        };
        cam.set_tilt(200.0);
        assert_eq!(cam.tilt, MAX_TILT_DEG);
        cam.set_tilt(-5.0);
        assert_eq!(cam.tilt, 0.0);
        cam.set_tilt(60.0);
        cam.zoom(0.5);
        assert_eq!(cam.tilt, 60.0);
        // Zooming all the way in keeps the tilt and stops at the closest distance,
        // still above the minimum height.
        for _ in 0..200 {
            cam.zoom(0.8);
        }
        assert_eq!(cam.tilt, 60.0);
        assert_eq!(cam.alt, MIN_CAMERA_ALTITUDE_KM);
        assert!(cam.alt * cam.tilt.to_radians().cos() >= MIN_TILTED_HEIGHT_KM);
        cam.zoom(2.0);
        assert_eq!(cam.tilt, 60.0);
        // Very close in, the camera may not dip below the minimum height.
        cam.alt = 0.004;
        cam.set_tilt(80.0);
        assert!(cam.tilt < 80.0);
        assert!(cam.alt * cam.tilt.to_radians().cos() >= MIN_TILTED_HEIGHT_KM - 1e-9);
        let (rect, aspect) = (
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0)),
            1.5,
        );
        let mut tilted = Cam {
            alt: 80.0,
            tilt: 55.0,
            heading: 25.0,
            ..cam
        };
        let cursor = rect.center() + Vec2::new(-200.0, 100.0);
        let before = tilted.basis(aspect).pick(rect, cursor).unwrap();
        tilted.zoom_at(0.7, Some(cursor), rect, aspect);
        let after = tilted.basis(aspect).pick(rect, cursor).unwrap();
        assert!(crate::math::haversine_km(before, after) < 1e-3);
        assert_eq!((tilted.tilt, tilted.heading), (55.0, 25.0));
    }

    #[test]
    fn zooming_toward_the_cursor_keeps_north_from_any_altitude_and_spot() {
        let mut turned = Vec::new();
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            for alt in [20_000.0, 8_000.0, 2_000.0, 300.0, 20.0, 1.0] {
                for (dx, dy) in [
                    (-300.0, 200.0),
                    (250.0, -150.0),
                    (60.0, 30.0),
                    (-380.0, -260.0),
                ] {
                    let (mut cam, rect, aspect) = test_app_cam(Cam {
                        lat: 30.0,
                        lon: 20.0,
                        heading: 0.0,
                        alt,
                        tilt: 0.0,
                    });
                    let cursor = rect.center() + Vec2::new(dx, dy);
                    let Some(before) = cam.basis_for_model(aspect, model).pick(rect, cursor) else {
                        continue;
                    };
                    for _ in 0..10 {
                        cam.zoom_at_for_model(0.8, Some(cursor), rect, aspect, model);
                    }
                    let after = cam
                        .basis_for_model(aspect, model)
                        .pick(rect, cursor)
                        .unwrap();
                    // North stays; near the globe's edge the point may slip a little.
                    if cam.heading.abs() > 1e-9 || model.distance_km(before, after) > alt * 0.01 {
                        turned.push((model, alt, dx, dy, cam.heading));
                    }
                }
            }
        }
        assert!(turned.is_empty(), "{turned:?}");
    }

    #[test]
    fn zooming_toward_the_cursor_keeps_north_where_it_was() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            for heading in [0.0, 35.0, -120.0] {
                let (mut cam, rect, aspect) = test_app_cam(Cam {
                    lat: 48.1,
                    lon: 11.6,
                    heading,
                    alt: 900.0,
                    tilt: 0.0,
                });
                let cursor = rect.center() + Vec2::new(-150.0, 90.0);
                let before = cam
                    .basis_for_model(aspect, model)
                    .pick(rect, cursor)
                    .unwrap();
                for factor in [0.6, 0.6, 0.6, 1.5] {
                    cam.zoom_at_for_model(factor, Some(cursor), rect, aspect, model);
                    let after = cam
                        .basis_for_model(aspect, model)
                        .pick(rect, cursor)
                        .unwrap();
                    assert!(
                        model.distance_km(before, after) < 1e-6,
                        "{model:?} {heading}"
                    );
                    assert!((cam.heading - heading).abs() < 1e-9, "{model:?} {heading}");
                }
            }
        }
    }

    #[test]
    fn wgs84_geodetic_picking_projection_and_zoom_agree() {
        for (lat, lon, heading, alt) in [
            (45.0, 179.8, 35.0, 1000.0),
            (89.9, -170.0, 120.0, 500.0),
            (-60.0, 120.0, -130.0, 800.0),
            (0.0, 0.0, 0.0, 0.15),
        ] {
            let (mut cam, rect, aspect) = test_app_cam(Cam {
                lat,
                lon,
                heading,
                alt,
                tilt: 0.0,
            });
            let model = EarthModel::Wgs84;
            let basis = cam.basis_for_model(aspect, model);
            let picked = basis.pick(rect, rect.center()).unwrap();
            assert!(model.distance_km((lat, lon), picked) < 1e-6);
            let projected = basis
                .project(basis.surface_position(lat, lon), rect)
                .unwrap();
            assert!(projected.distance(rect.center()) < 0.001);
            let cursor = rect.center() + Vec2::new(80.0, 40.0);
            let Some(before) = basis.pick(rect, cursor) else {
                continue;
            };
            for factor in [0.7, 0.5, 1.3] {
                cam.zoom_at_for_model(factor, Some(cursor), rect, aspect, model);
                let after = cam
                    .basis_for_model(aspect, model)
                    .pick(rect, cursor)
                    .unwrap();
                assert!(
                    model.distance_km(before, after) < 1e-6,
                    "anchor moved at {lat}, {lon}"
                );
            }
        }
    }

    fn test_app_cam(cam: Cam) -> (Cam, Rect, f64) {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0));
        (cam, rect, 1200.0 / 800.0)
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        for (lat, lon, hd, alt) in [
            (-15.0, -50.0, 0.0, 14_000.0),
            (40.0, 10.0, 70.0, 3000.0),
            (-60.0, 120.0, -130.0, 800.0),
        ] {
            let (mut cam, rect, aspect) = test_app_cam(Cam {
                lat,
                lon,
                heading: hd,
                alt,
                tilt: 0.0,
            });
            for cursor in [
                Pos2::new(300.0, 250.0),
                Pos2::new(900.0, 600.0),
                Pos2::new(610.0, 395.0),
            ] {
                let Some(before) = cam.basis(aspect).pick(rect, cursor) else {
                    continue;
                };
                for f in [0.7, 0.5, 1.3] {
                    cam.zoom_at(f, Some(cursor), rect, aspect);
                    let after = cam.basis(aspect).pick(rect, cursor).unwrap();
                    assert!(
                        haversine_km(before, after) < 0.01,
                        "moved {} km",
                        haversine_km(before, after)
                    );
                    // the point must stay the same one after repeated zooms too
                    let again = before;
                    assert!(haversine_km(again, after) < 0.01);
                }
            }
        }
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    #[test]
    fn zoom_clamps_altitude_without_a_surface_pick() {
        let mut cam = Cam {
            lat: 10.0,
            lon: 20.0,
            heading: 30.0,
            alt: 500.0,
            tilt: 0.0,
        };
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));
        cam.zoom_at(1e10, None, rect, 4.0 / 3.0);
        assert_eq!(cam.alt, MAX_CAMERA_ALTITUDE_KM);
        cam.zoom_at(1e-10, None, rect, 4.0 / 3.0);
        assert_eq!(cam.alt, MIN_CAMERA_ALTITUDE_KM);
        assert_eq!((cam.lat, cam.lon, cam.heading), (10.0, 20.0, 30.0));
    }

    #[test]
    fn pan_preserves_camera_bounds_near_poles_and_dateline() {
        let mut cam = Cam {
            lat: 89.0,
            lon: 179.0,
            heading: 0.0,
            alt: 1000.0,
            tilt: 0.0,
        };
        cam.pan(-10000.0, 10000.0, 600.0);
        // A long drag goes over the pole (a great circle), never into its last tenth.
        assert!(cam.lat.abs() <= MAX_CAMERA_LATITUDE_DEG);
        assert!(cam.lat < 89.0, "{cam:?}");
        assert!((-180.0..180.0).contains(&cam.lon));
        assert_eq!(cam.alt, 1000.0);
    }

    #[test]
    fn screen_center_picks_camera_location() {
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));
        let cam = Cam {
            lat: -15.0,
            lon: -50.0,
            heading: 70.0,
            alt: 14000.0,
            tilt: 0.0,
        };
        let point = cam.basis(4.0 / 3.0).pick(rect, rect.center()).unwrap();
        assert!(crate::math::haversine_km(point, (cam.lat, cam.lon)) < 1e-6);
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use emath::Vec2;

    #[test]
    fn picking_and_projection_agree_across_headings_poles_and_viewports() {
        for (lat, lon, heading, alt) in [
            (0.0, 0.0, 0.0, MIN_CAMERA_ALTITUDE_KM),
            (-23.55, -46.63, 25.0, 1000.0),
            (89.9, 179.9, -130.0, 3000.0),
            (-89.9, -179.9, 180.0, MAX_CAMERA_ALTITUDE_KM),
        ] {
            for aspect in [0.75, 16.0 / 9.0, 3.0] {
                let rect = Rect::from_min_size(
                    Pos2::new(37.0, 19.0),
                    Vec2::new(800.0 * aspect as f32, 800.0),
                );
                let basis = Cam {
                    lat,
                    lon,
                    heading,
                    alt,
                    tilt: 0.0,
                }
                .basis(aspect);
                for fraction in [0.48, 0.5, 0.52] {
                    let cursor = Pos2::new(
                        rect.min.x + rect.width() * fraction,
                        rect.min.y + rect.height() * fraction,
                    );
                    let (latitude, longitude) =
                        basis.pick(rect, cursor).expect("point on visible globe");
                    let projected = basis
                        .project(ll_to_v3(latitude, longitude), rect)
                        .expect("picked point remains visible");
                    assert!(
                        projected.distance(cursor) < 0.01,
                        "{lat}, {lon}, heading {heading}, alt {alt}, aspect {aspect}: {projected:?} != {cursor:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn projection_preserves_north_east_and_heading_without_mirroring() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        for heading in [0.0, 90.0] {
            let basis = Cam {
                lat: 0.0,
                lon: 0.0,
                heading,
                alt: 1000.0,
                tilt: 0.0,
            }
            .basis(4.0 / 3.0);
            let center = basis.project(ll_to_v3(0.0, 0.0), rect).unwrap();
            let north = basis.project(ll_to_v3(1.0, 0.0), rect).unwrap();
            let east = basis.project(ll_to_v3(0.0, 1.0), rect).unwrap();
            if heading == 0.0 {
                assert!(north.y < center.y);
                assert!(east.x > center.x);
            } else {
                assert!(north.x < center.x);
                assert!(east.y < center.y);
            }
        }
    }

    #[test]
    fn horizon_visibility_and_ray_directions_hold_at_altitude_limits() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        for alt in [MIN_CAMERA_ALTITUDE_KM, MAX_CAMERA_ALTITUDE_KM] {
            let basis = Cam {
                lat: 40.0,
                lon: 170.0,
                heading: 75.0,
                alt,
                tilt: 0.0,
            }
            .basis(4.0 / 3.0);
            let center = ll_to_v3(40.0, 170.0);
            assert!(basis.project(center, rect).is_some());
            assert!(basis.project(-center, rect).is_none());
            assert!(basis.dir(0.0, 0.0).abs_diff_eq(-center, 1e-12));
            for (x, y) in [(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)] {
                assert!((basis.dir(x, y).length_squared() - 1.0).abs() < 1e-12);
            }
        }
    }
}

impl Cam {
    /// Independent body orbit; one viewport height spans a half-turn.
    pub fn orbit_body(&mut self, dx_points: f64, dy_points: f64, height_points: f32) {
        let sensitivity = 180.0 / height_points.max(1.0) as f64;
        self.lon = wrap_lon(self.lon - dx_points * sensitivity);
        self.lat = (self.lat + dy_points * sensitivity)
            .clamp(-MAX_CAMERA_LATITUDE_DEG, MAX_CAMERA_LATITUDE_DEG);
    }
    pub fn zoom_fraction(&self) -> f32 {
        let (min, max) = (MIN_CAMERA_ALTITUDE_KM.ln(), MAX_CAMERA_ALTITUDE_KM.ln());
        ((self.alt.ln() - min) / (max - min)).clamp(0.0, 1.0) as f32
    }
    pub fn set_zoom_fraction(&mut self, fraction: f64) {
        let (min, max) = (MIN_CAMERA_ALTITUDE_KM.ln(), MAX_CAMERA_ALTITUDE_KM.ln());
        self.alt = (min + fraction.clamp(0.0, 1.0) * (max - min))
            .exp()
            .clamp(MIN_CAMERA_ALTITUDE_KM, MAX_CAMERA_ALTITUDE_KM);
    }
}

#[cfg(test)]
mod body_control_tests {
    use super::*;
    #[test]
    fn body_orbit_wraps_and_clamps_without_ground_pan() {
        let mut cam = Cam {
            lat: 0.0,
            lon: 170.0,
            alt: 14_000.0,
            heading: 45.0,
            tilt: 0.0,
        };
        cam.orbit_body(-100.0, 500.0, 100.0);
        assert_eq!(cam.lon, -10.0);
        assert_eq!(cam.lat, MAX_CAMERA_LATITUDE_DEG);
        assert_eq!(cam.heading, 45.0);
    }
    #[test]
    fn slider_round_trip_and_endpoints() {
        let mut cam = Cam {
            lat: 0.0,
            lon: 0.0,
            alt: 14_000.0,
            heading: 0.0,
            tilt: 0.0,
        };
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            cam.set_zoom_fraction(fraction);
            assert!((cam.zoom_fraction() as f64 - fraction).abs() < 1e-6);
        }
        cam.set_zoom_fraction(-1.0);
        assert!((cam.alt - MIN_CAMERA_ALTITUDE_KM).abs() < 1e-12);
        cam.set_zoom_fraction(2.0);
        assert!((cam.alt - MAX_CAMERA_ALTITUDE_KM).abs() < 1e-8);
    }

    #[test]
    fn tilting_close_to_the_ground_zooms_out_instead_of_locking() {
        let mut cam = Cam {
            lat: 47.42,
            lon: 10.98,
            heading: 0.0,
            alt: MIN_TILTED_HEIGHT_KM,
            tilt: 0.0,
        };
        // Clamped tilting stays locked at this distance.
        cam.set_tilt(45.0);
        assert_eq!(cam.tilt, 0.0);
        cam.tilt_zooming_out(45.0);
        assert!((cam.tilt - 45.0).abs() < 1e-9, "{}", cam.tilt);
        let height = cam.alt * cam.tilt.to_radians().cos();
        assert!((height - MIN_TILTED_HEIGHT_KM).abs() < 1e-6, "{height}");
        // The steepest tilt, and never past it.
        cam.tilt_zooming_out(120.0);
        assert!((cam.tilt - MAX_TILT_DEG).abs() < 1e-9);
        // Far enough away, tilting leaves the distance alone.
        let mut far = Cam { alt: 500.0, ..cam };
        far.tilt_zooming_out(30.0);
        assert_eq!((far.alt, far.tilt), (500.0, 30.0));
    }
}
