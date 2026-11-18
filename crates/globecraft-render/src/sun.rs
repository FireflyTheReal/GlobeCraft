//! The sun in the sky: its disc and glow, drawn into the frame's sky pixels through
//! the air in front of them. So the ground and mountains hide it, the air dims and
//! reddens it as it sets, and it shows by day over a bright sky, where the stars (drawn
//! behind the frame) go out (user, 2026-11-18: "sun needs to be affected by
//! atmosphere", "sunset look physical").
use glam::{DVec3, Vec3};

/// The sun's angular radius (radians).
const SUN_RADIUS_RAD: f64 = 0.004_65;
/// Its colour as drawn: white, a little warm.
const SUN_COLOUR: Vec3 = Vec3::new(1.0, 0.97, 0.9);
/// The glow around the disc, as an eye or a camera spreads a bright light: a bright
/// near part and a faint wide one, each its light at the disc's edge and the angle
/// (radians) over which it falls by e.
const SUN_GLOW_NEAR: (f32, f64) = (0.5, 0.012);
const SUN_GLOW_WIDE: (f32, f64) = (0.12, 0.09);
/// Past this angle from the sun (radians) its glow is left out.
const SUN_GLOW_REACH_RAD: f64 = 0.45;
/// The sun is far brighter than anything else on the screen: the light that crosses
/// the air still fills the screen's range many times over. Each colour is drawn as
/// this many times what arrives of it, at most full: white when high, yellow, orange
/// and red as the air between grows and takes first blue, then green.
const SUN_BRIGHTNESS: f32 = 8.0;

/// The sun's light `angle` radians from its centre (0 to 1): its disc, then its glow.
fn sun_light(angle: f64) -> f32 {
    if angle <= SUN_RADIUS_RAD {
        return 1.0;
    }
    if angle >= SUN_GLOW_REACH_RAD {
        return 0.0;
    }
    let past = angle - SUN_RADIUS_RAD;
    let glow = |(light, fall): (f32, f64)| light * (-past / fall).exp() as f32;
    (glow(SUN_GLOW_NEAR) + glow(SUN_GLOW_WIDE)).min(1.0)
}

/// The sun's light (as drawn, 0 to 1) along a sky ray `direction` toward a sun in
/// direction `sun`, through air that lets `transmitted` of each colour pass.
pub(crate) fn sun_in_sky(direction: DVec3, sun: DVec3, transmitted: Vec3) -> Vec3 {
    if direction.dot(sun) <= SUN_GLOW_REACH_RAD.cos() {
        return Vec3::ZERO;
    }
    let colour = (transmitted * SUN_BRIGHTNESS).min(Vec3::ONE);
    SUN_COLOUR * colour * sun_light(direction.angle_between(sun))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_is_a_full_disc_in_a_glow_that_fades_out() {
        assert_eq!(sun_light(0.0), 1.0);
        assert_eq!(sun_light(SUN_RADIUS_RAD), 1.0);
        let mut last = 1.0;
        for step in 1..100 {
            let light = sun_light(SUN_RADIUS_RAD + f64::from(step) * 0.005);
            assert!(light <= last);
            last = light;
        }
        assert!(sun_light(0.02) < 0.5);
        assert!(sun_light(SUN_GLOW_REACH_RAD - 1e-6) < 0.01);
        assert_eq!(sun_light(SUN_GLOW_REACH_RAD), 0.0);
    }

    #[test]
    fn the_air_reddens_the_setting_sun() {
        // Seen from the ground at sea level, through the frame's own air.
        let up = DVec3::Z;
        let air = crate::atmosphere::Frame::new(up * 1.000_01, 1.000_01, None, false);
        let at = |elevation_deg: f64| {
            let e = elevation_deg.to_radians();
            let direction = DVec3::new(e.cos(), 0.0, e.sin());
            let through = air.sky(direction, e.sin() as f32).unwrap().transmitted;
            sun_in_sky(direction, direction, through.truncate())
        };
        let (high, low, setting) = (at(60.0), at(6.0), at(0.5));
        println!("60°: {high}  6°: {low}  0.5°: {setting}");
        // High: white. Low: yellow to orange. Setting: red.
        assert!(high.min_element() > 0.85, "{high}");
        assert!(low.x > 0.95 && low.z < 0.7 * low.x, "{low}");
        assert!(
            setting.x > 0.4 && setting.y < 0.5 * setting.x && setting.z < 0.1,
            "{setting}"
        );
    }
}
