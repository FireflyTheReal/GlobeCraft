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

/// The water's glint: its shine (0 to 1, `LiveMaps::water`) times a highlight around
/// the sun's mirror image, broad as the waves make it (Blinn's half-vector form).
pub(crate) const GLINT_STRENGTH: f32 = 0.5;
pub(crate) const GLINT_SHININESS: f32 = 60.0;
/// A highlight fainter than this is left out (it would not show; it saves a lookup).
pub(crate) const GLINT_MIN_HIGHLIGHT: f32 = 1e-4;
/// The glint fades out as the sun sinks to the water's horizon: from all with the sun
/// this high (its cosine to the vertical, about 3°) to none at the horizon. It ended
/// at the horizon in a hard line across the sunset's path (user, 2026-11-18).
pub(crate) const GLINT_SUNSET_COSINE: f32 = 0.05;

/// The sun's glint (0 to 1) off the water at a place, seen along `direction` where
/// the surface's normal is `normal`.
pub(crate) fn glint(
    water: &image::GrayImage,
    lat: f64,
    lon: f64,
    direction: DVec3,
    normal: DVec3,
    sun: DVec3,
) -> f32 {
    // The glint fades as the sun sets where it shines, instead of ending in a line:
    // seen from above the ground, the sun stays up past where it set below.
    let up = (normal.dot(sun) as f32 / GLINT_SUNSET_COSINE).clamp(0.0, 1.0);
    if up <= 0.0 {
        return 0.0;
    }
    let half = (sun - direction).normalize();
    let facing = normal.dot(half).max(0.0) as f32;
    let highlight = facing.powf(GLINT_SHININESS);
    if highlight < GLINT_MIN_HIGHLIGHT {
        return 0.0;
    }
    let setting = up * up * (3.0 - 2.0 * up);
    crate::clouds::sample(water, lat, lon) * GLINT_STRENGTH * highlight * setting
}

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
    fn the_glint_shines_where_the_sun_is_mirrored_and_only_on_water() {
        let water = image::GrayImage::from_fn(360, 180, |x, _| {
            image::Luma([if x < 180 { 255 } else { 0 }])
        });
        // Looking straight down at the equator with the sun overhead: full glint on the
        // water (west), none on land (east), none with the sun to the side.
        let (normal, down) = (DVec3::X, -DVec3::X);
        let full = glint(&water, 0.0, -10.0, down, normal, DVec3::X);
        assert!((full - GLINT_STRENGTH).abs() < 1e-3, "{full}");
        assert_eq!(glint(&water, 0.0, 10.0, down, normal, DVec3::X), 0.0);
        let aside = DVec3::new(0.6, 0.8, 0.0);
        assert!(glint(&water, 0.0, -10.0, down, normal, aside) < 0.01);
        // Below the horizon there is none, and toward it the glint fades.
        assert_eq!(glint(&water, 0.0, -10.0, down, normal, -DVec3::X), 0.0);
        let low = |elevation_deg: f64| {
            let e = elevation_deg.to_radians();
            let sun = DVec3::new(e.sin(), e.cos(), 0.0);
            // Looking along the sun's mirror line.
            let view = DVec3::new(-e.sin(), e.cos(), 0.0);
            glint(&water, 0.0, -10.0, view, normal, sun)
        };
        assert_eq!(low(0.0), 0.0);
        assert!(0.0 < low(1.0) && low(1.0) < low(2.0) && low(2.0) < low(4.0));
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
