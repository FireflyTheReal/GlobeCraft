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
        ("COLOUR_COARSE", crate::colour_match::COARSE_LEVEL as usize),
        ("COLOUR_FINE", crate::colour_match::FINE_LEVEL as usize),
        (
            "OWN_COLOURS",
            crate::colour_match::OWN_COLOURS_LEVEL as usize,
        ),
        ("MEAN_CELLS", globecraft_data::tiles::MEAN_CELLS),
        ("MEANS_PER_TILE", super::MEANS_PER_TILE),
        ("AIR_TRANSMITTANCE_HEIGHTS", air::TRANSMITTANCE_HEIGHTS),
        ("AIR_SCATTERING_HEIGHTS", air::SCATTERING_HEIGHTS),
        ("AERIAL_VIEWS", air::aerial::VIEWS),
        ("AERIAL_AZIMUTHS", air::aerial::AZIMUTHS),
        ("AERIAL_DISTANCES", air::aerial::DISTANCES),
        ("AIR_SCATTERING_AT", super::overview::AIR_SCATTERING_AT),
        ("AIR_INSIDE_AT", super::overview::AIR_INSIDE_AT),
        ("AERIAL_AT", super::overview::AERIAL_AT),
        (
            "UNDRAWN",
            u32::from_le_bytes(super::overview::UNDRAWN) as usize,
        ),
        ("MAP_ZOOM", usize::from(super::overview::MAX_TRACKED_ZOOM)),
        (
            "GLOBAL_MAX_LEVEL",
            usize::from(super::overview::GLOBAL_MAX_LEVEL),
        ),
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
        ("AIR_NIGHT_SUN", air::NIGHT_SUN_COSINE),
        ("GLINT_STRENGTH", crate::sun::GLINT_STRENGTH),
        ("GLINT_SHININESS", crate::sun::GLINT_SHININESS),
        ("GLINT_MIN_HIGHLIGHT", crate::sun::GLINT_MIN_HIGHLIGHT),
        ("GLINT_SUNSET_COSINE", crate::sun::GLINT_SUNSET_COSINE),
        ("AURORA_LOW_KM", crate::aurora::AURORA_LOW_KM),
        ("AURORA_HIGH_KM", crate::aurora::AURORA_HIGH_KM),
        ("AURORA_MIN_CHANCE", crate::aurora::MIN_CHANCE),
        ("AURORA_FULL_CHANCE", crate::aurora::FULL_CHANCE),
        ("AURORA_GLOW_PER_KM", crate::aurora::GLOW_PER_KM),
        ("AURORA_MAX_LIGHT", crate::aurora::MAX_LIGHT),
        ("AURORA_RED", crate::aurora::AURORA_COLOUR[0]),
        ("AURORA_GREEN", crate::aurora::AURORA_COLOUR[1]),
        ("AURORA_BLUE", crate::aurora::AURORA_COLOUR[2]),
        ("AURORA_DAY_COSINE", crate::aurora::DAY_COSINE),
        ("AURORA_NIGHT_COSINE", crate::aurora::NIGHT_COSINE),
        ("AURORA_SAMPLE_KM", crate::aurora::SAMPLE_KM),
        ("AURORA_MAX_SAMPLES", crate::aurora::MAX_SAMPLES as f32),
        ("COLOUR_MAX_GAIN", crate::colour_match::MAX_GAIN),
        ("COLOUR_DARK_FLOOR", crate::colour_match::DARK_FLOOR),
        ("AIR_SEA_LEVEL_SLACK", air::SEA_LEVEL_SLACK_KM),
        ("SUN_BRIGHTNESS", crate::sun::SUN_BRIGHTNESS),
        ("SUN_NEAR_LIGHT", crate::sun::SUN_GLOW_NEAR.0),
        ("SUN_WIDE_LIGHT", crate::sun::SUN_GLOW_WIDE.0),
        ("SUN_RED", crate::sun::SUN_COLOUR.x),
        ("SUN_GREEN", crate::sun::SUN_COLOUR.y),
        ("SUN_BLUE", crate::sun::SUN_COLOUR.z),
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
        ("LIGHT_UP", crate::terrain::LIGHT_UP),
        ("LIGHT_NORTH", crate::terrain::LIGHT_NORTH),
        ("LIGHT_WEST", crate::terrain::LIGHT_WEST),
        ("CHART_BASE", crate::terrain::SHADE_BASE),
        ("CHART_GAIN", crate::terrain::SHADE_GAIN),
        ("CHART_MIN", crate::terrain::MIN_SHADE),
        ("CHART_MAX", crate::terrain::MAX_SHADE),
        ("SUN_DISC", crate::sun::SUN_RADIUS_RAD),
        ("SUN_NEAR_FALL", crate::sun::SUN_GLOW_NEAR.1),
        ("SUN_WIDE_FALL", crate::sun::SUN_GLOW_WIDE.1),
        ("SUN_REACH", crate::sun::SUN_GLOW_REACH_RAD),
        ("TWILIGHT_OFFSET", tuning::TWILIGHT_OFFSET),
        ("TWILIGHT_WIDTH", tuning::TWILIGHT_WIDTH),
        ("NIGHT_BRIGHTNESS", tuning::NIGHT_BRIGHTNESS as f64),
        ("NIGHT_BLUE", tuning::NIGHT_BLUE_TINT as f64),
        ("SUN_RISE_START", tuning::SUN_RISE_START_COSINE),
        ("SUN_RISE_FULL", tuning::SUN_RISE_FULL_COSINE),
        ("SHADE_RISE", tuning::SHADE_RISE as f64),
        ("SHADE_DAY", tuning::SHADE_DAY as f64),
        ("DIRECT_RISE", tuning::DIRECT_RISE as f64),
        ("EYE_TWILIGHT", tuning::EYE_TWILIGHT_COSINE),
        ("SKY_DAY", tuning::SKY_DAY_COSINE),
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
    for (name, [r, g, b]) in [
        ("TWILIGHT_SKY", tuning::TWILIGHT_SKY),
        ("DAY_SKY", tuning::DAY_SKY),
        ("LUMINANCE", tuning::LUMINANCE),
    ] {
        source = source.replace(
            &format!("__{name}__"),
            &format!("vec3<f32>({r:?}, {g:?}, {b:?})"),
        );
    }
    source
}
