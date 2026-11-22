//! Pixel-rendering parameters
use glam::Vec4;
// Tile resolution, projection limits, and level-of-detail tuning.
pub(super) const TILE_SIZE_PX: f64 = 256.0;
pub(super) const ROWS_PER_TILE_BATCH: usize = 8;
pub(super) const WEB_MERCATOR_MAX_LATITUDE_DEG: f64 = 85.051_128_78;
pub(super) const MIN_LOD_LATITUDE_COSINE: f64 = 0.02;
pub(super) const MIN_LOD_INCIDENCE_COSINE: f64 = 0.2;
/// Levels above the screen-resolution match used as the base level. With 0.5
/// the base rounded to the nearest level and its low half of each band
/// rendered undersampled (softer than the screen). A full level keeps a
/// constant one-level oversampling margin, so imagery stays sharp across the
/// whole band.
pub(super) const LOD_SHARPNESS_BIAS: f64 = 1.0;
pub(super) const MIN_ZOOM_BLEND_FRACTION: f32 = 0.01;
/// Text maps crossfade only over the last part of a zoom band, so labels stay
/// sharp and the level change reads as a short fade.
pub(super) const TEXT_ZOOM_FADE_START: f32 = 0.9;
/// Imagery blends even later: with a one-level-oversampling base the next
/// level is needed only in its own short window, so most of the band needs no
/// second tile pyramid at all and only the level change is smoothed.
pub(super) const IMAGERY_ZOOM_FADE_START: f32 = 0.7;
pub(super) const MISSING_TILE_COLOR: Vec4 = Vec4::new(0.04, 0.09, 0.18, 0.0);
/// Texels per raster pixel at which bilinear minification starts to alias.
/// Up to this ratio the two-tap bilinear still carries most of the contrast;
/// beyond it the renderer asks the sampler for an area average sized by the
/// ratio, so oversampled bands and reduced-scale motion frames stay free of
/// minification shimmer instead of looking like a lower-bitrate source.
/// Texels-per-raster-pixel exponent where bilinear minification starts to
/// alias. Up to `2^0.322` (a 1.25 texels-per-pixel ratio) bilinear still
/// carries most of the contrast; beyond it the renderer asks the sampler for
/// an area average sized by the ratio, so oversampled bands and reduced-scale
/// motion frames stay free of minification shimmer instead of looking like a
/// lower-bitrate source.
pub(super) const TILE_FILTER_RATIO_EXONENT_MIN: f64 = 0.321_928_094_887_362_3;
/// Largest useful ratio: the stride sampler caps at four texels per axis, so
/// a ratio beyond `2^2` saturates there instead of growing the box forever.
pub(super) const TILE_FILTER_RATIO_EXONENT_MAX: f64 = 2.0;
/// Fractional exp2 table resolution for per-pixel stride sizing.
pub(super) const TILE_EXP2_LUT_STEPS: f64 = 256.0;
// Grid visibility and day/night transition.
pub(super) const GRID_LINE_WIDTH_PX: f64 = 0.8;
pub(super) const GRID_COLOR: Vec4 = Vec4::new(1.0, 1.0, 0.6, 0.0);
pub(super) const GRID_BLEND: f32 = 0.55;
pub(super) const TWILIGHT_OFFSET: f64 = 0.12;
pub(super) const TWILIGHT_WIDTH: f64 = 0.30;
pub(super) const NIGHT_BRIGHTNESS: f32 = 0.10;
pub(super) const DAY_BRIGHTNESS_RANGE: f32 = 0.90;
pub(super) const NIGHT_BLUE_TINT: f32 = 0.015;
// Daylight in two parts, as Bing Maps shows it (Julien's recordings, 2026-11-21): the
// sun's direct light, warm where it crossed much air, and the sky's, cool. Ridges and
// blocks hide the first only, so shadows go blue and slopes facing a low sun glow.
/// The direct sun's share of daylight on flat ground at noon (the sky gives the rest).
pub(crate) const DIRECT_SHARE: f32 = 0.65;
/// The direct sun comes in as its disc rises: sine of its height where it starts (its
/// top edge at the horizon, with refraction) and where it is fully in.
pub(crate) const SUN_RISE_START_COSINE: f64 = -0.008_7;
pub(crate) const SUN_RISE_FULL_COSINE: f64 = 0.026;
/// A surface's direct light against flat ground's, which is taken as never below this
/// sine (8°): under it a slope or wall facing the sun outshines flat ground.
pub(crate) const DIRECT_FLOOR_COSINE: f64 = 0.139;
/// The most a slope or wall facing a low sun gets against flat ground.
pub(crate) const MAX_DIRECT_GAIN: f64 = 2.5;
/// How bright the scene is drawn as the sun sets (the eye's adaptation): from night's
/// floor where its sine is this far below the horizon (12°) to day's full light over
/// this width (to 8.5° up), so the blue hour stays readable as in Bing Maps.
pub(crate) const EXPOSURE_OFFSET: f64 = 0.208;
pub(crate) const EXPOSURE_WIDTH: f64 = 0.356;
/// The sky's light: blue in twilight, near white by day (from this sine, 20°).
pub(crate) const TWILIGHT_SKY: [f32; 3] = [0.74, 0.86, 1.26];
pub(crate) const DAY_SKY: [f32; 3] = [0.95, 1.0, 1.08];
pub(crate) const SKY_DAY_COSINE: f64 = 0.342;
// Surface rim and atmospheric shell.
// Space and 8-bit RGBA output.
pub(super) const SPACE_COLOR: Vec4 = Vec4::new(0.0, 0.0, 0.015, 0.0);
pub(super) const RGBA_CHANNELS: usize = 4;
pub(super) const COLOR_CHANNEL_MAX: f32 = 255.0;
