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
// Surface rim and atmospheric shell.
pub(super) const RIM_FALLOFF_POWER: i32 = 4;
pub(super) const RIM_BLEND: f32 = 0.75;
pub(super) const RIM_COLOR: Vec4 = Vec4::new(0.35, 0.60, 1.0, 0.0);
pub(super) const ATMOSPHERE_RADIUS_RATIO: f64 = 1.07;
pub(super) const ATMOSPHERE_HEIGHT_RATIO: f64 = 0.07;
pub(super) const ATMOSPHERE_COLOR: Vec4 = Vec4::new(0.30, 0.55, 1.0, 0.0);
pub(super) const ATMOSPHERE_BLEND: f32 = 0.9;
// Deterministic star field and 8-bit RGBA output.
pub(super) const SPACE_COLOR: Vec4 = Vec4::new(0.0, 0.0, 0.015, 0.0);
pub(super) const STAR_GRID_RESOLUTION: f64 = 900.0;
pub(super) const STAR_OCCURRENCE_MODULUS: u64 = 1400;
pub(super) const STAR_MIN_BRIGHTNESS: f32 = 0.45;
pub(super) const STAR_BRIGHTNESS_DIVISOR: f32 = 480.0;
pub(super) const STAR_BRIGHTNESS_SHIFT: u32 = 20;
pub(super) const STAR_BRIGHTNESS_MASK: u64 = 255;
pub(super) const RGBA_CHANNELS: usize = 4;
pub(super) const COLOR_CHANNEL_MAX: f32 = 255.0;
pub(super) const OPAQUE_ALPHA: u8 = 255;
