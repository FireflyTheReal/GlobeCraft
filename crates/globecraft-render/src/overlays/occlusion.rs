//! Terrain occlusion for screen-space overlays.
//!
//! The CPU renderer has no depth buffer: borders, roads, labels and markers are
//! painted after the globe, so a point on the far side of a ridge would otherwise
//! show through it. A point is visible when the straight line from the camera to it
//! stays outside the raised ground. The test samples the line against the elevation
//! tiles the frame already loaded; it never requests tiles.
use super::Ground;
use globecraft_core::camera::Basis;

/// Samples along the eye-to-target line. Elevation cells are coarse (about half the
/// camera altitude), so a handful of samples resolves the ridges that can hide a
/// target without turning every overlay vertex into a terrain query storm.
const OCCLUSION_RAY_SAMPLES: usize = 16;
/// A straight-down view cannot hide a point behind relief between it and the camera
/// (there is no horizontal line of sight to cross). Keep the exact top-down path free
/// of elevation lookups; it is the default view.
const MIN_OCCLUSION_TILT_DEG: f64 = 1.0;

pub struct TerrainVisibility<'a> {
    basis: &'a Basis,
    ground: &'a dyn Ground,
}

impl<'a> TerrainVisibility<'a> {
    /// The visibility test for this view, when relief is drawn and the view is
    /// oblique enough for an occluder to stand between the camera and a point.
    pub fn new(basis: &'a Basis, ground: Option<&'a dyn Ground>) -> Option<Self> {
        let ground = ground?;
        (basis.tilt_deg >= MIN_OCCLUSION_TILT_DEG).then_some(Self { basis, ground })
    }

    /// Whether `(lat, lon)` raised onto the relief is visible from the camera.
    pub fn visible(&self, lat: f64, lon: f64) -> bool {
        let target = super::ground_position(self.basis, Some(self.ground), lat, lon);
        let eye = self.basis.c;
        let geometry = self.basis.model.geometry();
        // The target itself is not an occluder; stop one sample short of it so the
        // terrain lookup at the target's own cell cannot hide it.
        for sample in 1..=OCCLUSION_RAY_SAMPLES {
            let t = sample as f64 / (OCCLUSION_RAY_SAMPLES + 1) as f64;
            let point = eye.lerp(target, t);
            let (lat, lon) = geometry.coordinates(point);
            let terrain = super::ground_position(self.basis, Some(self.ground), lat, lon);
            if point.length_squared() < terrain.length_squared() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::{EARTH_RADIUS_KM, EARTH_RADIUS_M, camera::Cam};

    fn world_height(km: f64) -> f64 {
        km / EARTH_RADIUS_KM
    }

    struct Flat;
    impl Ground for Flat {
        fn height(&self, _lat: f64, _lon: f64) -> f64 {
            0.0
        }
    }

    /// A ridge across the view, three kilometres high near the equator.
    struct Ridge;
    impl Ground for Ridge {
        fn height(&self, lat: f64, _lon: f64) -> f64 {
            if (0.2..0.3).contains(&lat) {
                world_height(3.0)
            } else {
                0.0
            }
        }
    }

    /// The ridge is hidden by the ground at the camera when the view looks down.
    #[test]
    fn flat_ground_never_hides_a_point() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 20.0,
            tilt: 60.0,
        }
        .basis(1.0);
        let visibility = TerrainVisibility::new(&basis, Some(&Flat)).unwrap();
        assert!(visibility.visible(0.5, 0.0));
    }

    #[test]
    fn a_ridge_hides_the_ground_behind_it_and_leaves_near_points_alone() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 20.0,
            tilt: 60.0,
        }
        .basis(1.0);
        let visibility = TerrainVisibility::new(&basis, Some(&Ridge)).unwrap();
        assert!(visibility.visible(0.1, 0.0), "in front of the ridge");
        assert!(!visibility.visible(0.4, 0.0), "behind the ridge");
        assert!(visibility.visible(-0.3, 0.0), "away from the ridge");
        // Nothing is hidden when the view is straight down.
        let down = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 20.0,
            tilt: 0.0,
        }
        .basis(1.0);
        assert!(TerrainVisibility::new(&down, Some(&Ridge)).is_none());
    }

    #[test]
    fn a_tall_mountain_hides_a_label_behind_it_and_the_ray_is_bounded() {
        struct Counted(std::cell::Cell<usize>);
        impl Ground for Counted {
            fn height(&self, lat: f64, _lon: f64) -> f64 {
                self.0.set(self.0.get() + 1);
                if (0.05..0.15).contains(&lat) {
                    world_height(9.0)
                } else {
                    0.0
                }
            }
        }
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 30.0,
            tilt: 65.0,
        }
        .basis(1.0);
        let ground = Counted(std::cell::Cell::new(0));
        let visibility = TerrainVisibility::new(&basis, Some(&ground)).unwrap();
        assert!(!visibility.visible(0.4, 0.0));
        assert!(
            ground.0.get() <= OCCLUSION_RAY_SAMPLES + 1,
            "{} elevation lookups for one point",
            ground.0.get()
        );
    }

    #[test]
    fn world_heights_use_earth_radii() {
        // A 3 km ridge is 3/6371 world units above the surface.
        assert!((world_height(3.0) * EARTH_RADIUS_M - 3_000.0).abs() < 1e-6);
    }
}
