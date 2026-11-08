//! Scale-bar distance selection, independent of drawing.
use globecraft_core::{METERS_PER_KM, VERTICAL_FOV_HALF_TAN, units::DistanceUnits};
const MAX_ALTITUDE_KM: f64 = 3000.0;
const TARGET_WIDTH_POINTS: f64 = 120.0;
#[derive(Clone, Copy)]
pub struct ScaleBar {
    pub distance_m: f64,
    pub width_points: f32,
    /// Points above the globe's bottom edge where the bar sits. A tilted view can
    /// put the bottom edge below the horizon; the bar then moves up to the ground.
    pub bottom_inset_points: f32,
}
impl ScaleBar {
    pub fn label(&self) -> String {
        if self.distance_m >= METERS_PER_KM {
            format!("{} km", self.distance_m / METERS_PER_KM)
        } else {
            format!("{} m", self.distance_m)
        }
    }
}

/// The experimental ellipsoid bar measures the actual surface under its screen
/// endpoints. The default sphere keeps its original altitude-based policy while it
/// looks straight down; a tilted view measures the ground under the bar instead,
/// because the altitude-based scale is only right at the view's centre.
fn scale_bar_for_view(
    basis: &globecraft_core::camera::Basis,
    rect: emath::Rect,
    altitude_km: f64,
    units: DistanceUnits,
) -> Option<ScaleBar> {
    if basis.model == globecraft_core::earth_model::EarthModel::PerfectSphere
        && basis.tilt_deg == 0.0
    {
        return scale_bar_with_units(altitude_km, rect.height(), units);
    }
    if altitude_km >= MAX_ALTITUDE_KM {
        return None;
    }
    let ground = visible_scale_ground(basis, rect)?;
    let maximum_m = basis.model.distance_km(ground.start, ground.end) * METERS_PER_KM;
    if !maximum_m.is_finite() || maximum_m <= 0.0 {
        return None;
    }
    let unit_m = units.scale_unit_meters(maximum_m);
    let target = maximum_m / unit_m;
    let pow = 10f64.powf(target.log10().floor());
    let distance_m = [5.0, 2.0, 1.0]
        .into_iter()
        .map(|multiple| multiple * pow)
        .find(|value| *value <= target)?
        * unit_m;
    let anchor = emath::pos2(
        rect.left() + SCALE_LEFT_INSET_POINTS,
        rect.bottom() - ground.bottom_inset_points,
    );
    let (mut low, mut high) = (0.0, TARGET_WIDTH_POINTS as f32);
    const WIDTH_REFINEMENT_STEPS: usize = 24;
    for _ in 0..WIDTH_REFINEMENT_STEPS {
        let middle = (low + high) * 0.5;
        let end = basis.pick(rect, anchor + emath::vec2(middle, 0.0))?;
        if basis.model.distance_km(ground.start, end) * METERS_PER_KM < distance_m {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(ScaleBar {
        distance_m,
        width_points: (low + high) * 0.5,
        bottom_inset_points: ground.bottom_inset_points,
    })
}

/// The ground under the scale bar's two ends, and the inset that keeps both on it.
struct ScaleGround {
    start: (f64, f64),
    end: (f64, f64),
    bottom_inset_points: f32,
}

/// The lowest line on the scale bar's left edge whose two ends both hit the ground.
/// A tilted view can look past the horizon at the bottom; the bar then moves up.
fn visible_scale_ground(
    basis: &globecraft_core::camera::Basis,
    rect: emath::Rect,
) -> Option<ScaleGround> {
    const SCAN_STEP_POINTS: f32 = 4.0;
    let x = rect.left() + SCALE_LEFT_INSET_POINTS;
    let mut bottom_inset_points = SCALE_BOTTOM_INSET_POINTS;
    while bottom_inset_points <= rect.height() * 0.5 {
        let anchor = emath::pos2(x, rect.bottom() - bottom_inset_points);
        if let (Some(start), Some(end)) = (
            basis.pick(rect, anchor),
            basis.pick(rect, anchor + emath::vec2(TARGET_WIDTH_POINTS as f32, 0.0)),
        ) {
            return Some(ScaleGround {
                start,
                end,
                bottom_inset_points,
            });
        }
        bottom_inset_points += SCAN_STEP_POINTS;
    }
    None
}
pub const SCALE_LEFT_INSET_POINTS: f32 = 18.0;
pub const SCALE_BOTTOM_INSET_POINTS: f32 = 28.0;

#[derive(Clone, Copy, PartialEq)]
struct ScaleView {
    basis: globecraft_core::camera::Basis,
    rect: emath::Rect,
    altitude_km: f64,
    units: DistanceUnits,
}
#[derive(Default)]
pub struct ScaleBarCache {
    latest: Option<(ScaleView, Option<ScaleBar>)>,
}
impl ScaleBarCache {
    pub fn for_view(
        &mut self,
        basis: &globecraft_core::camera::Basis,
        rect: emath::Rect,
        altitude_km: f64,
        units: DistanceUnits,
    ) -> Option<ScaleBar> {
        let view = ScaleView {
            basis: *basis,
            rect,
            altitude_km,
            units,
        };
        if let Some((old, result)) = &self.latest
            && *old == view
        {
            return *result;
        }
        let result = scale_bar_for_view(basis, rect, altitude_km, units);
        self.latest = Some((view, result));
        result
    }
}
pub fn scale_bar(altitude_km: f64, height_points: f32) -> Option<ScaleBar> {
    scale_bar_with_units(altitude_km, height_points, DistanceUnits::Metric)
}
/// Select a round displayed distance in the requested unit system.
pub fn scale_bar_with_units(
    altitude_km: f64,
    height_points: f32,
    units: DistanceUnits,
) -> Option<ScaleBar> {
    if altitude_km >= MAX_ALTITUDE_KM {
        return None;
    }
    let mpp = altitude_km * METERS_PER_KM * 2.0 * VERTICAL_FOV_HALF_TAN / height_points as f64;
    let unit_m = units.scale_unit_meters(mpp * TARGET_WIDTH_POINTS);
    let target = mpp * TARGET_WIDTH_POINTS / unit_m;
    let pow = 10f64.powf(target.log10().floor());
    let distance_m = [5.0, 2.0, 1.0]
        .iter()
        .map(|m| m * pow)
        .find(|v| *v <= target)
        .unwrap_or(pow);
    Some(ScaleBar {
        distance_m: distance_m * unit_m,
        width_points: (distance_m * unit_m / mpp) as f32,
        bottom_inset_points: SCALE_BOTTOM_INSET_POINTS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ellipsoid_bar_measures_its_actual_screen_endpoints() {
        use globecraft_core::{camera::Cam, earth_model::EarthModel};
        let rect = emath::Rect::from_min_size(emath::Pos2::ZERO, emath::vec2(800.0, 600.0));
        let mut cache = ScaleBarCache::default();
        for units in [DistanceUnits::Metric, DistanceUnits::Imperial] {
            for altitude in [1.0, 100.0, 1000.0] {
                let basis = Cam {
                    lat: 45.0,
                    lon: 179.9,
                    heading: 75.0,
                    alt: altitude,
                    tilt: 0.0,
                }
                .basis_for_model(4.0 / 3.0, EarthModel::Wgs84);
                let bar = cache.for_view(&basis, rect, altitude, units).unwrap();
                let anchor = emath::pos2(
                    SCALE_LEFT_INSET_POINTS,
                    rect.bottom() - SCALE_BOTTOM_INSET_POINTS,
                );
                let a = basis.pick(rect, anchor).unwrap();
                let b = basis
                    .pick(rect, anchor + emath::vec2(bar.width_points, 0.0))
                    .unwrap();
                let actual_m = basis.model.distance_km(a, b) * METERS_PER_KM;
                assert!(
                    (actual_m - bar.distance_m).abs() < bar.distance_m * 1e-6,
                    "{actual_m} vs {}",
                    bar.distance_m
                );
                assert_eq!(
                    cache
                        .for_view(&basis, rect, altitude, units)
                        .unwrap()
                        .width_points,
                    bar.width_points
                );
            }
        }
    }
    #[test]
    fn imperial_scales_use_round_distances_at_bounded_screen_widths() {
        for altitude in [0.15, 1.0, 10.0, 100.0, 1000.0, 2999.0] {
            let scale = scale_bar_with_units(altitude, 800.0, DistanceUnits::Imperial).unwrap();
            assert!(scale.width_points > 0.0 && scale.width_points <= TARGET_WIDTH_POINTS as f32);
            let label = DistanceUnits::Imperial.scale_label(scale.distance_m);
            assert!(label.ends_with(" ft") || label.ends_with(" mi"), "{label}");
        }
    }
    #[test]
    fn tilted_sphere_bar_measures_the_ground_under_its_ends() {
        use globecraft_core::{camera::Cam, earth_model::EarthModel};
        let rect = emath::Rect::from_min_size(emath::Pos2::ZERO, emath::vec2(900.0, 700.0));
        for (altitude, tilt) in [(900.0, 55.0), (60.0, 75.0)] {
            let basis = Cam {
                lat: 46.5,
                lon: 8.0,
                heading: 20.0,
                alt: altitude,
                tilt,
            }
            .basis_for_model(4.0 / 3.0, EarthModel::PerfectSphere);
            let mut cache = ScaleBarCache::default();
            let bar = cache
                .for_view(&basis, rect, altitude, DistanceUnits::Metric)
                .unwrap();
            let anchor = emath::pos2(
                rect.left() + SCALE_LEFT_INSET_POINTS,
                rect.bottom() - bar.bottom_inset_points,
            );
            let a = basis.pick(rect, anchor).unwrap();
            let b = basis
                .pick(rect, anchor + emath::vec2(bar.width_points, 0.0))
                .unwrap();
            let actual_m = basis.model.distance_km(a, b) * METERS_PER_KM;
            assert!(
                (actual_m - bar.distance_m).abs() < bar.distance_m * 1e-6,
                "{altitude} km at {tilt}°: {actual_m} vs {}",
                bar.distance_m
            );
            // The top-down policy uses the altitude, not the ground under the bar.
            let flat =
                scale_bar_with_units(altitude, rect.height(), DistanceUnits::Metric).unwrap();
            assert!(
                (bar.distance_m - flat.distance_m).abs() > flat.distance_m * 0.01,
                "{altitude} km at {tilt}°: {} vs {}",
                bar.distance_m,
                flat.distance_m
            );
        }
    }

    #[test]
    fn a_top_down_sphere_keeps_the_altitude_based_bar() {
        use globecraft_core::{camera::Cam, earth_model::EarthModel};
        let rect = emath::Rect::from_min_size(emath::Pos2::ZERO, emath::vec2(900.0, 700.0));
        let basis = Cam {
            lat: 46.5,
            lon: 8.0,
            heading: 20.0,
            alt: 900.0,
            tilt: 0.0,
        }
        .basis_for_model(4.0 / 3.0, EarthModel::PerfectSphere);
        let bar = ScaleBarCache::default()
            .for_view(&basis, rect, 900.0, DistanceUnits::Metric)
            .unwrap();
        let old = scale_bar_with_units(900.0, rect.height(), DistanceUnits::Metric).unwrap();
        assert_eq!(bar.distance_m, old.distance_m);
        assert_eq!(bar.width_points, old.width_points);
        assert_eq!(bar.bottom_inset_points, SCALE_BOTTOM_INSET_POINTS);
    }

    #[test]
    fn familiar_distance_and_visibility_cutoff() {
        let scale = scale_bar(1.0, 800.0).unwrap();
        assert_eq!(scale.distance_m, 100.0);
        assert_eq!(scale.label(), "100 m");
        assert!(scale.width_points > 0.0 && scale.width_points <= 120.0);
        assert!(scale_bar(3000.0, 800.0).is_none());
    }
}
