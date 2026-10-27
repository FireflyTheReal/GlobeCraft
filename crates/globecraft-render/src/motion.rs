//! How often live objects (aircraft, vessels, satellites) are moved on screen
//! (roadmap 1.8). Positions are dead-reckoned from time-stamped samples; this module
//! picks the time step so the fastest object of a layer moves about half a point per
//! step: frame rate when zoomed in, about once a second from orbit, where nothing
//! visibly moves faster and re-projecting thousands of objects would only cost time.
use globecraft_core::VERTICAL_FOV_HALF_TAN;
use std::time::Duration;

/// The finest step: about frame rate.
pub const MIN_STEP_SECS: f64 = 1.0 / 60.0;
/// The coarsest step, used when objects move less than a point a second.
pub const MAX_STEP_SECS: f64 = 1.0;
/// How far the fastest object may move between two steps.
const STEP_MOTION_POINTS: f64 = 0.5;

/// Typical fast ground speeds per layer, km/s.
pub const AIRCRAFT_SPEED_KM_S: f64 = 0.28;
pub const VESSEL_SPEED_KM_S: f64 = 0.015;
pub const SATELLITE_SPEED_KM_S: f64 = 7.8;

/// The opacity of the oldest samples: dimmed, never hidden, so a quiet transponder
/// still shows where the object was last seen.
pub const STALE_OPACITY: f32 = 0.35;

/// Full opacity while a sample is up to `fresh_secs` old (dead reckoning still
/// trusts it), then fading linearly to [`STALE_OPACITY`] at `stale_secs`.
pub fn age_opacity(age_secs: f64, fresh_secs: f64, stale_secs: f64) -> f32 {
    let fade = ((age_secs - fresh_secs) / (stale_secs - fresh_secs).max(1e-9)).clamp(0.0, 1.0);
    1.0 - (1.0 - STALE_OPACITY) * fade as f32
}

/// Kilometres per point at the centre of a view `height_points` tall, `altitude_km`
/// above the ground. Everything visible is at least that far away, so objects there
/// move at most this fast on screen.
pub fn km_per_point(altitude_km: f64, height_points: f32) -> f64 {
    altitude_km.max(0.0) * 2.0 * VERTICAL_FOV_HALF_TAN / f64::from(height_points.max(1.0))
}

/// The step for objects moving at up to `speed_km_s`.
pub fn step_secs(altitude_km: f64, height_points: f32, speed_km_s: f64) -> f64 {
    let points_per_sec = speed_km_s / km_per_point(altitude_km, height_points).max(1e-9);
    (STEP_MOTION_POINTS / points_per_sec.max(1e-9)).clamp(MIN_STEP_SECS, MAX_STEP_SECS)
}

/// The index of the step `unix_secs` falls in; two times in the same step show
/// objects at the same place.
pub fn tick(unix_secs: f64, step_secs: f64) -> i64 {
    (unix_secs / step_secs.max(MIN_STEP_SECS)).floor() as i64
}

/// When to repaint again for objects stepping every `step_secs`.
pub fn repaint_after(step_secs: f64) -> Duration {
    Duration::from_secs_f64(step_secs.clamp(MIN_STEP_SECS, MAX_STEP_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_step_at_frame_rate_close_up_and_once_a_second_from_orbit() {
        // A street view: an airliner crosses many points a second.
        assert_eq!(step_secs(2.0, 800.0, AIRCRAFT_SPEED_KM_S), MIN_STEP_SECS);
        // The whole globe: aircraft and ships barely move.
        assert_eq!(
            step_secs(14_000.0, 800.0, AIRCRAFT_SPEED_KM_S),
            MAX_STEP_SECS
        );
        assert_eq!(step_secs(500.0, 800.0, VESSEL_SPEED_KM_S), MAX_STEP_SECS);
        // A region: satellites need several steps a second.
        let satellites = step_secs(1_000.0, 800.0, SATELLITE_SPEED_KM_S);
        assert!(satellites < 0.25 && satellites > MIN_STEP_SECS);
    }

    #[test]
    fn old_samples_dim_but_stay_visible() {
        assert_eq!(age_opacity(10.0, 25.0, 90.0), 1.0);
        let middle = age_opacity(57.5, 25.0, 90.0);
        assert!(middle < 1.0 && middle > STALE_OPACITY);
        assert!((age_opacity(600.0, 25.0, 90.0) - STALE_OPACITY).abs() < 1e-6);
    }

    #[test]
    fn times_within_a_step_share_a_tick() {
        assert_eq!(tick(10.01, 0.1), tick(10.09, 0.1));
        assert_ne!(tick(10.09, 0.1), tick(10.11, 0.1));
    }
}
