//! Project actual flight telemetry on the same globe as the base imagery.
use emath::{Pos2, Rect, Vec2};
use globecraft_core::camera::Basis;
#[cfg(test)]
use globecraft_core::math::destination;
use globecraft_data::aircraft::Aircraft;
#[cfg(test)]
mod benchmarks;
mod worker;
pub use worker::{
    AircraftGeometry, AircraftPreparer, AircraftWorker, ProjectionInput, ProjectionView,
};

const ICON_SIZE_POINTS: f32 = 7.0;
const TRAIL_SIZE_POINTS: f32 = 12.0;
const WING_WIDTH_RATIO: f32 = 0.4;
/// Flights keep flying their last track at their last ground speed for this long:
/// as long as the global feed keeps a report (it is fetched every 17 minutes and
/// kept for 20), so planes never stand still between reports.
const EXTRAPOLATION_MAX_SECS: f64 = 20.0 * 60.0;
/// Reports up to this old are drawn at full opacity: the local feed near the camera
/// refreshes every 10 seconds, the global one far less often.
const FRESH_SECS: f64 = 120.0;
const EXTRAPOLATION_MAX_SPEED_KNOTS: f64 = 1200.0;
const KM_PER_NAUTICAL_MILE: f64 = 1.852;
const KM_PER_FOOT: f64 = 0.000_304_8;
const SECONDS_PER_HOUR: f64 = 3600.0;
const HEADING_PROBE_KM: f64 = 3.0;
/// A plane reported turning keeps turning for at most this long (a standard-rate
/// turn takes 30 s for 90°), then flies straight: turns end, and the next report
/// comes long before.
const TURN_MAX_SECS: f64 = 40.0;
/// Turns are followed in steps this long.
const TURN_STEP_SECS: f64 = 4.0;
const WING_REAR_OFFSET_RATIO: f32 = 0.25;
const TAIL_OFFSET_RATIO: f32 = 0.85;
const TRAIL_START_RATIO: f32 = 0.5;
const TRAIL_END_RATIO: f32 = 1.1;

pub struct ProjectedFlight {
    pub source_index: usize,
    pub position: Pos2,
    pub icon: [Pos2; 4],
    pub trail: [Pos2; 2],
    /// Dimmed as the last report ages (see [`crate::motion::age_opacity`]).
    pub opacity: f32,
}

pub fn project_flights(
    basis: &Basis,
    rect: Rect,
    aircraft: &[Aircraft],
    unix_secs: f64,
    map_scale: f32,
    heights: bool,
) -> Vec<ProjectedFlight> {
    aircraft
        .iter()
        .enumerate()
        .filter_map(|(source_index, flight)| {
            let age = (unix_secs - flight.observed_unix).max(0.0);
            // Dead reckoning along the great circle of the last track. The error grows
            // with turns and wind, so older reports fade (see `opacity`).
            let speed_km_s = flight
                .speed_knots
                .unwrap_or(0.0)
                .clamp(0.0, EXTRAPOLATION_MAX_SPEED_KNOTS)
                * KM_PER_NAUTICAL_MILE
                / SECONDS_PER_HOUR;
            let ((lat, lon), heading_deg) = fly(
                basis.model,
                (flight.latitude, flight.longitude),
                flight.heading_deg,
                flight.turn_rate_deg_s.unwrap_or(0.0),
                speed_km_s,
                age.min(EXTRAPOLATION_MAX_SECS),
            );
            // At its height (barometric, near enough), hidden only behind the Earth.
            let km = if heights {
                flight.altitude_feet.unwrap_or(0.0).max(0.0) * KM_PER_FOOT
            } else {
                0.0
            };
            let at = |lat: f64, lon: f64| {
                if km > 0.0 {
                    basis.project_above(basis.raised_position(lat, lon, km), rect)
                } else {
                    basis.project(basis.surface_position(lat, lon), rect)
                }
            };
            let screen = at(lat, lon)?;
            if !rect.contains(screen) {
                return None;
            }
            // Project a small step along the actual aircraft track to orient its icon
            // correctly even when the camera is rotated or near a pole.
            let (ahead_lat, ahead_lon) =
                basis
                    .model
                    .destination((lat, lon), heading_deg, HEADING_PROBE_KM);
            let heading = at(ahead_lat, ahead_lon)
                .map(|ahead| (ahead.y - screen.y).atan2(ahead.x - screen.x))
                .unwrap_or(-std::f32::consts::FRAC_PI_2);
            let forward = Vec2::angled(heading) * (ICON_SIZE_POINTS * map_scale);
            let wing = Vec2::new(-forward.y, forward.x) * WING_WIDTH_RATIO;
            let tail = Vec2::angled(heading) * (TRAIL_SIZE_POINTS * map_scale);
            Some(ProjectedFlight {
                opacity: crate::motion::age_opacity(age, FRESH_SECS, EXTRAPOLATION_MAX_SECS),
                source_index,
                position: screen,
                icon: [
                    screen + forward,
                    screen + wing - forward * WING_REAR_OFFSET_RATIO,
                    screen - forward * TAIL_OFFSET_RATIO,
                    screen - wing - forward * WING_REAR_OFFSET_RATIO,
                ],
                trail: [
                    screen - tail * TRAIL_START_RATIO,
                    screen - tail * TRAIL_END_RATIO,
                ],
            })
        })
        .collect()
}

/// Where a plane is after `secs`, and its track there: turning at `turn_rate` (°/s,
/// positive to the right) for up to [`TURN_MAX_SECS`], then straight on.
pub fn fly(
    model: globecraft_core::earth_model::EarthModel,
    start: (f64, f64),
    heading_deg: f64,
    turn_rate: f64,
    speed_km_s: f64,
    secs: f64,
) -> ((f64, f64), f64) {
    let (mut at, mut heading) = (start, heading_deg);
    let mut left = secs.max(0.0);
    if turn_rate.abs() > 0.05 {
        let mut turning = left.min(TURN_MAX_SECS);
        left -= turning;
        while turning > 0.0 {
            let step = turning.min(TURN_STEP_SECS);
            // Each step flies the heading of its middle.
            at = model.destination(at, heading + turn_rate * step / 2.0, speed_km_s * step);
            heading += turn_rate * step;
            turning -= step;
        }
    }
    if left > 0.0 {
        at = model.destination(at, heading, speed_km_s * left);
    }
    (at, heading.rem_euclid(360.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::earth_model::EarthModel;

    /// A plane at cruising height, seen from the side, is drawn above the point it
    /// flies over; flat, on it.
    #[test]
    fn planes_fly_at_their_height_unless_flat() {
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 600.0));
        let flight = Aircraft {
            hex: "abc123".into(),
            callsign: "TEST".into(),
            latitude: 47.0,
            longitude: 8.0,
            heading_deg: 90.0,
            speed_knots: Some(0.0),
            altitude_feet: Some(36_000.0),
            vertical_rate_fpm: None,
            turn_rate_deg_s: None,
            observed_unix: 1000.0,
            details: None,
            class: globecraft_data::aircraft::AircraftClass::Unknown,
        };
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let basis = globecraft_core::camera::Cam {
                lat: 46.6,
                lon: 8.0,
                heading: 0.0,
                alt: 30.0,
                tilt: 70.0,
            }
            .basis_for_model(4.0 / 3.0, model);
            let raised = project_flights(
                &basis,
                rect,
                std::slice::from_ref(&flight),
                1000.0,
                1.0,
                true,
            );
            let flat = project_flights(
                &basis,
                rect,
                std::slice::from_ref(&flight),
                1000.0,
                1.0,
                false,
            );
            let ground = basis
                .project(basis.surface_position(47.0, 8.0), rect)
                .unwrap();
            assert_eq!(flat[0].position, ground, "{model:?}");
            assert!(
                raised[0].position.y < ground.y - 20.0,
                "{model:?}: {:?} above {ground:?}",
                raised[0].position
            );
        }
    }

    #[test]
    fn a_turning_plane_follows_its_turn_then_flies_straight() {
        let speed = 0.2; // km/s, about 390 kn
        // Straight: same as one great-circle step.
        let (at, heading) = fly(
            EarthModel::PerfectSphere,
            (0.0, 0.0),
            90.0,
            0.0,
            speed,
            60.0,
        );
        assert!((heading - 90.0).abs() < 1e-9);
        assert!((at.1 - destination(0.0, 0.0, 90.0, 12.0).1).abs() < 1e-9);
        // Turning right at 3°/s from north: after 30 s it heads east, having moved
        // along a quarter circle (radius 0.2 * 30 * 2 / π ≈ 3.8 km) to the north-east.
        let (at, heading) = fly(EarthModel::PerfectSphere, (0.0, 0.0), 0.0, 3.0, speed, 30.0);
        assert!((heading - 90.0).abs() < 1e-9);
        let radius_deg = (0.2 * 30.0 * 2.0 / std::f64::consts::PI) / 111.195;
        assert!((at.0 - radius_deg).abs() < radius_deg * 0.02, "{at:?}");
        assert!((at.1 - radius_deg).abs() < radius_deg * 0.02, "{at:?}");
        // The turn stops after TURN_MAX_SECS: a 10-minute extrapolation is no circle.
        let (_, heading) = fly(
            EarthModel::PerfectSphere,
            (0.0, 0.0),
            0.0,
            3.0,
            speed,
            600.0,
        );
        assert!((heading - 3.0 * TURN_MAX_SECS).abs() < 1e-9);
    }
    #[test]
    fn eastbound_plane_moves_east_and_westbound_wraps_dateline() {
        let (lat, lon) = destination(0.0, 0.0, 90.0, 110.0);
        assert!(lat.abs() < 0.01);
        assert!(lon > 0.9 && lon < 1.1);
        let (_, lon) = destination(0.0, 179.9, 90.0, 50.0);
        assert!(lon < -179.0);
    }
}
