//! Looking around from the eye: the view turns about the camera instead of about the
//! point it looks at, as a person turns their head (user, 2026-11-18: "rotate in
//! place", a feature of its own beside turning and tilting the globe).
use super::{Cam, MAX_LOOK_UP_DEG, MAX_TILT_DEG};
use crate::earth_model::EarthModel;
use crate::math::{ll_to_v3, wrap_lon};
use crate::{EARTH_RADIUS_KM, MAX_CAMERA_LATITUDE_DEG, MIN_CAMERA_ALTITUDE_KM};
use glam::{DQuat, DVec3};

/// A line of sight this close to straight down keeps the heading it had (its
/// direction along the ground is not defined).
const LEVEL_SIGHT_EPSILON: f64 = 1e-9;
/// How far below the horizon the view can be lowered: straight down, less a little
/// so that the direction it looks along the ground stays defined.
const STRAIGHT_DOWN_DEG: f64 = 89.99;
/// A drag along a line of sight within about 6° of straight down does not turn the
/// view (the sine of that angle).
const NEAR_VERTICAL_SINE: f64 = 0.1;
/// Times the point where the line of sight meets the raised ground is found again.
const GROUND_REFINEMENTS: usize = 3;
/// Halvings that find how far a raise can lean the view over the ground before the
/// rest raises it above (a hundredth of a degree for a raise of 30°).
const RAISE_HALVINGS: usize = 12;

impl Cam {
    /// Turns the view about the eye: `turn_deg` to the right, `raise_deg` up. The eye
    /// stays where it is. While the line of sight meets the ground, the looked-at
    /// point becomes where it meets it, `ground_km` above the surface (the relief's
    /// height under the old point); past the steepest tilt, or where the line would
    /// miss the ground, the view is raised about the eye instead (`look_up`): up to
    /// the sky (roadmap C8). Lowered, the view comes down from the sky first. Returns
    /// false, changing nothing, when even the turn alone cannot be made.
    pub fn look_around(
        &mut self,
        turn_deg: f64,
        raise_deg: f64,
        model: EarthModel,
        ground_km: f64,
    ) -> bool {
        if !turn_deg.is_finite() || !raise_deg.is_finite() {
            return false;
        }
        let mut orbit = Cam {
            look_up: 0.0,
            ..*self
        };
        let looked_up = self.look_up.clamp(0.0, MAX_LOOK_UP_DEG);
        let raised = looked_up + raise_deg;
        // Above the ground's view already, or come down only part of the way.
        if looked_up > 0.0 && raised >= 0.0 {
            if turn_deg != 0.0 && !orbit.look_over_ground(turn_deg, 0.0, model, ground_km) {
                return false;
            }
            orbit.look_up = raised.min(orbit.max_look_up());
            *self = orbit;
            return true;
        }
        // From the sky down onto the ground, or over the ground all along.
        let over_ground = if looked_up > 0.0 { raised } else { raise_deg };
        if orbit.look_over_ground(turn_deg, over_ground, model, ground_km) {
            *self = orbit;
            return true;
        }
        if over_ground <= 0.0 {
            return false;
        }
        // Raised past what the ground allows: as far as it does, the rest about the eye.
        let (mut low, mut high) = (0.0, over_ground);
        let mut best = orbit;
        if !best.look_over_ground(turn_deg, 0.0, model, ground_km) {
            return false;
        }
        for _ in 0..RAISE_HALVINGS {
            let middle = 0.5 * (low + high);
            let mut tried = orbit;
            if tried.look_over_ground(turn_deg, middle, model, ground_km) {
                (low, best) = (middle, tried);
            } else {
                high = middle;
            }
        }
        best.look_up = (over_ground - low).min(best.max_look_up());
        *self = best;
        true
    }

    /// [`Self::look_around`] for a view on the ground (`look_up` zero): false where
    /// the new line of sight misses the ground, leans past the steepest tilt there or
    /// comes closer than the closest camera distance.
    fn look_over_ground(
        &mut self,
        turn_deg: f64,
        raise_deg: f64,
        model: EarthModel,
        ground_km: f64,
    ) -> bool {
        let basis = self.basis_over_ground(1.0, model, ground_km);
        let geometry = model.geometry();
        let eye = basis.c;
        let rotation = basis.rotation_from_view();
        // Turning to the right is clockwise seen from above.
        let vertical = geometry.normal(eye).normalize();
        let turn = DQuat::from_axis_angle(vertical, -turn_deg.to_radians());
        let right = turn * (rotation * DVec3::X);
        let forward = turn * (rotation * DVec3::NEG_Z);
        // Lowered at most to straight down: past it the view would flip over to the
        // other side (user, 2026-11-18).
        let below_deg = (-forward.dot(vertical))
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees();
        let raise_deg = raise_deg.max(below_deg - STRAIGHT_DOWN_DEG);
        let sight = (DQuat::from_axis_angle(right.normalize(), raise_deg.to_radians()) * forward)
            .normalize();
        // The ground there is the surface raised along its normal: met by the line from
        // the eye lowered as far along the normal where it lands (found again from
        // where the last try landed; it settles within a few).
        let raise = ground_km / EARTH_RADIUS_KM;
        let mut up = ll_to_v3(self.lat, self.lon);
        let mut found = None;
        for _ in 0..GROUND_REFINEMENTS {
            let Some(base) = geometry.intersect(eye - up * raise, sight) else {
                return false;
            };
            let (lat, lon) = geometry.coordinates(base);
            up = ll_to_v3(lat, lon);
            found = Some((base, lat, lon));
        }
        let Some((base, lat, lon)) = found else {
            return false;
        };
        if lat.abs() > MAX_CAMERA_LATITUDE_DEG {
            return false;
        }
        let target = base + up * raise;
        let alt = eye.distance(target) * EARTH_RADIUS_KM;
        let tilt = (-sight).dot(up).clamp(-1.0, 1.0).acos().to_degrees();
        let along = sight - up * sight.dot(up);
        let heading = if along.length() < LEVEL_SIGHT_EPSILON {
            self.heading + turn_deg
        } else {
            let (la, lo) = (lat.to_radians(), lon.to_radians());
            let east = DVec3::new(-lo.sin(), lo.cos(), 0.0);
            let north = DVec3::new(-la.sin() * lo.cos(), -la.sin() * lo.sin(), la.cos());
            along.dot(east).atan2(along.dot(north)).to_degrees()
        };
        let next = Cam {
            lat,
            lon,
            heading: wrap_lon(heading),
            alt,
            tilt,
            look_up: 0.0,
        };
        if alt < MIN_CAMERA_ALTITUDE_KM || tilt > next.max_tilt().min(MAX_TILT_DEG) {
            return false;
        }
        *self = next;
        true
    }
}

impl Cam {
    /// Looks around so that what was seen along `from` is seen along `to` (unit
    /// world directions, as `Basis::ray` gives them for two points on the screen): a
    /// drag holds the picture under the pointer (user, 2026-11-18). The turn about the
    /// eye's vertical is exact, the raise as near as a raise along the view's middle
    /// gives. Whether the view moved (see [`Self::look_around`]).
    pub fn look_drag(&mut self, from: DVec3, to: DVec3, model: EarthModel, ground_km: f64) -> bool {
        let eye = self.basis_over_ground(1.0, model, ground_km).c;
        let up = model.geometry().normal(eye).normalize();
        // Any two directions across the vertical measure the bearing.
        let across = if up.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
        let east = across.cross(up).normalize();
        let north = up.cross(east);
        let bearing = |d: DVec3| d.dot(east).atan2(d.dot(north)).to_degrees();
        let elevation = |d: DVec3| d.dot(up).clamp(-1.0, 1.0).asin().to_degrees();
        // Close to straight down a bearing swings wildly with a small move: the drag
        // only raises or lowers there.
        let level = |d: DVec3| (d - up * d.dot(up)).length() >= NEAR_VERTICAL_SINE;
        let turn = if level(from) && level(to) {
            wrap_lon(bearing(from) - bearing(to))
        } else {
            0.0
        };
        let raise = elevation(from) - elevation(to);
        self.look_around(turn, raise, model, ground_km)
            || (raise != 0.0 && turn != 0.0 && self.look_around(turn, 0.0, model, ground_km))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye(cam: &Cam, model: EarthModel, ground_km: f64) -> DVec3 {
        cam.basis_over_ground(1.0, model, ground_km).c
    }

    #[test]
    fn the_eye_stays_while_the_view_turns_and_leans() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let ground_km = 1.2;
            let mut cam = Cam {
                lat: 46.5,
                lon: 7.9,
                heading: 30.0,
                alt: 3.0,
                tilt: 60.0,
                look_up: 0.0,
            };
            let start = eye(&cam, model, ground_km);
            for (turn, raise) in [(20.0, 0.0), (0.0, 5.0), (-45.0, -10.0), (90.0, 3.0)] {
                let before = cam;
                assert!(
                    cam.look_around(turn, raise, model, ground_km),
                    "{turn} {raise}"
                );
                // Within a millimetre.
                let moved = eye(&cam, model, ground_km).distance(start) * EARTH_RADIUS_KM;
                assert!(moved < 1e-6, "{model:?} {turn} {raise}: {moved} km");
                // A turn turns the heading by as much, a raise leans the view further,
                // both roughly: the heading is a bearing at the looked-at point, which
                // moves over the curved Earth.
                if raise == 0.0 {
                    assert!(wrap_lon(cam.heading - before.heading - turn).abs() < 0.5);
                }
                if turn == 0.0 {
                    assert!(wrap_lon(cam.heading - before.heading).abs() < 0.5);
                    assert!(cam.tilt > before.tilt);
                }
            }
        }
    }

    #[test]
    fn a_drag_holds_the_picture_under_the_pointer() {
        let model = EarthModel::Wgs84;
        let mut cam = Cam {
            lat: 45.0,
            lon: 6.0,
            heading: 10.0,
            alt: 5.0,
            tilt: 65.0,
            look_up: 0.0,
        };
        let rect = emath::Rect::from_min_size(emath::Pos2::ZERO, emath::vec2(1200.0, 800.0));
        let (start, end) = (emath::pos2(500.0, 450.0), emath::pos2(540.0, 430.0));
        let before = cam.basis_over_ground(1.5, model, 0.0);
        let seen = before.ray(rect, start);
        assert!(cam.look_drag(seen, before.ray(rect, end), model, 0.0));
        let after = cam.basis_over_ground(1.5, model, 0.0);
        // What was under the pointer is under it still, within a pixel.
        let at = after.project_precise(after.c + seen * 1e-4, rect).unwrap();
        let off = (at - glam::DVec2::new(f64::from(end.x), f64::from(end.y))).length();
        assert!(off < 1.0, "{off} px");
    }

    #[test]
    fn lowering_the_view_stops_at_straight_down() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let mut cam = Cam {
                lat: 47.0,
                lon: 11.0,
                heading: 40.0,
                alt: 3.0,
                tilt: 20.0,
                look_up: 0.0,
            };
            // Far past straight down, in one go and in small steps.
            assert!(cam.look_around(0.0, -60.0, model, 0.5));
            assert!(cam.tilt < 0.05, "{}", cam.tilt);
            assert!(wrap_lon(cam.heading - 40.0).abs() < 1.0, "{}", cam.heading);
            for _ in 0..20 {
                cam.look_around(0.0, -2.0, model, 0.5);
                assert!(cam.tilt < 0.05 && wrap_lon(cam.heading - 40.0).abs() < 1.0);
            }
            // Raised again it leans the same way it did.
            assert!(cam.look_around(0.0, 10.0, model, 0.5));
            assert!(wrap_lon(cam.heading - 40.0).abs() < 1.0, "{}", cam.heading);
        }
    }

    #[test]
    fn the_tilt_controls_lean_on_into_the_sky_and_come_down_from_it_first() {
        let model = EarthModel::Wgs84;
        let mut cam = Cam {
            lat: 10.0,
            lon: 20.0,
            heading: 0.0,
            alt: 2.0,
            tilt: 80.0,
            look_up: 0.0,
        };
        // Leaning past the steepest tilt raises the view about the eye.
        cam.tilt_view(100.0);
        assert_eq!(cam.tilt, MAX_TILT_DEG);
        assert!((cam.look_up - 15.0).abs() < 1e-9);
        assert!((cam.view_tilt() - 100.0).abs() < 1e-9);
        // After a look around into the sky, lowering takes the look up away first.
        assert!(cam.look_around(0.0, 10.0, model, 0.0));
        let (tilt, raised) = (cam.tilt, cam.view_tilt());
        cam.tilt_view(raised - 5.0);
        assert_eq!(cam.tilt, tilt);
        assert!((cam.view_tilt() - (raised - 5.0)).abs() < 1e-9);
        cam.tilt_view(30.0);
        assert_eq!((cam.tilt, cam.look_up), (30.0, 0.0));
        cam.tilt_view(f64::NAN);
        assert_eq!(cam.tilt, 30.0);
    }

    #[test]
    fn far_out_the_view_stays_level_however_it_is_leaned() {
        let model = EarthModel::Wgs84;
        let mut cam = Cam {
            lat: 10.0,
            lon: 20.0,
            heading: 0.0,
            alt: 20_000.0,
            tilt: 0.0,
            look_up: 0.0,
        };
        // Beyond the level view's altitude neither the tilt nor a look up moves it.
        assert_eq!(cam.max_view_tilt(), 0.0);
        cam.tilt_view(150.0);
        assert_eq!((cam.tilt, cam.look_up), (0.0, 0.0));
        let _ = cam.look_around(0.0, 40.0, model, 0.0);
        assert_eq!(cam.look_up, 0.0);
        // Between, both ease out together; close in, the whole lean is there.
        cam.alt = 10_000.0;
        let between = cam.max_view_tilt();
        assert!(between > 0.0 && between < MAX_TILT_DEG + MAX_LOOK_UP_DEG);
        cam.tilt_view(175.0);
        assert!((cam.view_tilt() - between).abs() < 1e-6, "{cam:?}");
        cam.alt = 50.0;
        assert_eq!(cam.max_view_tilt(), MAX_TILT_DEG + MAX_LOOK_UP_DEG);
        // Zooming out from a raised view lowers it with the tilt.
        cam.tilt_view(150.0);
        cam.alt = 20_000.0;
        cam.clamp_tilt();
        assert_eq!((cam.tilt, cam.look_up), (0.0, 0.0));
    }

    #[test]
    fn looking_above_the_horizon_raises_the_view_about_the_eye() {
        let model = EarthModel::Wgs84;
        let mut cam = Cam {
            lat: 10.0,
            lon: 20.0,
            heading: 0.0,
            alt: 2.0,
            tilt: 80.0,
            look_up: 0.0,
        };
        let start = eye(&cam, model, 0.0);
        let up = model.geometry().normal(start).normalize();
        // The line of sight's angle above the horizon at the eye (degrees).
        let elevation = |cam: &Cam| {
            let forward = cam.basis_over_ground(1.0, model, 0.0).dir(0.0, 0.0);
            forward.dot(up).asin().to_degrees()
        };
        let level = elevation(&cam);
        assert!(level < 0.0, "{level}");
        assert!(cam.look_around(0.0, 30.0, model, 0.0));
        assert!(cam.look_up > 0.0, "{cam:?}");
        assert!(
            (elevation(&cam) - level - 30.0).abs() < 0.1,
            "{}",
            elevation(&cam)
        );
        // Turned while looking up, and lowered back: the eye stays all along.
        assert!(cam.look_around(40.0, 0.0, model, 0.0));
        assert!((elevation(&cam) - level - 30.0).abs() < 0.1);
        assert!(cam.look_around(0.0, -30.0, model, 0.0));
        assert_eq!(cam.look_up, 0.0);
        assert!((elevation(&cam) - level).abs() < 0.1, "{}", elevation(&cam));
        let moved = eye(&cam, model, 0.0).distance(start) * EARTH_RADIUS_KM;
        assert!(moved < 1e-6, "{moved} km");
        // No further than near the zenith, and nothing for a bad angle.
        assert!(cam.look_around(0.0, 500.0, model, 0.0));
        assert_eq!(cam.look_up, MAX_LOOK_UP_DEG);
        assert!(!cam.look_around(f64::NAN, 0.0, model, 0.0));
    }
}
