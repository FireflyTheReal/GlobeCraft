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
pub(super) const NIGHT_BLUE_TINT: f32 = 0.015;
// Daylight in two parts, as Bing Maps shows it (Julien's recordings, 2026-11-21): the
// sun's direct light, warm where it crossed much air, and the sky's, cool. Ridges and
// blocks hide the first only, so shadows go blue, and slopes and walls facing a low sun
// take the first light while the ground below stays in the blue (`pixel::Daylight`).
// Both only rise with the sun: a fixed split made shade darker just before sunset than
// just after (Julien, 2026-11-22 09:48); a direct light capped against flat ground's
// left no first light on the tops (10:59).
/// The direct sun comes in as its disc rises: the sine of its height where the top of
/// the disc shows (refraction included) and where the whole of it does.
pub(crate) const SUN_RISE_START_COSINE: f64 = -0.008_7;
pub(crate) const SUN_RISE_FULL_COSINE: f64 = 0.026;
/// The sky's light on the ground: as the sun rises, by day (from `SKY_DAY_COSINE` on).
/// The night side darkens from the first toward `NIGHT_BRIGHTNESS`.
pub(crate) const SHADE_RISE: f32 = 0.4;
pub(crate) const SHADE_DAY: f32 = 0.65;
/// The sun's light on a surface facing it, as it rises: with the sky's, a peak or wall
/// in the first light is as bright as by day while the ground below is in the blue.
/// By day it is what makes flat ground at noon one (`1 - SHADE_DAY`).
pub(crate) const DIRECT_RISE: f32 = 0.65;
/// Where twilight ends for the eye in the air (`atmosphere::eye_weight`): the sine of
/// the sun's depth (12°, nautical) where the night side reaches `NIGHT_BRIGHTNESS`.
/// From space the shorter `TWILIGHT_OFFSET` (7°) holds: no bright band past the
/// terminator.
pub(crate) const EYE_TWILIGHT_COSINE: f64 = 0.208;
/// The sky's light: blue in twilight, near white by day (from this sine, 20°).
pub(crate) const TWILIGHT_SKY: [f32; 3] = [0.74, 0.86, 1.26];
pub(crate) const DAY_SKY: [f32; 3] = [0.95, 1.0, 1.08];
pub(crate) const SKY_DAY_COSINE: f64 = 0.342;
/// Rec. 709 luminance weights: the sky's colour is scaled to carry no brightness.
pub(crate) const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];
// Surface rim and atmospheric shell.
// Space and 8-bit RGBA output.
pub(super) const SPACE_COLOR: Vec4 = Vec4::new(0.0, 0.0, 0.015, 0.0);
pub(super) const RGBA_CHANNELS: usize = 4;
pub(super) const COLOR_CHANNEL_MAX: f32 = 255.0;
