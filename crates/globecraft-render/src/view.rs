//! Raster resolution and compatibility keys; event timing stays in the frontend.
use emath::Vec2;
use globecraft_core::camera::Cam;
use globecraft_data::tiles::Provider;
use std::hash::{Hash, Hasher};
const IDLE_RENDER_SCALE: f32 = 1.0;
const ACTIVE_RENDER_SCALE: f32 = 0.4;
const MIN_RENDER_WIDTH_PX: usize = 64;
const MAX_RENDER_WIDTH_PX: usize = 2000;
const MIN_RENDER_HEIGHT_PX: usize = 32;
pub const SCENE_TIME_UPDATE_INTERVAL_SECS: f64 = 60.0;
pub fn raster_size(size_points: Vec2, pixels_per_point: f32, idle: bool) -> (usize, usize) {
    let scale = if idle {
        IDLE_RENDER_SCALE
    } else {
        ACTIVE_RENDER_SCALE
    };
    let width = ((size_points.x * pixels_per_point * scale) as usize)
        .clamp(MIN_RENDER_WIDTH_PX, MAX_RENDER_WIDTH_PX);
    let height =
        ((width as f32 * size_points.y / size_points.x) as usize).max(MIN_RENDER_HEIGHT_PX);
    (width, height)
}
/// Scale between this raster's pixels and the full-quality (idle) raster's, which
/// sets the level of detail. While interacting, frames render at
/// `ACTIVE_RENDER_SCALE`, but the level must follow the idle raster's density, or
/// motion and idle frames sample different zoom bands and the imagery strobes
/// between levels at every motion/idle switch.
///
/// The ratio comes from the actual raster sizes, not the nominal scale: idle rasters
/// are capped at `MAX_RENDER_WIDTH_PX`, so on a large Retina window (3660 px) the
/// idle raster is 2000 px while the motion raster (1464 px) is not capped. Using the
/// nominal 0.4 there put motion frames almost a full level finer than idle frames.
pub fn render_scale(size_points: Vec2, pixels_per_point: f32, idle: bool) -> f32 {
    let (width, _) = raster_size(size_points, pixels_per_point, idle);
    let (idle_width, _) = raster_size(size_points, pixels_per_point, true);
    width as f32 / idle_width as f32
}
pub struct RasterView {
    pub camera: Cam,
    pub model: globecraft_core::earth_model::EarthModel,
    pub width: usize,
    pub height: usize,
    pub aspect: f64,
    pub provider: Provider,
    pub release_index: usize,
    /// Wayback swipe compare: the second release, rendered as its own frame. The
    /// split is not part of the key: moving it only moves where the UI cuts the two
    /// frames, so it never needs a new raster.
    pub compare: Option<usize>,
    pub grid: bool,
    pub atmosphere: bool,
    pub terrain: bool,
    pub terrain_exaggeration: f32,
    /// The terrain height the camera looks at (km), part of the camera position.
    pub ground_km: f64,
    pub sunlight: bool,
    pub sky: bool,
    pub cinematic_sky: bool,
}
pub struct ViewKeys {
    pub state: u64,
    pub presentation: u64,
}
impl RasterView {
    pub fn keys(&self, unix_secs: f64) -> ViewKeys {
        let sun_key = if self.sunlight || self.sky {
            (unix_secs / SCENE_TIME_UPDATE_INTERVAL_SECS) as u64
        } else {
            0
        };
        let mut state = std::collections::hash_map::DefaultHasher::new();
        (
            (
                self.camera.lat.to_bits(),
                self.camera.lon.to_bits(),
                self.camera.heading.to_bits(),
                self.camera.alt.to_bits(),
                self.camera.tilt.to_bits(),
                self.width,
                self.height,
                self.provider.id(),
                self.grid,
            ),
            (
                self.atmosphere,
                self.terrain,
                self.terrain_exaggeration.to_bits(),
                sun_key,
                self.release_index,
                self.sky,
                self.cinematic_sky,
                self.compare,
            ),
            (
                self.aspect.to_bits(),
                self.model.code(),
                self.ground_km.to_bits(),
            ),
        )
            .hash(&mut state);
        let mut presentation = std::collections::hash_map::DefaultHasher::new();
        (
            self.width,
            self.height,
            self.aspect.to_bits(),
            self.provider.id(),
            self.release_index,
            self.compare,
            self.grid,
            self.atmosphere,
            self.terrain,
            self.terrain_exaggeration.to_bits(),
            sun_key,
            (self.model.code(), self.sky, self.cinematic_sky),
        )
            .hash(&mut presentation);
        ViewKeys {
            state: state.finish(),
            presentation: presentation.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sky_changes_invalidate_presentation_even_without_sunlight() {
        let mut view = RasterView {
            camera: Cam {
                lat: 0.0,
                lon: 0.0,
                alt: 14000.0,
                heading: 0.0,
                tilt: 0.0,
            },
            model: globecraft_core::earth_model::EarthModel::Wgs84,
            width: 64,
            height: 48,
            aspect: 4.0 / 3.0,
            provider: Provider::Satellite,
            release_index: 0,
            compare: None,
            grid: false,
            atmosphere: false,
            terrain: false,
            terrain_exaggeration: 1.0,
            ground_km: 0.0,
            sunlight: false,
            sky: false,
            cinematic_sky: false,
        };
        let off = view.keys(120.0);
        assert_eq!(off.presentation, view.keys(180.0).presentation);
        view.sky = true;
        assert_ne!(off.presentation, view.keys(120.0).presentation);
        assert_ne!(view.keys(120.0).presentation, view.keys(180.0).presentation);
        let realistic = view.keys(120.0);
        view.cinematic_sky = true;
        assert_ne!(realistic.presentation, view.keys(120.0).presentation);
    }
    #[test]
    fn motion_changes_state_but_preserves_presentation_compatibility() {
        let mut view = RasterView {
            model: globecraft_core::earth_model::EarthModel::PerfectSphere,
            camera: Cam {
                lat: 0.0,
                lon: 0.0,
                alt: 14_000.0,
                heading: 0.0,
                tilt: 0.0,
            },
            width: 800,
            height: 600,
            aspect: 4.0 / 3.0,
            provider: Provider::Satellite,
            release_index: 0,
            compare: None,
            grid: false,
            atmosphere: true,
            terrain: false,
            terrain_exaggeration: 1.0,
            ground_km: 0.0,
            sunlight: true,
            sky: false,
            cinematic_sky: false,
        };
        let original = view.keys(120.0);
        view.model = globecraft_core::earth_model::EarthModel::Wgs84;
        assert_ne!(view.keys(120.0).presentation, original.presentation);
        assert_ne!(view.keys(120.0).state, original.state);
        view.model = globecraft_core::earth_model::EarthModel::PerfectSphere;
        view.camera.lon = 90.0;
        let moved = view.keys(120.0);
        assert_ne!(moved.state, original.state);
        assert_eq!(moved.presentation, original.presentation);
        assert_ne!(view.keys(180.0).presentation, moved.presentation);
        view.grid = true;
        assert_ne!(view.keys(120.0).presentation, moved.presentation);
        // A swipe compare's release needs new frames (its split does not).
        let plain = view.keys(120.0);
        view.compare = Some(4);
        let compared = view.keys(120.0);
        assert_ne!(compared.presentation, plain.presentation);
        view.compare = Some(5);
        assert_ne!(view.keys(120.0).state, compared.state);
        view.compare = None;
        let before_resize = view.keys(120.0);
        // Raster dimensions can round to the same pixels even as the viewport changes.
        view.aspect += 0.0001;
        assert_ne!(view.keys(120.0).state, before_resize.state);
        assert_ne!(view.keys(120.0).presentation, before_resize.presentation);
    }
    #[test]
    fn terrain_toggle_and_exaggeration_invalidate_presentation() {
        let mut view = RasterView {
            camera: Cam {
                lat: 0.0,
                lon: 0.0,
                alt: 10.0,
                heading: 0.0,
                tilt: 0.0,
            },
            model: globecraft_core::earth_model::EarthModel::Wgs84,
            width: 80,
            height: 60,
            aspect: 4.0 / 3.0,
            provider: Provider::Satellite,
            release_index: 0,
            compare: None,
            grid: false,
            atmosphere: false,
            sunlight: false,
            sky: false,
            cinematic_sky: false,
            terrain: false,
            terrain_exaggeration: 1.0,
            ground_km: 0.0,
        };
        let flat = view.keys(120.0);
        view.terrain = true;
        let mountain = view.keys(120.0);
        assert_ne!(mountain.state, flat.state);
        assert_ne!(mountain.presentation, flat.presentation);
        view.terrain_exaggeration = 2.0;
        assert_ne!(view.keys(120.0).presentation, mountain.presentation);
    }

    #[test]
    fn motion_and_idle_frames_choose_the_same_level_even_when_the_idle_raster_is_capped() {
        // `pixel::surface` picks the level from `render_scale / raster height`.
        for (size, ppp) in [
            (Vec2::new(1280.0, 800.0), 1.0),
            (Vec2::new(1830.0, 1100.0), 2.0),
            (Vec2::new(4000.0, 2000.0), 2.0),
        ] {
            let density = |idle| {
                let (_, height) = raster_size(size, ppp, idle);
                render_scale(size, ppp, idle) / height as f32
            };
            let (idle, motion) = (density(true), density(false));
            assert!(
                ((motion / idle).log2()).abs() < 0.02,
                "{size:?} at {ppp}x: levels differ by {}",
                (motion / idle).log2()
            );
        }
        assert_eq!(render_scale(Vec2::new(1280.0, 800.0), 1.0, true), 1.0);
    }

    #[test]
    fn interaction_resolution_and_size_limits_are_preserved() {
        assert_eq!(
            raster_size(Vec2::new(1280.0, 800.0), 1.0, true),
            (1280, 800)
        );
        assert_eq!(
            raster_size(Vec2::new(1280.0, 800.0), 1.0, false),
            (512, 320)
        );
        assert_eq!(
            raster_size(Vec2::new(4000.0, 2000.0), 2.0, true),
            (2000, 1000)
        );
    }
}
