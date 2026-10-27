//! Picking on the relief (roadmap 1.10): with terrain drawn, the point under the
//! cursor is where the view ray meets the raised ground, not the ellipsoid beneath
//! it. On a tilted view of mountains the two differ by kilometres.
use super::Ground;
use emath::{Pos2, Rect};
use glam::DVec3;
use globecraft_core::camera::Basis;

/// Samples along the stretch of the ray that can meet the relief.
const RAY_SAMPLES: usize = 96;
/// Bisection steps once a sample is below the ground: 2^-24 of a sample step.
const REFINE_STEPS: usize = 24;
/// Steps to bring a high camera's ray down to the top of the relief.
const DESCENT_STEPS: usize = 64;

/// The ground under screen point `pos`, raised by `ground`, whose heights never
/// exceed `max_height` (world units). Falls back to the plain surface pick where the
/// relief is not met (the ground below sea level, or no ground loaded).
pub fn pick_on_ground(
    basis: &Basis,
    rect: Rect,
    pos: Pos2,
    ground: &dyn Ground,
    max_height: f64,
) -> Option<(f64, f64)> {
    let surface = basis.pick(rect, pos)?;
    let geometry = basis.model.geometry();
    let (origin, direction) = (basis.c, basis.ray(rect, pos));
    let end = (basis.surface_position(surface.0, surface.1) - origin).length();
    // Height above the surface, and the relief there.
    let clearance = |t: f64| {
        let point = origin + direction * t;
        let (lat, lon) = geometry.coordinates(point);
        let surface: DVec3 = geometry.position(lat, lon);
        (point.length() - surface.length(), ground.height(lat, lon))
    };
    // Down to the top of the relief: the distance to it is at least the height above.
    let mut start = 0.0;
    for _ in 0..DESCENT_STEPS {
        let (height, _) = clearance(start);
        if height <= max_height || start >= end {
            break;
        }
        start = (start + height - max_height).min(end);
    }
    let below = |t: f64| {
        let (height, relief) = clearance(t);
        height <= relief
    };
    let step = (end - start) / RAY_SAMPLES as f64;
    let mut previous = start;
    for sample in 1..=RAY_SAMPLES {
        let t = start + step * sample as f64;
        if below(t) {
            let (mut above, mut under) = (previous, t);
            for _ in 0..REFINE_STEPS {
                let middle = (above + under) / 2.0;
                if below(middle) {
                    under = middle;
                } else {
                    above = middle;
                }
            }
            return Some(geometry.coordinates(origin + direction * under));
        }
        previous = t;
    }
    Some(surface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::{EARTH_RADIUS_M, camera::Cam, earth_model::EarthModel};

    /// A 3 km plateau north of the equator, flat ground elsewhere.
    struct Plateau;
    impl Ground for Plateau {
        fn height(&self, lat: f64, _lon: f64) -> f64 {
            if lat > 0.05 {
                3_000.0 / EARTH_RADIUS_M
            } else {
                0.0
            }
        }
    }

    #[test]
    fn a_tilted_view_picks_the_raised_ground_in_front_of_the_surface_point() {
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(800.0, 600.0));
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            // Looking north, tilted, at the plateau edge.
            let basis = Cam {
                lat: 0.0,
                lon: 0.0,
                heading: 0.0,
                alt: 20.0,
                tilt: 60.0,
            }
            .basis_for_model(800.0 / 600.0, model);
            let at = rect.center() - emath::vec2(0.0, 120.0);
            let flat = basis.pick(rect, at).unwrap();
            let raised =
                pick_on_ground(&basis, rect, at, &Plateau, 3_000.0 / EARTH_RADIUS_M).unwrap();
            // The ray meets the plateau's face or top before the sea-level point.
            assert!(flat.0 > 0.05, "{model:?} {flat:?}");
            assert!(raised.0 < flat.0 - 0.01, "{model:?} {raised:?} {flat:?}");
            assert!(raised.0 >= 0.05 - 1e-6, "{model:?} {raised:?}");
            // Flat ground picks the same point as before.
            let low = rect.center() + emath::vec2(0.0, 200.0);
            let plain = basis.pick(rect, low).unwrap();
            let picked =
                pick_on_ground(&basis, rect, low, &Plateau, 3_000.0 / EARTH_RADIUS_M).unwrap();
            assert!(model.distance_km(plain, picked) < 0.001, "{model:?}");
        }
    }
}
