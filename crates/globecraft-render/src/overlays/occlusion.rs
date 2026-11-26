//! Terrain occlusion for screen-space overlays.
//!
//! The CPU renderer has no depth buffer: borders, roads, labels and markers are
//! painted after the globe, so a point on the far side of a ridge would otherwise
//! show through it. A point is visible when the straight line from the camera to it
//! stays outside the raised ground. The test samples the line against the elevation
//! tiles the frame already loaded; it never requests tiles.
use super::Ground;
use emath::Rect;
use globecraft_core::camera::Basis;

/// Samples along the eye-to-target line. Elevation cells are coarse (about half the
/// camera altitude), so a handful of samples resolves the ridges that can hide a
/// target without turning every overlay vertex into a terrain query storm.
const OCCLUSION_RAY_SAMPLES: usize = 16;
/// A straight-down view cannot hide a point behind relief between it and the camera
/// (there is no horizontal line of sight to cross). Keep the exact top-down path free
/// of elevation lookups; it is the default view.
const MIN_OCCLUSION_TILT_DEG: f64 = 1.0;
/// Points this far outside the view (as a share of its size) are still tested: a
/// segment from a visible point to a hidden one just off screen must still break at
/// the ridge. Farther points are cut away by clipping and skip the test.
const NEAR_VIEW_MARGIN: f32 = 0.25;

/// How far a point may lie past the ground drawn at its pixel and still show, as a
/// share of that distance: the overlays' heights come from coarser elevation tiles
/// than the frame's, so a point on the ground may sit a little under the drawn
/// surface (most of all along grazing rays).
const DEPTH_SLACK: f64 = 0.03;

/// How far the frame's rays went before they met the drawn ground (the GPU's relief
/// walk, `relief::Walked::pixels`): a point farther than that at its pixel is behind
/// the relief. Its planes are the walk's, row by row: where each ray may start, where
/// it met the ground (world units from the camera; below 0: it did not), the sun's
/// share, and whether the CPU took that ground as it is (above one half).
pub struct DepthMap {
    width: usize,
    height: usize,
    planes: std::sync::Arc<Vec<f32>>,
}

impl DepthMap {
    /// The walk's planes for a `width` × `height` frame (four planes of that size).
    pub fn new(width: usize, height: usize, planes: std::sync::Arc<Vec<f32>>) -> Option<Self> {
        (width > 0 && height > 0 && planes.len() >= 4 * width * height).then_some(Self {
            width,
            height,
            planes,
        })
    }
    /// How far along its ray the frame (drawn in `rect`) met the ground at screen
    /// point `at` (mean Earth radii), where the walk was sure of it.
    pub fn ground_at(&self, rect: Rect, at: emath::Pos2) -> Option<f64> {
        let x = ((at.x - rect.min.x) / rect.width() * self.width as f32).floor();
        let y = ((at.y - rect.min.y) / rect.height() * self.height as f32).floor();
        if !(0.0..self.width as f32).contains(&x) || !(0.0..self.height as f32).contains(&y) {
            return None;
        }
        let (count, i) = (
            self.width * self.height,
            y as usize * self.width + x as usize,
        );
        let (hit, trusted) = (self.planes[count + i], self.planes[3 * count + i]);
        (hit >= 0.0 && trusted > 0.5).then_some(f64::from(hit))
    }
    /// The farthest drawn ground among pixel `(x, y)` and its eight neighbours (a point
    /// at a ridge's edge falls between them), or `None` where any of them has no
    /// ground the walk was sure of.
    fn ground_near(&self, x: usize, y: usize) -> Option<f64> {
        let count = self.width * self.height;
        let mut farthest = 0.0f32;
        for ny in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                let i = ny * self.width + nx;
                let (hit, trusted) = (self.planes[count + i], self.planes[3 * count + i]);
                if !(hit >= 0.0 && trusted > 0.5) {
                    return None;
                }
                farthest = farthest.max(hit);
            }
        }
        Some(f64::from(farthest))
    }
}

/// Whether the counts below are kept (`GLOBECRAFT_FRAME_TIMES` set): shared counters
/// bumped by every lookup cost threads that project in parallel.
pub(super) static COUNTING: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("GLOBECRAFT_FRAME_TIMES").is_some());
/// Bumps `counter` when the counts are kept.
pub(super) fn count(counter: &std::sync::atomic::AtomicUsize) {
    if *COUNTING {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Line-of-sight tests since `take_counts`: answered by the drawn ground, and by the
/// line of elevation lookups (`GLOBECRAFT_FRAME_TIMES`).
static BY_DEPTH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static BY_LINE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Of those by the line: off the frame, and on it where the ground was not sure.
static OFF_FRAME: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static NOT_SURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Loaded-elevation lookups (`GroundHeights::height`) since `take_counts`.
pub(super) static HEIGHTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Since the last call: the line-of-sight tests by the drawn ground and by the line
/// (of those, off the frame and where its ground was not sure), and the
/// loaded-elevation lookups.
pub fn take_counts() -> [usize; 5] {
    use std::sync::atomic::Ordering::Relaxed;
    [
        BY_DEPTH.swap(0, Relaxed),
        BY_LINE.swap(0, Relaxed),
        OFF_FRAME.swap(0, Relaxed),
        NOT_SURE.swap(0, Relaxed),
        HEIGHTS.swap(0, Relaxed),
    ]
}

pub struct TerrainVisibility<'a> {
    basis: &'a Basis,
    ground: &'a dyn Ground,
    /// The frame's drawn ground and the rectangle it covers, when there is one.
    depth: Option<(&'a DepthMap, Rect)>,
}

impl<'a> TerrainVisibility<'a> {
    /// The visibility test for this view, when relief is drawn and the view is
    /// oblique enough for an occluder to stand between the camera and a point.
    pub fn new(basis: &'a Basis, ground: Option<&'a dyn Ground>) -> Option<Self> {
        let ground = ground?;
        (basis.tilt_deg >= MIN_OCCLUSION_TILT_DEG).then_some(Self {
            basis,
            ground,
            depth: None,
        })
    }
    /// The test against the frame's drawn ground `depth` (covering `rect`), where it
    /// has one: one comparison instead of a line of elevation lookups (on an M1, a
    /// view tilted 70° at 16 km spent 36–77 ms a frame on those, 2026-11-26).
    pub fn with_depth(mut self, depth: Option<&'a DepthMap>, rect: Rect) -> Self {
        self.depth = depth.map(|depth| (depth, rect));
        self
    }

    /// Whether a point projected to `at` is near enough to the view `rect` to be
    /// worth testing (see `NEAR_VIEW_MARGIN`).
    pub fn near_view(&self, at: glam::DVec2, rect: Rect) -> bool {
        let near = rect.expand2(rect.size() * NEAR_VIEW_MARGIN);
        (f64::from(near.min.x)..=f64::from(near.max.x)).contains(&at.x)
            && (f64::from(near.min.y)..=f64::from(near.max.y)).contains(&at.y)
    }

    /// Whether `(lat, lon)` raised onto the relief is visible from the camera.
    pub fn visible(&self, lat: f64, lon: f64) -> bool {
        let target = super::ground_position(self.basis, Some(self.ground), lat, lon);
        let eye = self.basis.c;
        if let Some(visible) = self.visible_in_depth(target) {
            count(&BY_DEPTH);
            return visible;
        }
        count(&BY_LINE);
        let geometry = self.basis.model.geometry();
        let ceiling = self.ground.max_height();
        // The target itself is not an occluder; stop one sample short of it so the
        // terrain lookup at the target's own cell cannot hide it.
        for sample in 1..=OCCLUSION_RAY_SAMPLES {
            let t = sample as f64 / (OCCLUSION_RAY_SAMPLES + 1) as f64;
            let point = eye.lerp(target, t);
            let (lat, lon) = geometry.coordinates(point);
            let surface = geometry.position(lat, lon);
            // Higher above the surface than any relief reaches: nothing to look up.
            // Near the camera that is most of the line.
            if point.length() - surface.length() > ceiling {
                continue;
            }
            let terrain =
                surface + globecraft_core::math::ll_to_v3(lat, lon) * self.ground.height(lat, lon);
            if point.length_squared() < terrain.length_squared() {
                return false;
            }
        }
        true
    }
    /// Whether `target` lies no farther than the ground drawn at its pixel; `None`
    /// off the frame or where the walk was not sure of the ground there.
    fn visible_in_depth(&self, target: glam::DVec3) -> Option<bool> {
        let (depth, rect) = self.depth?;
        let at = self.basis.project_precise(target, rect)?;
        let x = (at.x - f64::from(rect.min.x)) / f64::from(rect.width()) * depth.width as f64;
        let y = (at.y - f64::from(rect.min.y)) / f64::from(rect.height()) * depth.height as f64;
        if !(0.0..depth.width as f64).contains(&x) || !(0.0..depth.height as f64).contains(&y) {
            count(&OFF_FRAME);
            return None;
        }
        let Some(ground) = depth.ground_near(x as usize, y as usize) else {
            count(&NOT_SURE);
            return None;
        };
        Some((target - self.basis.c).length() <= ground * (1.0 + DEPTH_SLACK))
    }
}

/// Where `(lat, lon)` raised onto `ground` is drawn, or `None` behind the horizon or
/// a ridge. The point is projected first; only points near the view pay for the
/// line-of-sight test.
pub fn project_unhidden(
    basis: &Basis,
    rect: Rect,
    ground: Option<&dyn Ground>,
    visibility: Option<&TerrainVisibility<'_>>,
    lat: f64,
    lon: f64,
) -> Option<glam::DVec2> {
    let at = basis.project_precise(super::ground_position(basis, ground, lat, lon), rect)?;
    match visibility {
        Some(visibility) if visibility.near_view(at, rect) && !visibility.visible(lat, lon) => None,
        _ => Some(at),
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
        fn max_height(&self) -> f64 {
            world_height(3.0)
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
            look_up: 0.0,
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
            look_up: 0.0,
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
            look_up: 0.0,
        }
        .basis(1.0);
        assert!(TerrainVisibility::new(&down, Some(&Ridge)).is_none());
    }

    #[test]
    fn a_tall_mountain_hides_a_label_behind_it_and_the_ray_is_bounded() {
        struct Counted(std::sync::atomic::AtomicUsize);
        impl Ground for Counted {
            fn height(&self, lat: f64, _lon: f64) -> f64 {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            look_up: 0.0,
        }
        .basis(1.0);
        let ground = Counted(std::sync::atomic::AtomicUsize::new(0));
        let visibility = TerrainVisibility::new(&basis, Some(&ground)).unwrap();
        assert!(!visibility.visible(0.4, 0.0));
        assert!(
            ground.0.load(std::sync::atomic::Ordering::Relaxed) <= OCCLUSION_RAY_SAMPLES + 1,
            "{} elevation lookups for one point",
            ground.0.load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    /// With the frame's drawn ground, a point farther than the ground at its pixel is
    /// hidden without a lookup along the line, a nearer one shows, and a pixel whose
    /// ground the walk was not sure of falls back to the line test.
    #[test]
    fn the_drawn_ground_hides_what_lies_behind_it() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 20.0,
            tilt: 60.0,
            look_up: 0.0,
        }
        .basis(1.0);
        let rect = Rect::from_min_size(emath::Pos2::ZERO, emath::vec2(800.0, 600.0));
        let (width, height) = (80, 60);
        let count = width * height;
        let near = super::super::ground_position(&basis, Some(&Flat), 0.1, 0.0);
        let ground = (near - basis.c).length() as f32 * 1.1;
        let planes = |trusted: f32| {
            let mut planes = vec![0.0f32; 4 * count];
            planes[count..2 * count].fill(ground);
            planes[3 * count..].fill(trusted);
            DepthMap::new(width, height, std::sync::Arc::new(planes)).unwrap()
        };
        let (sure, unsure) = (planes(1.0), planes(-1.0));
        for (lat, _) in [(0.1, ()), (0.4, ())] {
            let target = super::super::ground_position(&basis, Some(&Flat), lat, 0.0);
            assert!(
                basis
                    .project_precise(target, rect)
                    .is_some_and(|at| rect.contains(emath::pos2(at.x as f32, at.y as f32))),
                "{lat} on screen"
            );
        }
        let visibility = TerrainVisibility::new(&basis, Some(&Flat))
            .unwrap()
            .with_depth(Some(&sure), rect);
        assert!(visibility.visible(0.1, 0.0), "nearer than the drawn ground");
        assert!(!visibility.visible(0.4, 0.0), "behind the drawn ground");
        let fallback = TerrainVisibility::new(&basis, Some(&Flat))
            .unwrap()
            .with_depth(Some(&unsure), rect);
        assert!(fallback.visible(0.4, 0.0), "flat ground by the line test");
    }

    #[test]
    fn world_heights_use_earth_radii() {
        // A 3 km ridge is 3/6371 world units above the surface.
        assert!((world_height(3.0) * EARTH_RADIUS_M - 3_000.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod cost {
    use super::*;
    use globecraft_core::{EARTH_RADIUS_KM, camera::Cam};

    /// A 9 km mountain range, counting its lookups.
    struct Counted(std::sync::atomic::AtomicUsize, f64);
    impl Ground for Counted {
        fn height(&self, lat: f64, _lon: f64) -> f64 {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if (0.05..0.15).contains(&lat) {
                9.0 / EARTH_RADIUS_KM
            } else {
                0.0
            }
        }
        fn max_height(&self) -> f64 {
            self.1
        }
    }

    /// Knowing the highest relief skips most lookups and changes no answer.
    #[test]
    fn the_relief_ceiling_skips_lookups_high_above_the_ground() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 30.0,
            tilt: 65.0,
            look_up: 0.0,
        }
        .basis(1.0);
        let unbounded = Counted(std::sync::atomic::AtomicUsize::new(0), f64::INFINITY);
        let bounded = Counted(
            std::sync::atomic::AtomicUsize::new(0),
            9.0 / EARTH_RADIUS_KM,
        );
        let (a, b) = (
            TerrainVisibility::new(&basis, Some(&unbounded)).unwrap(),
            TerrainVisibility::new(&basis, Some(&bounded)).unwrap(),
        );
        for i in 0..200 {
            let lat = -0.2 + f64::from(i) * 0.004;
            for lon in [-0.1, 0.0, 0.1] {
                assert_eq!(a.visible(lat, lon), b.visible(lat, lon), "{lat} {lon}");
            }
        }
        let (all, skipped) = (
            unbounded.0.load(std::sync::atomic::Ordering::Relaxed),
            bounded.0.load(std::sync::atomic::Ordering::Relaxed),
        );
        println!("lookups: {all} without a ceiling, {skipped} with");
        // At 30 km with a 9 km ceiling the upper ~40% of each line needs no lookup.
        assert!(skipped * 4 < all * 3, "{skipped} of {all}");
    }
}

#[cfg(test)]
mod benchmarks {
    use super::*;
    use globecraft_core::{EARTH_RADIUS_KM, camera::Cam};
    use std::sync::Mutex;

    /// Rolling mountains up to 3 km behind a lock and a map lookup per call, roughly
    /// what a loaded-elevation lookup costs, counting its calls.
    struct Mountains {
        calls: Mutex<usize>,
        cells: std::collections::HashMap<(i32, i32), f64>,
        ceiling: f64,
    }
    impl Ground for Mountains {
        fn height(&self, lat: f64, lon: f64) -> f64 {
            *self.calls.lock().unwrap() += 1;
            let cell = ((lat * 200.0) as i32, (lon * 200.0) as i32);
            self.cells.get(&cell).copied().unwrap_or(0.0)
        }
        fn max_height(&self) -> f64 {
            self.ceiling
        }
    }
    fn mountains(ceiling: f64) -> Mountains {
        let mut cells = std::collections::HashMap::new();
        for i in -400..400 {
            for j in -400..400 {
                let (lat, lon) = (f64::from(i) / 200.0, f64::from(j) / 200.0);
                let km = 1.5 + 1.5 * (lat * 40.0).sin() * (lon * 30.0).cos();
                cells.insert((i, j), km / EARTH_RADIUS_KM);
            }
        }
        Mountains {
            calls: Mutex::new(0),
            cells,
            ceiling,
        }
    }

    /// A tilted relief view over 40,000 line points (borders and roads of a region,
    /// most of them off screen): the line-of-sight cost before (every point tested,
    /// every sample looked up) and after (on-screen points only, samples above the
    /// highest relief skipped). Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "manual occlusion cost benchmark"]
    fn tilted_relief_occlusion_cost() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 25.0,
            tilt: 65.0,
            look_up: 0.0,
        }
        .basis(1.6);
        let rect = Rect::from_min_size(emath::Pos2::ZERO, emath::vec2(1600.0, 1000.0));
        let points: Vec<(f64, f64)> = (0..200)
            .flat_map(|i| {
                (0..200).map(move |j| (-1.0 + f64::from(i) * 0.01, -1.0 + f64::from(j) * 0.01))
            })
            .collect();
        let before = mountains(f64::INFINITY);
        let start = std::time::Instant::now();
        let visibility = TerrainVisibility::new(&basis, Some(&before)).unwrap();
        let mut shown_before = 0;
        let on_screen = |at: glam::DVec2| {
            (0.0..=f64::from(rect.max.x)).contains(&at.x)
                && (0.0..=f64::from(rect.max.y)).contains(&at.y)
        };
        for &(lat, lon) in &points {
            if visibility.visible(lat, lon)
                && basis
                    .project_precise(
                        super::super::ground_position(&basis, Some(&before), lat, lon),
                        rect,
                    )
                    .is_some_and(on_screen)
            {
                shown_before += 1;
            }
        }
        let before_time = start.elapsed();
        let after = mountains(3.0 / EARTH_RADIUS_KM);
        let start = std::time::Instant::now();
        let visibility = TerrainVisibility::new(&basis, Some(&after)).unwrap();
        let ground: &dyn Ground = &after;
        let mut shown_after = 0;
        for &(lat, lon) in &points {
            if project_unhidden(&basis, rect, Some(ground), Some(&visibility), lat, lon)
                .is_some_and(on_screen)
            {
                shown_after += 1;
            }
        }
        let after_time = start.elapsed();
        // Off-screen points are clipped away either way; on screen the answers match.
        assert_eq!(shown_before, shown_after);
        println!(
            "{} points: before {} lookups in {:.1} ms ({} drawn), after {} lookups in {:.1} ms ({} drawn)",
            points.len(),
            before.calls.lock().unwrap(),
            before_time.as_secs_f64() * 1000.0,
            shown_before,
            after.calls.lock().unwrap(),
            after_time.as_secs_f64() * 1000.0,
            shown_after,
        );
    }
}
