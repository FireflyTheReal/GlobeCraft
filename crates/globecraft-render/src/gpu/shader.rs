//! Embed shared rendering constants into the portable WGSL shader.
use crate::tuning;
use globecraft_core::{EARTH_RADIUS_M, VERTICAL_FOV_HALF_TAN};

/// The shader's f32 latitude may land on the other side of the cap's start than the
/// CPU's: it hands over this much more, where the cap's weight is still zero.
const POLAR_CAP_MARGIN_DEG: f64 = 0.01;

pub(super) fn source() -> String {
    let color = |v: glam::Vec4| format!("vec3<f32>({:?}, {:?}, {:?})", v.x, v.y, v.z);
    let mut source = include_str!("shaders/overview.wgsl")
        .replace("__MISSING__", &color(tuning::MISSING_TILE_COLOR))
        .replace("__LIGHTS_TAG__", &super::overview::LIGHTS_TAG.to_string());
    use crate::atmosphere as air;
    for (name, value) in [
        ("AIR_ANGLES", air::TRANSMITTANCE_ANGLES),
        ("AIR_VIEWS", air::SCATTERING_VIEWS),
        ("AIR_SUNS", air::SCATTERING_SUNS),
        ("AIR_PHASES", air::SCATTERING_PHASES),
    ] {
        source = source.replace(&format!("__{name}__"), &value.to_string());
    }
    for (name, value) in [
        ("AIR_GROUND", air::GROUND_KM),
        ("AIR_TOP", air::TOP_KM),
        ("AIR_HEIGHT", air::HEIGHT_KM),
        ("AIR_HORIZON", air::TABLES.horizon),
        ("AIR_LOWEST_SUN", air::TABLES.lowest_sun),
        ("AIR_NOON_GREEN", air::TABLES.noon[0]),
        ("AIR_NOON_BLUE", air::TABLES.noon[1]),
        ("AIR_PHASE", air::AIR_PHASE),
        ("HAZE_PHASE", air::HAZE_PHASE),
        ("HAZE_SPREAD", air::HAZE_SPREAD[0]),
        ("HAZE_FORWARD", air::HAZE_SPREAD[1]),
        ("HAZE_GREEN", air::HAZE_GREEN),
        ("HAZE_BLUE", air::HAZE_BLUE),
        ("AIR_EXPOSURE", air::EXPOSURE),
        ("AIR_NADIR_SHARE", air::NADIR_HAZE_SHARE),
        ("AIR_UNLIT_SUN", air::UNLIT_SUN_COSINE),
    ] {
        source = source.replace(&format!("__{name}__"), &format!("{value:?}"));
    }
    for (name, value) in [
        ("FOV", VERTICAL_FOV_HALF_TAN),
        ("RADIUS", EARTH_RADIUS_M),
        ("MERCATOR", tuning::WEB_MERCATOR_MAX_LATITUDE_DEG),
        ("LOD_BIAS", tuning::LOD_SHARPNESS_BIAS),
        ("LOD_LAT_COS", tuning::MIN_LOD_LATITUDE_COSINE),
        ("LOD_INC_COS", tuning::MIN_LOD_INCIDENCE_COSINE),
        ("MIN_BLEND", tuning::MIN_ZOOM_BLEND_FRACTION as f64),
        ("TEXT_ZOOM_FADE_START", tuning::TEXT_ZOOM_FADE_START as f64),
        (
            "IMAGERY_ZOOM_FADE_START",
            tuning::IMAGERY_ZOOM_FADE_START as f64,
        ),
        ("FILTER_EXPONENT_MIN", tuning::TILE_FILTER_RATIO_EXONENT_MIN),
        ("FILTER_EXPONENT_MAX", tuning::TILE_FILTER_RATIO_EXONENT_MAX),
        (
            "MAX_SAMPLE_STRIDE",
            globecraft_data::tiles::Tile::MAX_SAMPLE_STRIDE,
        ),
        ("TWILIGHT_OFFSET", tuning::TWILIGHT_OFFSET),
        ("TWILIGHT_WIDTH", tuning::TWILIGHT_WIDTH),
        ("NIGHT_BRIGHTNESS", tuning::NIGHT_BRIGHTNESS as f64),
        ("DAY_RANGE", tuning::DAY_BRIGHTNESS_RANGE as f64),
        ("NIGHT_BLUE", tuning::NIGHT_BLUE_TINT as f64),
        (
            "MIN_LIGHTS_DARKNESS",
            crate::MIN_NIGHT_LIGHTS_DARKNESS as f64,
        ),
        (
            "POLAR_CAP_START",
            (crate::polar::CAP_START_DEG - POLAR_CAP_MARGIN_DEG).to_radians(),
        ),
    ] {
        source = source.replace(&format!("__{name}__"), &format!("{:?}", value as f32));
    }
    source
}
