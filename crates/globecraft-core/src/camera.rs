use crate::earth_model::EarthModel;
use crate::math::{ll_to_v3, v3_to_ll, wrap_lon};
use crate::{
    EARTH_RADIUS_KM, MAX_CAMERA_ALTITUDE_KM, MIN_CAMERA_ALTITUDE_KM, VERTICAL_FOV_HALF_TAN,
};
use crate::{MAX_CAMERA_LATITUDE_DEG, METERS_PER_KM, METERS_PER_LATITUDE_DEGREE};
use emath::{Pos2, Rect};
use glam::{DMat2, DMat3, DMat4, DQuat, DVec2, DVec3, dcamera::rh};
// Reject near-zero projection depths and rotation axes; bound pan amplification near poles.
const PROJECTION_DEPTH_EPSILON: f64 = 1e-9;
const ROTATION_AXIS_EPSILON: f64 = 1e-13;
const MIN_PAN_LATITUDE_COSINE: f64 = 0.05;
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
        Basis {
            c: target + back * (self.alt / EARTH_RADIUS_KM),
            model,
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
        let horizon_camera = if self.model == EarthModel::PerfectSphere {
            self.c
        } else {
            self.c * self.model.geometry().inverse_axes_squared()
        };
        if p.dot(horizon_camera) < 1.0 {
            return None; // behind the horizon
        }
        let v = p - self.c;
        let local = self.view_from_world * v;
        let z = -local.z;
        if z <= PROJECTION_DEPTH_EPSILON {
            return None;
        }
        let ndc = self.projection.project_point3(local);
        Some(Pos2::new(
            rect.center().x + ndc.x as f32 * rect.width() * 0.5,
            rect.center().y - ndc.y as f32 * rect.height() * 0.5,
        ))
    }
    pub fn pick(&self, rect: Rect, pos: Pos2) -> Option<(f64, f64)> {
        let nx = ((pos.x - rect.min.x) / rect.width()) as f64 * 2.0 - 1.0;
        let ny = 1.0 - ((pos.y - rect.min.y) / rect.height()) as f64 * 2.0;
        let d = self.dir(nx, ny);
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
pub const MAX_TILT_DEG: f64 = 80.0;
/// A tilted camera keeps at least this height above the ground point.
const MIN_TILTED_HEIGHT_KM: f64 = 0.3;

impl Cam {
    /// The steepest tilt at this distance: `MAX_TILT_DEG`, less when the camera would
    /// otherwise come closer to the ground than `MIN_TILTED_HEIGHT_KM`.
    pub fn max_tilt(&self) -> f64 {
        let height_ratio = (MIN_TILTED_HEIGHT_KM / self.alt.max(MIN_TILTED_HEIGHT_KM)).min(1.0);
        height_ratio.acos().to_degrees().min(MAX_TILT_DEG)
    }
    /// Sets the tilt, kept between straight down and `max_tilt`.
    pub fn set_tilt(&mut self, degrees: f64) {
        if degrees.is_finite() {
            self.tilt = degrees.clamp(0.0, self.max_tilt());
        }
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
        self.lat = (self.lat + dn).clamp(-MAX_CAMERA_LATITUDE_DEG, MAX_CAMERA_LATITUDE_DEG);
        self.lon =
            wrap_lon(self.lon + de / self.lat.to_radians().cos().max(MIN_PAN_LATITUDE_COSINE));
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
        if model == EarthModel::PerfectSphere {
            self.zoom_at(factor, cursor, rect, aspect);
            return;
        }
        let before = cursor.and_then(|c| self.basis_for_model(aspect, model).pick(rect, c));
        self.zoom(factor);
        if let (Some(p0), Some(cursor)) = (before, cursor) {
            self.anchor_keeping_heading(p0, |cam| {
                cam.basis_for_model(aspect, model).pick(rect, cursor)
            });
        }
    }

    /// Moves the camera so `pick` (the point under the cursor) returns `target` again,
    /// keeping the heading: zooming toward a point never turns north away (user,
    /// 2026-10-25). Each step turns the camera rigidly toward the target, then restores
    /// the heading; the next step corrects what that moved. Where a fixed heading cannot
    /// hold the point exactly (near the globe's edge, or the pole), the closest match is
    /// kept: north stays locked and the point may slip by a little.
    fn anchor_keeping_heading(
        &mut self,
        target: (f64, f64),
        pick: impl Fn(&Self) -> Option<(f64, f64)>,
    ) {
        const MAX_ANCHOR_ITERATIONS: usize = 24;
        const ANCHOR_TOLERANCE_KM: f64 = 1e-8;
        /// Within a degree of a pole north points every way: the camera turns rigidly.
        const FREE_HEADING_MIN_LATITUDE_DEG: f64 = 89.0;
        let heading = self.heading;
        let keep_heading = self.lat.abs() < FREE_HEADING_MIN_LATITUDE_DEG;
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
        if let Some((_, cam)) = best {
            *self = cam;
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
        // Zooming all the way in keeps the tilt and stops at the minimum height.
        for _ in 0..200 {
            cam.zoom(0.8);
        }
        assert_eq!(cam.tilt, 60.0);
        assert!((cam.alt * cam.tilt.to_radians().cos() - MIN_TILTED_HEIGHT_KM).abs() < 1e-9);
        cam.zoom(2.0);
        assert_eq!(cam.tilt, 60.0);
        // Very close in, the camera may not dip below the minimum height.
        cam.alt = 0.4;
        cam.set_tilt(80.0);
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
        assert_eq!(cam.lat, 89.9);
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
                for fraction in [0.45, 0.5, 0.55] {
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
}
