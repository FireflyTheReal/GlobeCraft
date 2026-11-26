//! Raster resolution and compatibility keys; event timing stays in the frontend.
use emath::Vec2;
use globecraft_core::camera::Cam;
use globecraft_data::tiles::Provider;
use std::hash::{Hash, Hasher};
const IDLE_RENDER_SCALE: f32 = 1.0;
const ACTIVE_RENDER_SCALE: f32 = 0.4;
/// While moving over 3D relief: marching the elevation model costs several times a
/// flat frame, so motion frames are smaller (about half the pixels of
/// `ACTIVE_RENDER_SCALE`); the full-resolution frame follows once the view rests.
const ACTIVE_RELIEF_RENDER_SCALE: f32 = 0.28;
/// The lowest motion scale a setting may choose.
pub const MIN_MOTION_SCALE: f32 = 0.2;
const MIN_RENDER_WIDTH_PX: usize = 64;
const MAX_RENDER_WIDTH_PX: usize = 2000;
const MIN_RENDER_HEIGHT_PX: usize = 32;
pub const SCENE_TIME_UPDATE_INTERVAL_SECS: f64 = 60.0;
/// The moment a frame is drawn for: at rest the scene's time to the minute, so a still
/// view keeps its frame while the clock runs; while the time plays exactly that time,
/// or the sun moved in jumps a minute of scene time apart (user, 2026-11-19).
pub fn frame_time(scene_unix: f64, playing: bool) -> f64 {
    if playing {
        scene_unix
    } else {
        (scene_unix / SCENE_TIME_UPDATE_INTERVAL_SECS).floor() * SCENE_TIME_UPDATE_INTERVAL_SECS
    }
}
/// The raster for a viewport: full resolution at rest, smaller while moving (smaller
/// still with `relief`, 3D terrain on screen).
/// The raster for a view `size_points` large. A moving view (`idle` false) renders
/// at `motion_scale` of the full size, or by default at a scale for its kind of
/// view (lower with relief, which costs most).
pub fn raster_size(
    size_points: Vec2,
    pixels_per_point: f32,
    idle: bool,
    relief: bool,
    motion_scale: Option<f32>,
) -> (usize, usize) {
    let scale = match (idle, relief, motion_scale) {
        (true, ..) => IDLE_RENDER_SCALE,
        (false, _, Some(scale)) => scale.clamp(MIN_MOTION_SCALE, IDLE_RENDER_SCALE),
        (false, false, None) => ACTIVE_RENDER_SCALE,
        (false, true, None) => ACTIVE_RELIEF_RENDER_SCALE,
    };
    let width = ((size_points.x * pixels_per_point * scale) as usize)
        .clamp(MIN_RENDER_WIDTH_PX, MAX_RENDER_WIDTH_PX);
    let height =
        ((width as f32 * size_points.y / size_points.x) as usize).max(MIN_RENDER_HEIGHT_PX);
    (width, height)
}
/// The pixels per point a raster is drawn at: the screen's, or fewer so a frame of a
/// view `size_points` large is at most `max_height_px` high (the quality setting).
pub fn quality_pixels_per_point(
    size_points: Vec2,
    pixels_per_point: f32,
    max_height_px: Option<f32>,
) -> f32 {
    max_height_px.map_or(pixels_per_point, |most| {
        pixels_per_point.min(most / size_points.y.max(1.0))
    })
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
pub fn render_scale(
    size_points: Vec2,
    pixels_per_point: f32,
    idle: bool,
    relief: bool,
    motion_scale: Option<f32>,
) -> f32 {
    let (width, _) = raster_size(size_points, pixels_per_point, idle, relief, motion_scale);
    let (idle_width, _) = raster_size(size_points, pixels_per_point, true, relief, None);
    width as f32 / idle_width as f32
}
#[derive(Clone)]
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
    /// `TerrainCfg::without_smoothing` when false.
    pub smooth_relief: bool,
    /// The terrain height the camera looks at (km), part of the camera position.
    pub ground_km: f64,
    pub sunlight: bool,
    pub night_lights: Option<u16>,
    pub stars: bool,
    pub sky: bool,
    /// The clouds as the frame draws them from below: the layer's opacity (bits)
    /// and which map (the live maps' generation), when shown.
    pub clouds: Option<(u32, u64)>,
    /// The water map for the sun's glint (the live maps' generation), when shown.
    pub glint: Option<u64>,
    /// The aurora as the frame draws it: the layer's opacity (bits) and the
    /// forecast's moment (bits), when shown.
    pub aurora: Option<(u32, u64)>,
}
pub struct ViewKeys {
    pub state: u64,
    pub presentation: u64,
}
impl RasterView {
    /// The frame's keys at `frame_time` (see [`frame_time`]). The time is in the state
    /// key only: a frame drawn a moment of scene time ago still shows (as one drawn a
    /// moment of camera motion ago does); in the presentation key every frame of a
    /// playing timeline was out of date before it finished, and few were shown. The
    /// size is in the state key only too: a frame of the size before still shows (at
    /// that size, centred on the view) while a pane's edge slides or is dragged; in the
    /// presentation key every frame finished during a resize was thrown away and the
    /// globe stood still beside black bars until the edge stopped (user, 2026-11-20).
    pub fn keys(&self, frame_time: f64) -> ViewKeys {
        let sun_key = if self.sunlight || self.sky {
            frame_time.to_bits()
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
                (self.camera.tilt.to_bits(), self.camera.look_up.to_bits()),
                self.width,
                self.height,
                self.provider.id(),
                self.grid,
            ),
            (
                self.atmosphere,
                self.terrain,
                (self.terrain_exaggeration.to_bits(), self.smooth_relief),
                sun_key,
                self.release_index,
                self.sky,
                self.compare,
            ),
            (
                self.aspect.to_bits(),
                self.model.code(),
                self.ground_km.to_bits(),
                self.night_lights,
                // Sunlight changes the picture even when the sky already needs the
                // clock (the Milky Way): without it a toggle drew nothing new.
                self.sunlight,
                self.stars,
                (self.clouds, self.glint, self.aurora),
            ),
        )
            .hash(&mut state);
        let mut presentation = std::collections::hash_map::DefaultHasher::new();
        (
            self.provider.id(),
            self.release_index,
            self.compare,
            self.grid,
            self.atmosphere,
            self.terrain,
            (self.terrain_exaggeration.to_bits(), self.smooth_relief),
            (
                self.model.code(),
                self.sky,
                self.clouds,
                (self.glint, self.aurora),
            ),
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
    fn the_quality_caps_the_frame_height_and_never_raises_it() {
        let retina = Vec2::new(1440.0, 860.0);
        let hd = quality_pixels_per_point(retina, 2.0, Some(1080.0));
        assert!((1078..=1080).contains(&raster_size(retina, hd, true, false, None).1));
        assert_eq!(quality_pixels_per_point(retina, 2.0, None), 2.0);
        // A window already smaller than the cap keeps its pixels.
        assert_eq!(
            quality_pixels_per_point(Vec2::new(800.0, 500.0), 1.0, Some(720.0)),
            1.0
        );
        // Moving frames stay the same share of the smaller frame at rest.
        let sd = quality_pixels_per_point(retina, 2.0, Some(720.0));
        assert_eq!(render_scale(retina, sd, true, true, None), 1.0);
        assert!(raster_size(retina, sd, false, true, None).1 < 720 / 3);
    }
    #[test]
    fn sky_changes_invalidate_presentation_even_without_sunlight() {
        let mut view = RasterView {
            camera: Cam {
                lat: 0.0,
                lon: 0.0,
                alt: 14000.0,
                heading: 0.0,
                tilt: 0.0,
                look_up: 0.0,
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
            smooth_relief: true,
            ground_km: 0.0,
            sunlight: false,
            sky: false,
            clouds: None,
            glint: None,
            aurora: None,
            night_lights: None,
            stars: true,
        };
        let off = view.keys(120.0);
        assert_eq!(off.presentation, view.keys(180.0).presentation);
        // Sunlight and stars redraw the frame, also while the Milky Way shows.
        let mut sky_on = view.clone();
        sky_on.sky = true;
        let mut sunlit = sky_on.clone();
        sunlit.sunlight = true;
        assert_ne!(sky_on.keys(120.0).state, sunlit.keys(120.0).state);
        let mut starless = sky_on.clone();
        starless.stars = false;
        assert_ne!(sky_on.keys(120.0).state, starless.keys(120.0).state);
        view.sky = true;
        assert_ne!(off.presentation, view.keys(120.0).presentation);
        // Another moment draws a new frame; the last one shows until it is done.
        assert_ne!(view.keys(120.0).state, view.keys(180.0).state);
        assert_eq!(view.keys(120.0).presentation, view.keys(180.0).presentation);
        // At rest the frame keeps its minute; playing, every moment is its own.
        assert_eq!(frame_time(125.0, false), frame_time(170.0, false));
        assert_eq!(frame_time(125.5, true), 125.5);
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
                look_up: 0.0,
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
            smooth_relief: true,
            ground_km: 0.0,
            sunlight: true,
            sky: false,
            clouds: None,
            glint: None,
            aurora: None,
            night_lights: None,
            stars: true,
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
        assert_ne!(view.keys(180.0).state, moved.state);
        assert_eq!(view.keys(180.0).presentation, moved.presentation);
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
        // A frame of the size before still shows while the view is resized.
        view.width = 640;
        assert_ne!(view.keys(120.0).state, before_resize.state);
        assert_eq!(view.keys(120.0).presentation, before_resize.presentation);
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
                look_up: 0.0,
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
            clouds: None,
            glint: None,
            aurora: None,
            terrain: false,
            terrain_exaggeration: 1.0,
            smooth_relief: true,
            ground_km: 0.0,
            night_lights: None,
            stars: true,
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
    fn a_chosen_motion_resolution_sets_the_moving_raster_and_keeps_the_level() {
        let size = Vec2::new(1280.0, 800.0);
        for (scale, width) in [(Some(1.0), 1280), (Some(0.28), 358), (Some(0.05), 256)] {
            assert_eq!(raster_size(size, 1.0, false, true, scale).0, width);
            // Still at rest: the full size.
            assert_eq!(raster_size(size, 1.0, true, true, scale).0, 1280);
            let (_, height) = raster_size(size, 1.0, false, true, scale);
            let density = render_scale(size, 1.0, false, true, scale) / height as f32;
            let idle = 1.0 / raster_size(size, 1.0, true, true, None).1 as f32;
            assert!((density / idle).log2().abs() < 0.02, "{scale:?}");
        }
    }

    #[test]
    fn motion_and_idle_frames_choose_the_same_level_even_when_the_idle_raster_is_capped() {
        // `pixel::surface` picks the level from `render_scale / raster height`.
        for (size, ppp) in [
            (Vec2::new(1280.0, 800.0), 1.0),
            (Vec2::new(1830.0, 1100.0), 2.0),
            (Vec2::new(4000.0, 2000.0), 2.0),
        ] {
            for relief in [false, true] {
                let density = |idle| {
                    let (_, height) = raster_size(size, ppp, idle, relief, None);
                    render_scale(size, ppp, idle, relief, None) / height as f32
                };
                let (idle, motion) = (density(true), density(false));
                assert!(
                    ((motion / idle).log2()).abs() < 0.02,
                    "{size:?} at {ppp}x (relief {relief}): levels differ by {}",
                    (motion / idle).log2()
                );
            }
        }
        assert_eq!(
            render_scale(Vec2::new(1280.0, 800.0), 1.0, true, false, None),
            1.0
        );
    }

    #[test]
    fn interaction_resolution_and_size_limits_are_preserved() {
        assert_eq!(
            raster_size(Vec2::new(1280.0, 800.0), 1.0, true, false, None),
            (1280, 800)
        );
        assert_eq!(
            raster_size(Vec2::new(1280.0, 800.0), 1.0, false, false, None),
            (512, 320)
        );
        assert_eq!(
            raster_size(Vec2::new(1280.0, 800.0), 1.0, false, true, None),
            (358, 223)
        );
        assert_eq!(
            raster_size(Vec2::new(4000.0, 2000.0), 2.0, true, true, None),
            (2000, 1000)
        );
    }
}
