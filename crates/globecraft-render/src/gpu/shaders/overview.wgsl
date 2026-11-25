// The globe's ground in f32, from space down to the ground: places on the map from a
// global f32 coordinate to level 13 and from the map near the camera past it (`Spot`).
// The CPU repairs the exact sky, horizon, seams and what the near map does not hold.
struct View {
    camera: vec4<f32>,
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    sun: vec4<f32>, // toward the sun; w: the glint on (1) or off (0)
    shape: vec4<f32>, // 1 / the surface's semi-axes squared; the camera in that metric, squared
    figure: vec4<f32>, // semi-major axis (m), eccentricity squared, the eye's twilight adaptation (`atmosphere::eye_weight`), aurora opacity (0: off)
    raster: vec4<u32>, // width, height, imagery, provider max zoom
    effects: vec4<u32>, // sunlight, atmosphere, level-of-detail raster height, colour matching (1)
    limits: vec4<u32>, // tracked max zoom, the needs' set size (a power of two), table mask, water map width
    lights: vec4<u32>, // night lights shown, their max zoom, unused, tracked max zoom
    // The camera's tile coordinates at `MAP_ZOOM` (x, y): needs are kept relative to the
    // camera's tile at their own zoom.
    origin: vec4<u32>,
    // The camera inside the air (`atmosphere::Frame`): its distance from the centre
    // (km), the sun's cosine there, sqrt(radius^2 - ground^2), and 1 (0: above it).
    air_frame: vec4<f32>,
    // The camera's vertical; w: the integrated table's share low down (0: none).
    air_up: vec4<f32>,
    // The eye's adaptation to twilight on the air's light.
    air_adapt: vec4<f32>,
    // The integrated table (`atmosphere::aerial`): its horizon's sine, reach (km), sun
    // cosine, and 1 when there is one.
    aerial: vec4<f32>,
    // The map near the camera (`relief::NearMap`, in `MAP_ZOOM`'s texels): the
    // camera's texel (whole part; fraction in `near_frac.xy`, range of the near map
    // squared in `near_frac.z`), and the texel coordinates' first (`near_grad_*`, per
    // world unit) and second derivatives (`near_hess_*`: xx, yy, zz, xy; xz, yz).
    // `near_frac.w`: the camera's |c|² − 1 in the surface's metric, from f64.
    near: vec4<i32>,
    near_frac: vec4<f32>,
    near_grad_x: vec4<f32>, near_grad_y: vec4<f32>,
    near_hess_x: vec4<f32>, near_hess_x2: vec4<f32>,
    near_hess_y: vec4<f32>, near_hess_y2: vec4<f32>,
    // The third-order terms (`NearMap::cubic`): x's ten coefficients, then y's.
    near_cubic_0: vec4<f32>, near_cubic_1: vec4<f32>, near_cubic_2: vec4<f32>,
    near_cubic_3: vec4<f32>, near_cubic_4: vec4<f32>,
    // The relief (roadmap D8, G4): 1 when the view has it (the ground then comes from
    // the GPU's walk, `walk`), its fade, its exaggeration; w unused.
    relief: vec4<f32>,
    // The sky: drawn here (1), the camera's distance from the centre (km) and the sun's
    // cosine there (`atmosphere::Frame::sky`).
    sky: vec4<f32>,
}
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var atlas: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> tiles: array<vec4<u32>>;
// The tiles sampled or missed: a hash set of packed keys (`need_key`), then each
// one's nearest pixel distance (f32 bits, `atomicMin`), then an overflow flag.
@group(0) @binding(4) var<storage, read_write> needs: array<atomic<u32>>;
// Each atlas layer's tile's mean colours (`Tile::means`; w -1 without them).
@group(0) @binding(9) var<storage, read> means: array<vec4<f32>>;
// The air (the CPU's `atmosphere`), one buffer: its whole tables
// (`atmosphere::Tables`), transmittance by height (rows) and way to the top
// (columns), from 0, and scattering by height, view, sun and phase, from
// `AIR_SCATTERING_AT`; the air at the camera's height and sun with the camera inside
// it (`atmosphere::Inside`: scattering by view and phase, then the transmittance
// row), from `AIR_INSIDE_AT`; the air integrated along rays for a low camera
// (`atmosphere::aerial`: light and the share crossed for each view row, azimuth
// column and distance slice), from `AERIAL_AT`.
@group(0) @binding(5) var<storage, read> air_tables: array<vec4<f32>>;
// The GPU's relief walk (`relief.wgsl`'s `starts`, planes of one value per pixel):
// start, ground, the sun's share there, trust, slope east and north. Read only with
// the relief on (`view.relief.x`).
@group(0) @binding(6) var<storage, read> walk: array<f32>;
const NO_SLOPE: f32 = 3.4e38;
const SKY_MISS: f32 = -3.4e38;
// A pixel left to the CPU (`overview::UNDRAWN`).
const UNDRAWN: u32 = __UNDRAWN__u;
// A ray misses the ellipsoid for sure past this much of the discriminant.
const SKY_MARGIN: f32 = 1e-5;
// The sun in the sky (`sun::sun_in_sky`).
const SUN_DISC: f32 = __SUN_DISC__;
const SUN_REACH: f32 = __SUN_REACH__;
const SUN_BRIGHTNESS: f32 = __SUN_BRIGHTNESS__;
const SUN_NEAR_LIGHT: f32 = __SUN_NEAR_LIGHT__;
const SUN_NEAR_FALL: f32 = __SUN_NEAR_FALL__;
const SUN_WIDE_LIGHT: f32 = __SUN_WIDE_LIGHT__;
const SUN_WIDE_FALL: f32 = __SUN_WIDE_FALL__;
const SUN_COLOUR: vec3<f32> = vec3<f32>(__SUN_RED__, __SUN_GREEN__, __SUN_BLUE__);
// Hill shading without the sun (`terrain::Slope::chart_light`).
const LIGHT_UP: f32 = __LIGHT_UP__;
const LIGHT_NORTH: f32 = __LIGHT_NORTH__;
const LIGHT_WEST: f32 = __LIGHT_WEST__;
const CHART_BASE: f32 = __CHART_BASE__;
const CHART_GAIN: f32 = __CHART_GAIN__;
const CHART_MIN: f32 = __CHART_MIN__;
const CHART_MAX: f32 = __CHART_MAX__;
const AIR_SCATTERING_AT: u32 = __AIR_SCATTERING_AT__u;
const AIR_INSIDE_AT: u32 = __AIR_INSIDE_AT__u;
const AERIAL_AT: u32 = __AERIAL_AT__u;
// The water map for the sun's glint (`sun::glint`), bytes packed four to a word.
@group(0) @binding(7) var<storage, read> water: array<u32>;
const GLINT_STRENGTH: f32 = __GLINT_STRENGTH__;
const GLINT_SHININESS: f32 = __GLINT_SHININESS__;
const GLINT_MIN_HIGHLIGHT: f32 = __GLINT_MIN_HIGHLIGHT__;
const GLINT_SUNSET_COSINE: f32 = __GLINT_SUNSET_COSINE__;
// The aurora in the air (`aurora::glow`): the forecast's chance by 1° (bytes packed
// four to a word, longitude by longitude from latitude −90°).
@group(0) @binding(8) var<storage, read> aurora: array<u32>;
const AURORA_LOW_KM: f32 = __AURORA_LOW_KM__;
const AURORA_HIGH_KM: f32 = __AURORA_HIGH_KM__;
const AURORA_MIN_CHANCE: f32 = __AURORA_MIN_CHANCE__;
const AURORA_FULL_CHANCE: f32 = __AURORA_FULL_CHANCE__;
const AURORA_GLOW_PER_KM: f32 = __AURORA_GLOW_PER_KM__;
const AURORA_MAX_LIGHT: f32 = __AURORA_MAX_LIGHT__;
const AURORA_COLOUR: vec3<f32> = vec3<f32>(__AURORA_RED__, __AURORA_GREEN__, __AURORA_BLUE__);
const AURORA_DAY_COSINE: f32 = __AURORA_DAY_COSINE__;
const AURORA_NIGHT_COSINE: f32 = __AURORA_NIGHT_COSINE__;
const AURORA_SAMPLE_KM: f32 = __AURORA_SAMPLE_KM__;
const AURORA_MAX_SAMPLES: f32 = __AURORA_MAX_SAMPLES__;

fn smooth01(x: f32) -> f32 {
    let t = clamp(x,0.0,1.0);
    return t*t*(3.0-2.0*t);
}

fn aurora_node(lon: i32, lat: i32) -> f32 {
    let index = u32(((lon % 360) + 360) % 360) * 181u + u32(clamp(lat, 0, 180));
    return f32((aurora[index / 4u] >> ((index % 4u) * 8u)) & 255u);
}
fn aurora_chance(lat_deg: f32, lon_deg: f32) -> f32 {
    let x = lon_deg - floor(lon_deg / 360.0) * 360.0;
    let y = clamp(lat_deg + 90.0, 0.0, 180.0);
    let x0 = floor(x); let y0 = floor(y);
    let fx = x - x0; let fy = y - y0;
    let ix = i32(x0); let iy = i32(y0);
    let top = aurora_node(ix, iy) * (1.0 - fx) + aurora_node(ix + 1, iy) * fx;
    let bottom = aurora_node(ix, iy + 1) * (1.0 - fx) + aurora_node(ix + 1, iy + 1) * fx;
    return top * (1.0 - fy) + bottom * fy;
}
// How much the aurora glows at a point of the shell, as the CPU's `aurora::glow_at`.
fn aurora_at(at: vec3<f32>) -> f32 {
    var night = 1.0;
    if view.effects.x != 0u {
        var t = clamp((AURORA_DAY_COSINE - dot(at, view.sun.xyz)) / (AURORA_DAY_COSINE - AURORA_NIGHT_COSINE), 0.0, 1.0);
        night = t * t * (3.0 - 2.0 * t);
    }
    if night <= 0.0 { return 0.0; }
    let chance = aurora_chance(degrees(asin(clamp(at.z, -1.0, 1.0))), degrees(atan2(at.y, at.x)));
    return pow(clamp((chance - AURORA_MIN_CHANCE) / (AURORA_FULL_CHANCE - AURORA_MIN_CHANCE), 0.0, 1.0), 0.8) * night;
}
// The glowing km along a stretch of the ray, the forecast read every
// AURORA_SAMPLE_KM or so (at most AURORA_MAX_SAMPLES times).
fn aurora_shine(camera: vec3<f32>, dir: vec3<f32>, from_t: f32, to_t: f32, end: f32) -> f32 {
    let start = max(from_t, 0.0); let stop = min(to_t, end);
    if stop <= start { return 0.0; }
    let length_km = (stop - start) * (EARTH_RADIUS_M / 1000.0);
    let samples = u32(clamp(ceil(length_km / AURORA_SAMPLE_KM), 1.0, AURORA_MAX_SAMPLES));
    let step = (stop - start) / f32(samples);
    var seen = 0.0;
    for (var index = 0u; index < samples; index++) {
        seen += aurora_at(normalize(camera + dir * (start + step * (f32(index) + 0.5))));
    }
    return seen / f32(samples) * length_km;
}
// Where the ray meets the sphere `km` up: its distances in and out (x > y: missed).
fn aurora_sphere(camera: vec3<f32>, dir: vec3<f32>, km: f32) -> vec2<f32> {
    let radius = 1.0 + km / (EARTH_RADIUS_M / 1000.0);
    let b = dot(camera, dir);
    let disc = b * b - (dot(camera, camera) - radius * radius);
    if disc <= 0.0 { return vec2<f32>(1.0, 0.0); }
    return vec2<f32>(-b - sqrt(disc), -b + sqrt(disc));
}
fn aurora_glow(camera: vec3<f32>, dir: vec3<f32>, end: f32) -> vec3<f32> {
    let outer = aurora_sphere(camera, dir, AURORA_HIGH_KM);
    if outer.x > outer.y { return vec3<f32>(0.0); }
    let inner = aurora_sphere(camera, dir, AURORA_LOW_KM);
    var km = 0.0;
    if inner.x <= inner.y {
        km = aurora_shine(camera, dir, outer.x, inner.x, end) + aurora_shine(camera, dir, inner.y, outer.y, end);
    } else {
        km = aurora_shine(camera, dir, outer.x, outer.y, end);
    }
    let raw = km * AURORA_GLOW_PER_KM * view.figure.w;
    let light = raw / (1.0 + raw / AURORA_MAX_LIGHT);
    return vec3<f32>(1.0) - exp(-AURORA_COLOUR * light);
}
fn water_texel(x: i32, y: i32) -> f32 {
    let width = i32(view.limits.w);
    let index = u32(y * width + ((x % width) + width) % width);
    return f32((water[index / 4u] >> ((index % 4u) * 8u)) & 255u) / 255.0;
}
// As the CPU's `clouds::sample`: between the four nearest texels, east-west around.
fn water_at(lat_deg: f32, lon_deg: f32) -> f32 {
    let width = f32(view.limits.w);
    let height = width * 0.5;
    let x = (lon_deg + 180.0) / 360.0 * width - 0.5;
    let y = clamp((90.0 - lat_deg) / 180.0 * height - 0.5, 0.0, height - 1.0);
    let x0 = floor(x); let y0 = floor(y);
    let fx = x - x0; let fy = y - y0;
    let ix = i32(x0); let iy = i32(y0);
    let below = min(iy + 1, i32(height) - 1);
    let top = water_texel(ix, iy) * (1.0 - fx) + water_texel(ix + 1, iy) * fx;
    let bottom = water_texel(ix, below) * (1.0 - fx) + water_texel(ix + 1, below) * fx;
    return top * (1.0 - fy) + bottom * fy;
}
const FOV_HALF_TAN: f32 = __FOV__;
const EARTH_RADIUS_M: f32 = __RADIUS__;
const MERCATOR_LIMIT_DEG: f32 = __MERCATOR__;
const TILE_SEAM_PRECISION_MARGIN: f32 = 0.0001;
const MERCATOR_EDGE_EPSILON: f32 = 0.00000011920928955078125;
const HORIZON_PRECISION_MARGIN: f32 = 0.01;
const HORIZON_SHARE: f32 = 1e-4;
// Where the polar caps begin (the CPU draws them, see `polar`), less a margin for f32.
const POLAR_CAP_START: f32 = __POLAR_CAP_START__;
const PI: f32 = 3.141592653589793;
const MISSING: vec3<f32> = __MISSING__;
const LOD_BIAS: f32 = __LOD_BIAS__;
const LOD_LAT_COS: f32 = __LOD_LAT_COS__;
const LOD_INC_COS: f32 = __LOD_INC_COS__;
const MIN_BLEND: f32 = __MIN_BLEND__;
const TEXT_ZOOM_FADE_START: f32 = __TEXT_ZOOM_FADE_START__;
const IMAGERY_ZOOM_FADE_START: f32 = __IMAGERY_ZOOM_FADE_START__;
const FILTER_EXPONENT_MIN: f32 = __FILTER_EXPONENT_MIN__;
const FILTER_EXPONENT_MAX: f32 = __FILTER_EXPONENT_MAX__;
const MAX_SAMPLE_STRIDE: f32 = __MAX_SAMPLE_STRIDE__;
// Texels one axis of an area sample can touch (the CPU's MAX_AREA_SPAN_TEXELS).
const MAX_AREA_SPAN: i32 = 11;
const TWILIGHT_OFFSET: f32 = __TWILIGHT_OFFSET__;
const TWILIGHT_WIDTH: f32 = __TWILIGHT_WIDTH__;
const NIGHT_BRIGHTNESS: f32 = __NIGHT_BRIGHTNESS__;
const NIGHT_BLUE: f32 = __NIGHT_BLUE__;
const SUN_RISE_START: f32 = __SUN_RISE_START__;
const SUN_RISE_FULL: f32 = __SUN_RISE_FULL__;
const SHADE_RISE: f32 = __SHADE_RISE__;
const SHADE_DAY: f32 = __SHADE_DAY__;
const DIRECT_RISE: f32 = __DIRECT_RISE__;
const EYE_TWILIGHT: f32 = __EYE_TWILIGHT__;
const LUMINANCE: vec3<f32> = __LUMINANCE__;
const SKY_DAY: f32 = __SKY_DAY__;
const TWILIGHT_SKY: vec3<f32> = __TWILIGHT_SKY__;
const DAY_SKY: vec3<f32> = __DAY_SKY__;
const AIR_GROUND: f32 = __AIR_GROUND__;
const AIR_TOP: f32 = __AIR_TOP__;
const AIR_HEIGHT: f32 = __AIR_HEIGHT__;
const AIR_HORIZON: f32 = __AIR_HORIZON__;
const AIR_LOWEST_SUN: f32 = __AIR_LOWEST_SUN__;
const AIR_NOON_GREEN: f32 = __AIR_NOON_GREEN__;
const AIR_NOON_BLUE: f32 = __AIR_NOON_BLUE__;
const AIR_ANGLES: u32 = __AIR_ANGLES__u;
const AIR_VIEWS: u32 = __AIR_VIEWS__u;
const AIR_SUNS: u32 = __AIR_SUNS__u;
const AIR_PHASES: u32 = __AIR_PHASES__u;
const AIR_PHASE: f32 = __AIR_PHASE__;
const HAZE_PHASE: f32 = __HAZE_PHASE__;
const HAZE_SPREAD: f32 = __HAZE_SPREAD__;
const HAZE_FORWARD: f32 = __HAZE_FORWARD__;
const HAZE_GREEN: f32 = __HAZE_GREEN__;
const HAZE_BLUE: f32 = __HAZE_BLUE__;
const AIR_EXPOSURE: f32 = __AIR_EXPOSURE__;
const AIR_NADIR_SHARE: f32 = __AIR_NADIR_SHARE__;
const AIR_UNLIT_SUN: f32 = __AIR_UNLIT_SUN__;
const AIR_NIGHT_SUN: f32 = __AIR_NIGHT_SUN__;
const AIR_TINY: f32 = 1.1754944e-38;
const AIR_TRANSMITTANCE_HEIGHTS: u32 = __AIR_TRANSMITTANCE_HEIGHTS__u;
const AIR_SCATTERING_HEIGHTS: u32 = __AIR_SCATTERING_HEIGHTS__u;
const AIR_SEA_LEVEL_SLACK: f32 = __AIR_SEA_LEVEL_SLACK__;
const AERIAL_VIEWS: u32 = __AERIAL_VIEWS__u;
const AERIAL_AZIMUTHS: u32 = __AERIAL_AZIMUTHS__u;
const AERIAL_DISTANCES: u32 = __AERIAL_DISTANCES__u;
// Where the scattering table's top layer starts (rays entering the air from space).
const AIR_TOP_LAYER: u32 = (AIR_SCATTERING_HEIGHTS - 1u) * AIR_VIEWS * AIR_SUNS * AIR_PHASES;
const MIN_LIGHTS_DARKNESS: f32 = __MIN_LIGHTS_DARKNESS__;
// Matching imagery levels' colours (`colour_match`).
const COLOUR_COARSE_U: u32 = __COLOUR_COARSE__u;
const COLOUR_FINE_U: u32 = __COLOUR_FINE__u;
const OWN_COLOURS_U: u32 = __OWN_COLOURS__u;
const COLOUR_MAX_GAIN: f32 = __COLOUR_MAX_GAIN__;
const COLOUR_DARK_FLOOR: f32 = __COLOUR_DARK_FLOOR__;
const MEAN_CELLS_U: u32 = __MEAN_CELLS__u;
const MEANS_PER_TILE_U: u32 = __MEANS_PER_TILE__u;
// Marks the night lights' tiles in the tile table's level word.
const LIGHTS_TAG: u32 = __LIGHTS_TAG__u;
fn blend(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> { return a * (1.0 - t) + b * t; }
// A point's place on the map in `MAP_ZOOM`'s texels: whole texels as integers and
// the fraction, so a level's tile and the place in it keep f32's precision at every
// level. From the global f32 map coordinate the result is the old one bit for bit
// (scaling by powers of two is exact); from the near map (`spot_near`) it holds
// past level `GLOBAL_MAX_LEVEL`, where an f32 map coordinate is a texel or coarser.
struct Spot { whole: vec2<i32>, frac: vec2<f32> }
const MAP_ZOOM: u32 = __MAP_ZOOM__u;
const MAP_BITS: u32 = MAP_ZOOM + 8u;
const GLOBAL_MAX_LEVEL: u32 = __GLOBAL_MAX_LEVEL__u;
fn spot_from_uv(uv: vec2<f32>) -> Spot {
    let p = uv * f32(1u << MAP_BITS);
    let whole = floor(p);
    return Spot(vec2<i32>(whole), p - whole);
}
// The spot of the point `d` from the camera, within the near map's range.
fn spot_near(d: vec3<f32>) -> Spot {
    let hx = view.near_hess_x; let hx2 = view.near_hess_x2;
    let hy = view.near_hess_y; let hy2 = view.near_hess_y2;
    let dd = d * d;
    let cross = vec3<f32>(d.x * d.y, d.x * d.z, d.y * d.z);
    // x³, y³, z³, x²y, x²z, y²x, y²z, z²x, z²y, xyz (`CUBIC_TERMS`).
    let m0 = vec4<f32>(dd.x * d.x, dd.y * d.y, dd.z * d.z, dd.x * d.y);
    let m1 = vec4<f32>(dd.x * d.z, dd.y * d.x, dd.y * d.z, dd.z * d.x);
    let m2 = vec2<f32>(dd.z * d.y, d.x * d.y * d.z);
    let c0 = view.near_cubic_0; let c1 = view.near_cubic_1; let c2 = view.near_cubic_2;
    let c3 = view.near_cubic_3; let c4 = view.near_cubic_4;
    let cubic = vec2<f32>(
        dot(c0, m0) + dot(c1, m1) + dot(c2.xy, m2),
        dot(vec4<f32>(c2.zw, c3.xy), m0) + dot(vec4<f32>(c3.zw, c4.xy), m1) + dot(c4.zw, m2),
    );
    let off = view.near_frac.xy + vec2<f32>(
        dot(view.near_grad_x.xyz, d) + 0.5 * dot(hx.xyz, dd) + dot(vec3<f32>(hx.w, hx2.x, hx2.y), cross),
        dot(view.near_grad_y.xyz, d) + 0.5 * dot(hy.xyz, dd) + dot(vec3<f32>(hy.w, hy2.x, hy2.y), cross),
    ) + cubic;
    let whole = floor(off);
    let side = i32(1u << MAP_BITS);
    let x = ((view.near.x + i32(whole.x)) % side + side) % side;
    return Spot(vec2<i32>(x, view.near.y + i32(whole.y)), off - whole);
}
// The tile at level `z` under a spot, as the CPU sampler's signed floor (not below
// 0 clamped, so the place in it reproduces the Mercator edge's choice).
fn spot_tile(spot: Spot, z: u32) -> vec2<i32> {
    return spot.whole >> vec2<u32>(MAP_BITS - z);
}
// The place in tile `coordinate` of level `z` (0 to 1 inside it).
fn spot_local(spot: Spot, z: u32, coordinate: vec2<i32>) -> vec2<f32> {
    let shift = MAP_BITS - z;
    return (vec2<f32>(spot.whole - (coordinate << vec2<u32>(shift))) + spot.frac) / f32(1u << shift);
}
// `spot_tile` capped at the last tile, as the sampler's key coordinates.
fn spot_coordinate(spot: Spot, z: u32) -> vec2<i32> {
    return min(spot_tile(spot, z), vec2<i32>(i32((1u << z) - 1u)));
}
fn tile_hash(z: u32, x: u32, y: u32) -> u32 { return (z * 73856093u ^ x * 19349663u ^ y * 83492791u) & view.limits.z; }
fn find_tile(z: u32, x: u32, y: u32) -> i32 {
    var index = tile_hash(z, x, y);
    for(var probe = 0u; probe <= view.limits.z; probe++) {
        let tile = tiles[index];
        if tile.x == 0xffffffffu { return -1; }
        if all(tile.xyz == vec3<u32>(z,x,y)) { return i32(tile.w); }
        index = (index + 1u) & view.limits.z;
    }
    return -1;
}
fn sample_tile(uv: vec2<f32>, layer: i32) -> vec3<f32> {
    let dimensions = vec2<i32>(textureDimensions(atlas));
    let p = max(uv * vec2<f32>(dimensions) - vec2<f32>(0.5), vec2<f32>(0.0));
    let low = min(vec2<i32>(p), dimensions - vec2<i32>(1));
    let high = min(low + vec2<i32>(1), dimensions - vec2<i32>(1));
    let f = min(p - vec2<f32>(low), vec2<f32>(1.0));
    let a = textureLoad(atlas, low, layer, 0).rgb * 255.0;
    let b = textureLoad(atlas, vec2<i32>(high.x, low.y), layer, 0).rgb * 255.0;
    let c = textureLoad(atlas, vec2<i32>(low.x, high.y), layer, 0).rgb * 255.0;
    let d = textureLoad(atlas, high, layer, 0).rgb * 255.0;
    return blend(blend(a,b,f.x),blend(c,d,f.x),f.y) / 255.0;
}
// The CPU sampler's area average (`Tile::sample_area`): every texel under a
// `stride`-wide box centred on the sample, weighted by its overlap, so minified
// imagery does not alias on four bilinear taps.
fn sample_area(uv: vec2<f32>, layer: i32, stride: f32) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(atlas));
    let center = uv * size - vec2<f32>(0.5);
    let lower = max(center - vec2<f32>(stride * 0.5), vec2<f32>(0.0));
    let upper = min(center + vec2<f32>(stride * 0.5), size);
    let lo = vec2<i32>(max(floor(lower + vec2<f32>(0.5)), vec2<f32>(0.0)));
    let hi = min(vec2<i32>(floor(upper + vec2<f32>(0.5))) + vec2<i32>(1), vec2<i32>(size));
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var ty = lo.y; ty < min(hi.y, lo.y + MAX_AREA_SPAN); ty++) {
        let wy = max(min(min(f32(ty) + 0.5, center.y + stride * 0.5), size.y)
            - max(max(f32(ty) - 0.5, center.y - stride * 0.5), 0.0), 0.0);
        if wy <= 0.0 { continue; }
        for (var tx = lo.x; tx < min(hi.x, lo.x + MAX_AREA_SPAN); tx++) {
            let wx = max(min(min(f32(tx) + 0.5, center.x + stride * 0.5), size.x)
                - max(max(f32(tx) - 0.5, center.x - stride * 0.5), 0.0), 0.0);
            sum += textureLoad(atlas, vec2<i32>(tx, ty), layer, 0).rgb * 255.0 * wx * wy;
            total += wx * wy;
        }
    }
    if total <= 0.0 {
        let nearest = min(vec2<i32>(max(round(center), vec2<f32>(0.0))), vec2<i32>(size) - vec2<i32>(1));
        return textureLoad(atlas, nearest, layer, 0).rgb;
    }
    return sum / total / 255.0;
}
// Texels per pixel at level `zz` for a pixel whose level band is `base` + `frac`,
// as the CPU's `sample_chain` sizes its area samples (1: bilinear).
fn stride_at(zz: i32, base: i32, frac: f32) -> f32 {
    let exponent = f32(zz - base) + LOD_BIAS - frac;
    if exponent <= FILTER_EXPONENT_MIN { return 1.0; }
    if exponent >= FILTER_EXPONENT_MAX { return MAX_SAMPLE_STRIDE; }
    return exp2(exponent);
}
fn overflow() { atomicStore(&needs[2u * view.limits.y], 1u); }
// A need's key: the source (top bit), the zoom (5 bits) and the tile's offset from the
// camera's tile at that zoom (13 bits each, signed; x round the globe); 0xffffffff
// (empty) is never one, the zoom being at most 20.
fn need_key(source: u32, z: u32, coord: vec2<u32>) -> u32 {
    let n = 1u << z;
    let camera = view.origin.xy >> vec2<u32>(MAP_ZOOM - z);
    var dx = i32((coord.x - camera.x) & (n - 1u));
    if dx >= i32(n / 2u) { dx -= i32(n); }
    let dy = i32(coord.y) - i32(camera.y);
    if abs(dx) >= 4096 || abs(dy) >= 4096 { return 0xffffffffu; }
    return (source << 31u) | (z << 26u) | ((u32(dx) & 0x1fffu) << 13u) | (u32(dy) & 0x1fffu);
}
// Notes tile `coord` of `source` at zoom `z` as sampled by a pixel `distance` from the
// view's centre (the nearest one counts).
fn need(source: u32, z: u32, coord: vec2<u32>, distance: f32) {
    let key = need_key(source, z, coord);
    if key == 0xffffffffu { overflow(); return; }
    let size = view.limits.y;
    let priority = bitcast<u32>(distance);
    var index = ((key * 2654435761u) >> 12u) & (size - 1u);
    for (var probe = 0u; probe < size; probe++) {
        var held = atomicLoad(&needs[index]);
        if held == 0xffffffffu {
            let swap = atomicCompareExchangeWeak(&needs[index], 0xffffffffu, key);
            held = swap.old_value;
            // Lost to a spurious failure: the slot is still empty, try it again.
            if !swap.exchanged && held == 0xffffffffu { probe--; continue; }
            if swap.exchanged { held = key; }
        }
        if held == key {
            if priority < atomicLoad(&needs[size + index]) { atomicMin(&needs[size + index], priority); }
            return;
        }
        index = (index + 1u) & (size - 1u);
    }
    overflow();
}
// The mean colour of the tile at level `z` under `spot` over `cells` × `cells` blocks,
// between the four nearest (`Tile::mean_colour`); w -1 when the tile is not held or
// has none. Noted as a need, as the CPU asks for it.
fn mean_colour(z: u32, spot: Spot, distance: f32, cells: u32) -> vec4<f32> {
    let coordinate = spot_coordinate(spot, z);
    let coord = vec2<u32>(max(coordinate, vec2<i32>(0)));
    need(0u, z, coord, distance);
    let layer = find_tile(z, coord.x, coord.y);
    if layer < 0 { return vec4<f32>(-1.0); }
    // The grid of `cells` comes after the finer ones.
    var start = u32(layer) * MEANS_PER_TILE_U;
    var finer = MEAN_CELLS_U;
    while finer > cells { start += finer * finer; finer /= 2u; }
    if means[start].w < 0.0 { return vec4<f32>(-1.0); }
    let last = f32(cells - 1u);
    let p = clamp(spot_local(spot, z, coordinate) * f32(cells) - vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(last));
    let low = vec2<u32>(p);
    let high = min(low + vec2<u32>(1u), vec2<u32>(cells - 1u));
    let f = p - vec2<f32>(low);
    let top = mix(means[start + low.y * cells + low.x], means[start + low.y * cells + high.x], f.x);
    let bottom = mix(means[start + high.y * cells + low.x], means[start + high.y * cells + high.x], f.x);
    return mix(top, bottom, f.y);
}
// `colour`, sampled at level `z`, brought toward level 11's colours where `spot` is
// (`colour_match::matched`): unchanged until both level 11's and 12's tiles there are
// held, and for other maps than Esri's imagery.
fn matched(colour: vec3<f32>, z: u32, spot: Spot, distance: f32) -> vec3<f32> {
    if view.effects.w == 0u || z < COLOUR_FINE_U { return colour; }
    let weight = clamp(f32(OWN_COLOURS_U - min(z, OWN_COLOURS_U)) / f32(OWN_COLOURS_U - COLOUR_FINE_U), 0.0, 1.0);
    if weight <= 0.0 { return colour; }
    let coarse = mean_colour(COLOUR_COARSE_U, spot, distance, MEAN_CELLS_U);
    if coarse.w < 0.0 { return colour; }
    let fine = mean_colour(COLOUR_FINE_U, spot, distance, MEAN_CELLS_U / 2u);
    if fine.w < 0.0 { return colour; }
    let floor = vec3<f32>(COLOUR_DARK_FLOOR);
    var gain = clamp((coarse.rgb + floor) / (fine.rgb + floor), vec3<f32>(1.0 / COLOUR_MAX_GAIN), vec3<f32>(COLOUR_MAX_GAIN));
    gain = vec3<f32>(1.0) + (gain - vec3<f32>(1.0)) * weight;
    return min(colour * gain, vec3<f32>(1.0));
}
// The colour of `source` (0: the base map, 1: the night lights) from level `zoom` or
// the nearest coarser level loaded; `w` is 1 when a level was found.
fn chain(source: u32, zoom: u32, spot: Spot, distance: f32, base: u32, frac: f32) -> vec4<f32> {
    let tag = select(0u, LIGHTS_TAG, source == 1u);
    let tracked = select(view.limits.x, view.lights.w, source == 1u);
    var z = i32(zoom);
    loop {
        // Preserve the CPU sampler's signed floor before saturating key coordinates.
        let coordinate = spot_coordinate(spot, u32(z));
        let coord = vec2<u32>(max(coordinate,vec2<i32>(0)));
        // Track sampled keys as well as misses, so residency follows this view
        // instead of growing with the entire decoded cache.
        if u32(z) <= tracked { need(source, u32(z), coord, distance); } else { overflow(); }
        let layer = find_tile(u32(z) | tag,coord.x,coord.y);
        if layer >= 0 {
            let local = spot_local(spot, u32(z), coordinate);
            let stride = stride_at(z, i32(base), frac);
            // Bilinear where the level is not minified; the area average (a loop over
            // up to 11 × 11 texels) only where it is.
            var colour: vec3<f32>;
            if stride <= 1.0 { colour = sample_tile(local,layer); }
            else { colour = sample_area(local,layer,min(stride,MAX_SAMPLE_STRIDE)); }
            if source == 0u { colour = matched(colour, u32(z), spot, distance); }
            return vec4<f32>(colour,1.0);
        }
        z -= 1;
        if z < 0 { break; }
    }
    return vec4<f32>(MISSING,0.0);
}
// The air functions repeat `atmosphere::Tables::ground_from_space` sum for sum.
// A table position: the cell below it (at most the last but one) and the way to the next.
fn air_cell(position: f32, cells: u32) -> vec2<f32> {
    let p = clamp(position,0.0,f32(cells-1u));
    let below = min(floor(p),f32(cells-2u));
    return vec2<f32>(below,p-below);
}
fn air_lerp(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> { return a + (b - a) * t; }
// From the ground to the top of the air along a ray at this cosine to the vertical.
fn air_up(cosine: f32) -> f32 {
    return AIR_HORIZON*AIR_HORIZON / (AIR_GROUND*cosine + sqrt(AIR_GROUND*AIR_GROUND*cosine*cosine + AIR_HORIZON*AIR_HORIZON));
}
// The share of light that crosses the air along a ray of that length from the ground.
fn air_crossed(up: f32) -> vec4<f32> {
    let c = air_cell(clamp((up-AIR_HEIGHT)/(AIR_HORIZON-AIR_HEIGHT),0.0,1.0)*f32(AIR_ANGLES-1u),AIR_ANGLES);
    let i = u32(c.x);
    return air_lerp(air_tables[i],air_tables[i+1u],c.y);
}
fn air_entry(view: u32, sun: u32, phase: vec2<f32>) -> vec4<f32> {
    let first = AIR_TOP_LAYER+(view*AIR_SUNS+sun)*AIR_PHASES+u32(phase.x);
    return air_lerp(air_tables[AIR_SCATTERING_AT + first],air_tables[AIR_SCATTERING_AT + first+1u],phase.y);
}
// Sunlight scattered toward the eye in front of ground seen from space.
fn air_scattered(up: f32, mu_s: f32, nu: f32) -> vec3<f32> {
    let way = clamp((up-AIR_HEIGHT)/(AIR_HORIZON-AIR_HEIGHT),0.0,1.0);
    let views = AIR_VIEWS/2u;
    let view = air_cell(f32(views-1u)*(1.0-way),views);
    let to_top = max(-AIR_GROUND*mu_s + sqrt(max(AIR_GROUND*AIR_GROUND*(mu_s*mu_s-1.0) + AIR_TOP*AIR_TOP,0.0)),0.0);
    let a = (to_top-AIR_HEIGHT)/(AIR_HORIZON-AIR_HEIGHT);
    let sun = air_cell(max(1.0-a/AIR_LOWEST_SUN,0.0)/(1.0+a)*f32(AIR_SUNS-1u),AIR_SUNS);
    let phase = air_cell((nu+1.0)*0.5*f32(AIR_PHASES-1u),AIR_PHASES);
    let v = u32(view.x); let s = u32(sun.x);
    let entry = air_lerp(
        air_lerp(air_entry(v,s,phase),air_entry(v,s+1u,phase),sun.y),
        air_lerp(air_entry(v+1u,s,phase),air_entry(v+1u,s+1u,phase),sun.y),
        view.y);
    return air_radiance(entry, nu);
}
// The scattered light of a table entry (`atmosphere::radiance`): air in rgb, haze from
// its red in w.
fn air_radiance(entry: vec4<f32>, nu: f32) -> vec3<f32> {
    let spread = HAZE_SPREAD - HAZE_FORWARD*nu;
    let both = 1.0 + nu*nu;
    var haze = vec3<f32>(0.0);
    if entry.x > 0.0 { let ratio = entry.w/entry.x; haze = vec3<f32>(entry.w,entry.y*ratio*HAZE_GREEN,entry.z*ratio*HAZE_BLUE); }
    return entry.xyz*(AIR_PHASE*both) + haze*(HAZE_PHASE*both/(spread*sqrt(spread)));
}
// The camera inside the air (`atmosphere::Frame::ground`), sum for sum.
fn distance_to_top(r: f32, mu: f32) -> f32 {
    return max(-r * mu + sqrt(max(r * r * (mu * mu - 1.0) + AIR_TOP * AIR_TOP, 0.0)), 0.0);
}
fn distance_to_ground(r: f32, mu: f32) -> f32 {
    return max(-r * mu - sqrt(max(r * r * (mu * mu - 1.0) + AIR_GROUND * AIR_GROUND, 0.0)), 0.0);
}
fn meets_ground(r: f32, mu: f32) -> bool {
    return mu < 0.0 && r * r * (mu * mu - 1.0) + AIR_GROUND * AIR_GROUND >= 0.0;
}
// The radius and the view and sun cosines `d` further along a ray.
fn air_along(r: f32, mu: f32, mu_s: f32, nu: f32, d: f32) -> vec3<f32> {
    let r_d = clamp(sqrt(d * d + 2.0 * r * mu * d + r * r), AIR_GROUND, AIR_TOP);
    return vec3<f32>(r_d, clamp((r * mu + d) / r_d, -1.0, 1.0), clamp((r * mu_s + d * nu) / r_d, -1.0, 1.0));
}
fn sun_coordinate(mu_s: f32) -> f32 {
    let a = (distance_to_top(AIR_GROUND, mu_s) - AIR_HEIGHT) / (AIR_HORIZON - AIR_HEIGHT);
    return max(1.0 - a / AIR_LOWEST_SUN, 0.0) / (1.0 + a);
}
// The share of light that reaches the top of the air from radius `r` along `mu`.
fn air_to_top(r: f32, mu: f32) -> vec4<f32> {
    let rho = sqrt(max(r * r - AIR_GROUND * AIR_GROUND, 0.0));
    let d_min = AIR_TOP - r; let d_max = rho + AIR_HORIZON;
    let column = air_cell((distance_to_top(r, mu) - d_min) / (d_max - d_min) * f32(AIR_ANGLES - 1u), AIR_ANGLES);
    let row = air_cell(rho / AIR_HORIZON * f32(AIR_TRANSMITTANCE_HEIGHTS - 1u), AIR_TRANSMITTANCE_HEIGHTS);
    let c = u32(column.x); let w = u32(row.x) * AIR_ANGLES;
    return air_lerp(
        air_lerp(air_tables[w + c], air_tables[w + c + 1u], column.y),
        air_lerp(air_tables[w + AIR_ANGLES + c], air_tables[w + AIR_ANGLES + c + 1u], column.y),
        row.y);
}
// The scattering table's entry for a ray (`Tables::scattering`).
fn air_scattering_at(r: f32, mu: f32, mu_s: f32, nu: f32, to_ground: bool) -> vec4<f32> {
    let rho = sqrt(max(r * r - AIR_GROUND * AIR_GROUND, 0.0));
    let views = AIR_VIEWS / 2u;
    var view: vec2<f32>;
    if to_ground {
        let d_min = r - AIR_GROUND; let d_max = rho;
        var way = 0.0;
        if d_max != d_min { way = (distance_to_ground(r, mu) - d_min) / (d_max - d_min); }
        view = air_cell(f32(views - 1u) * (1.0 - way), views);
    } else {
        let d_min = AIR_TOP - r; let d_max = rho + AIR_HORIZON;
        view = air_cell(f32(views - 1u) * (distance_to_top(r, mu) - d_min) / (d_max - d_min), views);
        view.x += f32(views);
    }
    let height = air_cell(rho / AIR_HORIZON * f32(AIR_SCATTERING_HEIGHTS - 1u), AIR_SCATTERING_HEIGHTS);
    let sun = air_cell(sun_coordinate(mu_s) * f32(AIR_SUNS - 1u), AIR_SUNS);
    let phase = air_cell((nu + 1.0) * 0.5 * f32(AIR_PHASES - 1u), AIR_PHASES);
    let h = u32(height.x);
    if height.y == 0.0 { return air_layer(h, u32(view.x), view.y, u32(sun.x), sun.y, phase); }
    if height.y == 1.0 { return air_layer(h + 1u, u32(view.x), view.y, u32(sun.x), sun.y, phase); }
    return air_lerp(
        air_layer(h, u32(view.x), view.y, u32(sun.x), sun.y, phase),
        air_layer(h + 1u, u32(view.x), view.y, u32(sun.x), sun.y, phase),
        height.y);
}
fn air_at(height: u32, view: u32, sun: u32, phase: vec2<f32>) -> vec4<f32> {
    let first = ((height * AIR_VIEWS + view) * AIR_SUNS + sun) * AIR_PHASES + u32(phase.x);
    return air_lerp(air_tables[AIR_SCATTERING_AT + first], air_tables[AIR_SCATTERING_AT + first + 1u], phase.y);
}
fn air_layer(height: u32, view: u32, view_step: f32, sun: u32, sun_step: f32, phase: vec2<f32>) -> vec4<f32> {
    return air_lerp(
        air_lerp(air_at(height, view, sun, phase), air_at(height, view, sun + 1u, phase), sun_step),
        air_lerp(air_at(height, view + 1u, sun, phase), air_at(height, view + 1u, sun + 1u, phase), sun_step),
        view_step);
}
// The sunlight's colour after crossing the air (`Tables::sunlight`).
fn air_sunlight(transmitted: vec4<f32>) -> vec3<f32> {
    let red = max(transmitted.x, AIR_TINY);
    return sqrt(min(vec3<f32>(1.0, transmitted.y / red * AIR_NOON_GREEN, transmitted.z / red * AIR_NOON_BLUE), vec3<f32>(1.0)));
}
// `Inside`: how far along its half of the views a ray that leaves the air lies, and
// one that reaches the ground.
fn inside_way_out(r: f32, mu: f32) -> f32 {
    let d_min = AIR_TOP - r; let d_max = view.air_frame.z + AIR_HORIZON;
    return (distance_to_top(r, mu) - d_min) / (d_max - d_min);
}
fn inside_way_down(r: f32, mu: f32) -> f32 {
    let d_min = r - AIR_GROUND; let d_max = view.air_frame.z;
    if d_max == d_min { return 0.0; }
    return (distance_to_ground(r, mu) - d_min) / (d_max - d_min);
}
fn inside_crossed(way: f32) -> vec4<f32> {
    let c = air_cell(way * f32(AIR_ANGLES - 1u), AIR_ANGLES);
    let first = AIR_VIEWS * AIR_PHASES + u32(c.x);
    return air_lerp(air_tables[AIR_INSIDE_AT + first], air_tables[AIR_INSIDE_AT + first + 1u], c.y);
}
fn inside_scattered(way: f32, to_ground: bool, nu: f32) -> vec4<f32> {
    let views = AIR_VIEWS / 2u;
    var view = air_cell(f32(views - 1u) * way, views);
    if to_ground { view = air_cell(f32(views - 1u) * (1.0 - way), views); } else { view.x += f32(views); }
    let phase = air_cell((nu + 1.0) * 0.5 * f32(AIR_PHASES - 1u), AIR_PHASES);
    let a = u32(view.x) * AIR_PHASES + u32(phase.x);
    let b = a + AIR_PHASES;
    return air_lerp(
        air_lerp(air_tables[AIR_INSIDE_AT + a], air_tables[AIR_INSIDE_AT + a + 1u], phase.y),
        air_lerp(air_tables[AIR_INSIDE_AT + b], air_tables[AIR_INSIDE_AT + b + 1u], phase.y),
        view.y);
}
// The integrated table's light and share crossed (`aerial::Aerial::sample`).
struct AerialLight { light: vec4<f32>, crossed: vec4<f32> }
fn aerial_corner(at: f32, count: u32) -> vec2<f32> {
    let low = min(floor(at), f32(count - 2u));
    return vec2<f32>(low, at - low);
}
fn aerial_sample(mu: f32, nu: f32, distance_km: f32) -> AerialLight {
    let sun_cosine = view.aerial.z;
    let sun_across = sqrt(max(1.0 - sun_cosine * sun_cosine, 0.0));
    let across = sqrt(max(1.0 - mu * mu, 0.0)) * sun_across;
    var azimuth = 1.0;
    if across > 1e-6 { azimuth = clamp((nu - mu * sun_cosine) / across, -1.0, 1.0); }
    let horizon = asin(view.aerial.x);
    let angle = asin(clamp(mu, -1.0, 1.0)) - horizon;
    var side = PI / 2.0 - horizon;
    if angle < 0.0 { side = PI / 2.0 + horizon; }
    var v = sqrt(abs(angle) / side);
    if angle < 0.0 { v = -v; }
    let row = aerial_corner(clamp((v + 1.0) * 0.5 * f32(AERIAL_VIEWS - 1u), 0.0, f32(AERIAL_VIEWS - 1u)), AERIAL_VIEWS);
    let column = aerial_corner(clamp(sqrt(clamp(acos(azimuth) / PI, 0.0, 1.0)) * f32(AERIAL_AZIMUTHS - 1u), 0.0, f32(AERIAL_AZIMUTHS - 1u)), AERIAL_AZIMUTHS);
    let slice = aerial_corner(clamp(sqrt(clamp(distance_km / view.aerial.y, 0.0, 1.0)) * f32(AERIAL_DISTANCES - 1u), 0.0, f32(AERIAL_DISTANCES - 1u)), AERIAL_DISTANCES);
    var result = AerialLight(vec4<f32>(0.0), vec4<f32>(0.0));
    for (var dr = 0u; dr < 2u; dr++) {
        for (var dc = 0u; dc < 2u; dc++) {
            for (var ds = 0u; ds < 2u; ds++) {
                let weight = select(1.0 - row.y, row.y, dr == 1u) * select(1.0 - column.y, column.y, dc == 1u) * select(1.0 - slice.y, slice.y, ds == 1u);
                let cell = ((u32(row.x) + dr) * AERIAL_AZIMUTHS + u32(column.x) + dc) * AERIAL_DISTANCES + u32(slice.x) + ds;
                result.light += air_tables[AERIAL_AT + 2u * cell] * weight;
                result.crossed += air_tables[AERIAL_AT + 2u * cell + 1u] * weight;
            }
        }
    }
    return result;
}
// The air in front of ground `distance_km` along `dir` with the camera inside it:
// scattered light, the share crossed, the sunlight's colour.
struct InsideAir { scattered: vec3<f32>, crossed: vec3<f32>, sunlight: vec3<f32> }
fn inside_ground(dir: vec3<f32>, distance_km: f32, sunlit: bool) -> InsideAir {
    let r = view.air_frame.x;
    let sun_cosine = view.air_frame.y;
    let mu = dot(dir, view.air_up.xyz);
    var nu = mu * sun_cosine;
    if sunlit { nu = dot(dir, view.sun.xyz); }
    let adapt = view.air_adapt.rgb;
    let share = view.air_up.w;
    var aerial = AerialLight(vec4<f32>(0.0), vec4<f32>(1.0));
    if view.aerial.w > 0.0 { aerial = aerial_sample(mu, nu, distance_km); }
    let to_ground = meets_ground(r, mu);
    let at = air_along(r, mu, sun_cosine, nu, distance_km);
    var sunlight = vec3<f32>(1.0);
    if sunlit { sunlight = air_sunlight(air_to_top(at.x, max(at.z, 0.0))); }
    if view.aerial.w > 0.0 && share >= 1.0 {
        return InsideAir(aerial.light.rgb * adapt, aerial.crossed.rgb, sunlight);
    }
    var whole: vec4<f32>; var rest: vec4<f32>;
    if to_ground {
        whole = air_to_top(at.x, -at.y);
        rest = inside_crossed(inside_way_out(r, -mu));
    } else {
        whole = inside_crossed(inside_way_out(r, mu));
        rest = air_to_top(at.x, at.y);
    }
    var transmitted = min(whole / max(rest, vec4<f32>(AIR_TINY)), vec4<f32>(1.0));
    var way = inside_way_out(r, mu);
    if to_ground { way = inside_way_down(r, mu); }
    var near = inside_scattered(way, to_ground, nu);
    if at.x > AIR_GROUND + AIR_SEA_LEVEL_SLACK {
        let beyond = air_scattering_at(at.x, at.y, at.z, nu, to_ground);
        let reached = vec4<f32>(transmitted.x, transmitted.y, transmitted.z, transmitted.x);
        near = max(near - beyond * reached, vec4<f32>(0.0));
    }
    var scattered = air_radiance(near, nu);
    if view.aerial.w > 0.0 {
        scattered = scattered + (aerial.light.rgb - scattered) * share;
        transmitted = transmitted + (aerial.crossed - transmitted) * share;
    }
    return InsideAir(scattered * adapt, transmitted.rgb, sunlight);
}
// The colour (as drawn, premultiplied) and alpha of a ray that misses the Earth, for
// a sky drawn behind the frame (`pixel::sky` with `sky_behind`, no clouds below): the
// air's glow (`atmosphere::Frame::sky`, `Air::behind`), the sun through it
// (`sun::sun_in_sky`), the aurora. `mu`: the ray's cosine to the camera's vertical in
// the surface's measure.
fn sky_colour(camera: vec3<f32>, dir: vec3<f32>, mu: f32) -> vec4<f32> {
    let sunlit = view.effects.x != 0u;
    let sun = view.sun.xyz;
    var scattered = vec3<f32>(0.0);
    var through = vec3<f32>(1.0);
    var air = false;
    if view.effects.y != 0u {
        let r = view.sky.y;
        let sun_cosine = view.sky.z;
        if view.air_frame.w > 0.0 {
            var nu = mu * sun_cosine;
            if sunlit { nu = dot(dir, sun); }
            let way = inside_way_out(r, mu);
            scattered = air_radiance(inside_scattered(way, false, nu), nu) * view.air_adapt.rgb;
            through = inside_crossed(way).rgb;
            air = true;
        } else {
            let reach = r * r * (mu * mu - 1.0) + AIR_TOP * AIR_TOP;
            if mu < 0.0 && reach >= 0.0 {
                let entry = -r * mu - sqrt(reach);
                let mu_top = clamp((r * mu + entry) / AIR_TOP, -1.0, 1.0);
                var mu_s = sun_cosine;
                var nu = mu_top * sun_cosine;
                if sunlit {
                    nu = dot(dir, sun);
                    mu_s = clamp((r * sun_cosine + entry * nu) / AIR_TOP, -1.0, 1.0);
                }
                scattered = air_radiance(air_scattering_at(AIR_TOP, mu_top, mu_s, nu, false), nu);
                through = air_to_top(AIR_TOP, mu_top).rgb;
                air = true;
            }
        }
    }
    var glow = vec4<f32>(0.0);
    if air {
        let light = scattered * AIR_EXPOSURE;
        let toned = light / (vec3<f32>(1.0) + light);
        let dark = 1.0 - max(max(toned.x, toned.y), toned.z);
        glow = vec4<f32>(sqrt(toned), 1.0 - sqrt((through.x + through.y + through.z) / 3.0) * dark * dark);
    }
    var disc = vec3<f32>(0.0);
    if sunlit && dot(dir, sun) > cos(SUN_REACH) {
        // The angle from the sun's centre without the cancellation of acos near 1.
        let angle = 2.0 * asin(min(length(dir - sun) * 0.5, 1.0));
        var light = 0.0;
        if angle <= SUN_DISC { light = 1.0; } else if angle < SUN_REACH {
            let past = angle - SUN_DISC;
            light = min(SUN_NEAR_LIGHT * exp(-past / SUN_NEAR_FALL) + SUN_WIDE_LIGHT * exp(-past / SUN_WIDE_FALL), 1.0);
        }
        disc = SUN_COLOUR * min(through * SUN_BRIGHTNESS, vec3<f32>(1.0)) * light;
    }
    var aurora = vec3<f32>(0.0);
    if view.figure.w > 0.0 { aurora = aurora_glow(camera, dir, 3.4e38); }
    let lit = min(glow.rgb + disc, vec3<f32>(1.0));
    let open = lit + aurora * (vec3<f32>(1.0) - lit);
    return vec4<f32>(open, max(glow.w, max(max(disc.x, disc.y), disc.z)));
}
// A drawn pixel: never `UNDRAWN` (a sky that comes out so moves by a level of green).
fn put(i: u32, colour: vec4<f32>) {
    let c = vec4<u32>(clamp(colour, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0);
    var word = c.r | (c.g << 8u) | (c.b << 16u) | (c.a << 24u);
    if word == UNDRAWN { word += 256u; }
    output[i] = word;
}
// The sky's light and the share of the sun's a surface facing it takes, for the sun's
// cosine `e` over ground whose own horizon lies `dip` (radians) below the level one
// (`pixel::Daylight::below`; `raised` with a dip): the sun rises and the sky
// brightens that much earlier there.
struct Day { sky: vec3<f32>, facing: f32 }
fn day(e: f32, dip: f32) -> Day {
    let over = e + dip;
    // The eye adapted to twilight (`view.figure.z`) keeps the blue hour longer.
    let night_end = TWILIGHT_OFFSET+(EYE_TWILIGHT-TWILIGHT_OFFSET)*view.figure.z;
    var shaded = SHADE_RISE+(SHADE_DAY-SHADE_RISE)*smooth01((over-SUN_RISE_START)/(SKY_DAY-SUN_RISE_START));
    if over <= SUN_RISE_START {
        shaded = NIGHT_BRIGHTNESS+(SHADE_RISE-NIGHT_BRIGHTNESS)*smooth01((over+night_end)/(SUN_RISE_START+night_end));
    }
    let risen = smooth01((over-SUN_RISE_START)/(SUN_RISE_FULL-SUN_RISE_START));
    let facing = risen*(DIRECT_RISE+(1.0-SHADE_DAY-DIRECT_RISE)*smooth01(e/SKY_DAY));
    let colour = mix(TWILIGHT_SKY,DAY_SKY,smooth01(e/SKY_DAY));
    return Day(colour/dot(colour,LUMINANCE)*shaded, facing);
}
// `terrain::Slope::chart_light`: the slope (unfaded) under a fixed light, faded.
fn chart_light(slope: vec2<f32>, fade: f32) -> f32 {
    let light_length = sqrt(LIGHT_UP*LIGHT_UP + LIGHT_NORTH*LIGHT_NORTH + LIGHT_WEST*LIGHT_WEST);
    let flat = LIGHT_UP / light_length;
    let slope_length = sqrt(1.0 + dot(slope, slope));
    let lit = max((LIGHT_UP + slope.x*LIGHT_WEST - slope.y*LIGHT_NORTH) / (slope_length*light_length), 0.0);
    let shade = clamp(CHART_BASE + CHART_GAIN*(lit - flat), CHART_MIN, CHART_MAX);
    return 1.0 + (shade - 1.0)*fade;
}
// Workgroups in rows of at most 65,535 (one axis holds no more): frames past 16.8
// million pixels (a 6K display) were refused.
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.y * groups.x * 256u + id.x;
    let width = view.raster.x; let height = view.raster.y;
    if i >= width * height { return; }
    let x = i % width; let y = i / width;
    let nx = (f32(x)+0.5) / f32(width) * 2.0 - 1.0;
    let ny = 1.0 - (f32(y)+0.5) / f32(height) * 2.0;
    let dir = normalize(view.right.xyz * (nx * view.camera.w * FOV_HALF_TAN) + view.up.xyz * (ny * FOV_HALF_TAN) - view.forward.xyz);
    // A sphere is the ellipsoid whose axes are all one: `shape` is then (1, 1, 1, |camera|²).
    let camera = view.camera.xyz;
    let q = view.shape.xyz;
    let a = dot(dir * dir, q);
    let bb = dot(dir, camera * q);
    // The camera's height term from f64, and the nearer root in the form that does not
    // subtract two values of about one: close to the ground both give metres.
    let height_term = view.near_frac.w;
    var t = 0.0;
    // With the relief: the ground the walk met, where the CPU would take it as its own
    // and the walk held what shading it takes; every other pixel is the CPU's.
    let relief_on = view.relief.x > 0.0;
    var slope = vec2<f32>(0.0);
    var seen = 1.0;
    if relief_on {
        let count = width * height;
        t = walk[count + i];
        seen = walk[2u * count + i];
        slope = vec2<f32>(walk[4u * count + i], walk[5u * count + i]);
        // The sky, where the walk surely met no ground.
        if slope.x <= SKY_MISS {
            let disc = bb * bb - a * height_term;
            if view.sky.x > 0.0 && (bb >= 0.0 || disc < -SKY_MARGIN) {
                put(i, sky_colour(camera, dir, bb / sqrt(a * view.shape.w)));
            } else { output[i] = UNDRAWN; }
            return;
        }
        if t < 0.0 || walk[3u * count + i] < 0.5 || slope.x >= NO_SLOPE || (view.effects.x != 0u && seen < 0.0) {
            output[i] = UNDRAWN; return;
        }
    } else {
        let disc = bb * bb - a * height_term;
        if bb >= 0.0 || disc < -SKY_MARGIN {
            if view.sky.x > 0.0 { put(i, sky_colour(camera, dir, bb / sqrt(a * view.shape.w))); } else { output[i] = UNDRAWN; }
            return;
        }
        // Near the horizon the root's f32 error grows as 1/sqrt(disc) of bb's: within a
        // hundredth of |bb| the CPU draws (up to 6e-6 of the distance off before).
        if disc < min(HORIZON_PRECISION_MARGIN, HORIZON_SHARE * bb * bb) { output[i] = UNDRAWN; return; }
        t = height_term / (-bb + sqrt(disc));
    }
    let point = camera + dir * t;
    let normal = normalize(point * q);
    let incidence = max(-dot(normal,dir),0.0);
    let latitude_cosine = length(normal.xy);
    let lat = atan2(normal.z,latitude_cosine);
    let lon = atan2(normal.y,normal.x);
    // The polar caps have their own imagery, projection and colour matching, and the
    // night lights their own fill past their tiles: the precise CPU repair draws them
    // (a small share of an overview).
    if (view.raster.z != 0u || view.lights.x != 0u) && abs(lat) > POLAR_CAP_START { output[i] = UNDRAWN; return; }
    let mpp = t * EARTH_RADIUS_M * (2.0 * FOV_HALF_TAN / f32(view.effects.z)) / sqrt(max(incidence,LOD_INC_COS));
    // The radius of the parallel's circle: the prime vertical radius times cos(latitude).
    let prime_vertical = view.figure.x / sqrt(1.0 - view.figure.y * normal.z * normal.z);
    let lod = log2(2.0 * PI * prime_vertical * max(latitude_cosine,LOD_LAT_COS) / (256.0 * mpp)) + LOD_BIAS;
    var zl = floor(lod); var fraction = lod - zl;
    let z = u32(clamp(zl,0.0,f32(view.raster.w)));
    if z >= view.raster.w { fraction = 0.0; }
    // As the CPU: imagery fades over the last 30% of the zoom band (its base is
    // oversampled), text maps over the last 10%, so labels stay sharp.
    var fade = clamp((fraction - IMAGERY_ZOOM_FADE_START) / (1.0 - IMAGERY_ZOOM_FADE_START), 0.0, 1.0);
    if view.raster.z == 0u { fade = clamp((fraction - TEXT_ZOOM_FADE_START) / (1.0 - TEXT_ZOOM_FADE_START), 0.0, 1.0); }
    let latc = clamp(lat,-MERCATOR_LIMIT_DEG * PI/180.0,MERCATOR_LIMIT_DEG * PI/180.0);
    let s = select(sin(latc),normal.z,lat == latc);
    var v = 0.5 - log((1.0+s)/(1.0-s))/(4.0*PI);
    // The f64 Mercator clamp lands just beyond [0,1]. Reproduce its
    // tile-local edge choice instead of silently changing polar imagery.
    if lat >= MERCATOR_LIMIT_DEG*PI/180.0 { v = -MERCATOR_EDGE_EPSILON; }
    if lat <= -MERCATOR_LIMIT_DEG*PI/180.0 { v = 1.0+MERCATOR_EDGE_EPSILON; }
    let uv = vec2<f32>(fract((lon+PI)/(2.0*PI)), v);
    // f32 coordinate rounding must not choose the opposite side of a tile
    // seam. Repair these narrow strips with the shared precise CPU sampler.
    var seam_zoom = z + select(0u,1u,fade > MIN_BLEND);
    // The night lights use the level nearest the pixel's detail, up to their finest.
    let lights_zoom = min(z + select(0u,1u,fraction >= 0.5), view.lights.y);
    if view.lights.x != 0u { seam_zoom = max(seam_zoom, lights_zoom); }
    // Past `GLOBAL_MAX_LEVEL` the place comes from the near map, or the CPU draws it.
    let d = dir * t;
    var spot = spot_from_uv(uv);
    if abs(lat) < POLAR_CAP_START && dot(d, d) < view.near_frac.z {
        spot = spot_near(d);
    } else if seam_zoom > GLOBAL_MAX_LEVEL { output[i] = UNDRAWN; return; }
    let seam_local = spot_local(spot, seam_zoom, spot_tile(spot, seam_zoom));
    if any(min(seam_local, vec2<f32>(1.0) - seam_local) < vec2<f32>(TILE_SEAM_PRECISION_MARGIN)) { output[i] = UNDRAWN; return; }
    let offset = vec2<f32>(f32(x)-f32(width)/2.0,f32(y)-f32(height)/2.0);
    let distance = dot(offset,offset);
    var color = chain(0u,z,spot,distance,z,fraction).rgb;
    if fade > MIN_BLEND {
        // Sample the finer level first so the miss is recorded as a need, then
        // only blend when that level is actually resident; a coarser fallback
        // blended on top would flash while tiles load.
        let fine = chain(0u,z+1u,spot,distance,z,fraction).rgb;
        let c1 = vec2<u32>(max(spot_coordinate(spot, z+1u),vec2<i32>(0)));
        if find_tile(z+1u,c1.x,c1.y) >= 0 { color = blend(color,fine,fade); }
    }
    let sunlit = view.effects.x != 0u;
    var shade = 1.0; var night = 0.0; var darkness = 0.0;
    // With the sun, its light and the sky's apart (the CPU's `pixel::Daylight`).
    var sky = vec3<f32>(0.0); var direct = 0.0;
    if sunlit {
        let e = dot(normal,view.sun.xyz);
        let k = smooth01((e+TWILIGHT_OFFSET)/TWILIGHT_WIDTH);
        night = NIGHT_BLUE*(1.0-k); darkness = 1.0-k;
        let flat = day(e, 0.0);
        sky = flat.sky;
        direct = flat.facing*max(e,0.0);
    }
    // Night lights without the sun: night everywhere (`RenderCfg::night_everywhere`).
    let all_night = !sunlit && view.lights.x != 0u;
    if all_night {
        shade = NIGHT_BRIGHTNESS; night = NIGHT_BLUE; darkness = 1.0;
        sky = TWILIGHT_SKY/dot(TWILIGHT_SKY,LUMINANCE)*NIGHT_BRIGHTNESS;
    }
    // As the CPU's `night_lights` and `finish_surface`: the lights glow where it is dark.
    var lights = vec4<f32>(0.0);
    if view.lights.x != 0u && darkness >= MIN_LIGHTS_DARKNESS { lights = chain(1u,lights_zoom,spot,distance,z,fraction); }
    // The air in front of the ground (`atmosphere::Tables::ground_from_space`,
    // `finish_surface`): low sunlight is orange, the air dims and adds its glow.
    let airy = view.effects.y != 0u;
    var sunlight = vec3<f32>(1.0);
    var sun_tint = vec3<f32>(1.0);
    var glow = vec3<f32>(0.0);
    var crossed = vec3<f32>(1.0);
    if airy && view.air_frame.w > 0.0 {
        // The camera inside the air.
        let air = inside_ground(dir, t * (EARTH_RADIUS_M / 1000.0), sunlit);
        glow = air.scattered;
        crossed = air.crossed;
        if sunlit {
            sunlight = vec3<f32>(1.0) + (air.sunlight - vec3<f32>(1.0))*(1.0-darkness);
            sun_tint = air.sunlight;
        }
    } else if airy {
        let up = air_up(incidence);
        crossed = air_crossed(up).xyz;
        var mu_s = select(AIR_UNLIT_SUN,AIR_NIGHT_SUN,all_night);
        var nu = -incidence*mu_s;
        if sunlit {
            let ground_sun = dot(normal,view.sun.xyz);
            nu = dot(dir,view.sun.xyz);
            mu_s = clamp((AIR_GROUND*ground_sun - up*nu)/AIR_TOP,-1.0,1.0);
            let lit = air_crossed(air_up(max(ground_sun,0.0)));
            let red = max(lit.x,AIR_TINY);
            let tint = vec3<f32>(1.0,sqrt(min(lit.y/red*AIR_NOON_GREEN,1.0)),sqrt(min(lit.z/red*AIR_NOON_BLUE,1.0)));
            sunlight = vec3<f32>(1.0) + (tint - vec3<f32>(1.0))*(1.0-darkness);
            sun_tint = tint;
        }
        glow = air_scattered(up,mu_s,nu);
    }
    // The sun's glint off the water (`sun::glint`), lit as the ground is.
    var glint = 0.0;
    let up = clamp(dot(normal,view.sun.xyz) / GLINT_SUNSET_COSINE, 0.0, 1.0);
    if sunlit && view.sun.w > 0.0 && up > 0.0 {
        let half = normalize(view.sun.xyz - dir);
        let highlight = pow(max(dot(normal,half),0.0), GLINT_SHININESS);
        if highlight >= GLINT_MIN_HIGHLIGHT {
            glint = water_at(degrees(lat), degrees(lon)) * GLINT_STRENGTH * highlight * up * up * (3.0 - 2.0 * up);
        }
    }
    var light = vec3<f32>(shade);
    if sunlit || all_night { light = sky + sun_tint*direct; }
    // The relief's slope in the light (`finish_surface` with `Relief`): the sun on the
    // slope, the share of it seen and the share of the sky faced; the sun rises
    // earlier over high ground (`terrain::horizon_dip`). Without the sun, hill shading.
    if relief_on {
        let fade = view.relief.y;
        let faded = slope * fade;
        let slope_length = sqrt(1.0 + dot(faded, faded));
        let sky_share = 0.5 + 0.5 / slope_length;
        if sunlit {
            let sun = view.sun.xyz;
            var east = vec3<f32>(-normal.y, normal.x, 0.0);
            if dot(east, east) > 0.0 { east = normalize(east); } else { east = vec3<f32>(1.0, 0.0, 0.0); }
            let north = cross(normal, east);
            let cosine = (dot(sun, normal) - faded.x*dot(sun, east) - faded.y*dot(sun, north)) / slope_length;
            let g = sqrt(dot(point, point * q));
            let height = length(point) * (1.0 - 1.0 / g) / max(view.relief.z, 1e-30);
            let raised = day(dot(normal, sun), sqrt(2.0 * max(height, 0.0)) * fade);
            light = raised.sky*sky_share + sun_tint*(raised.facing*max(cosine, 0.0)*seen);
        } else if all_night {
            light = sky*sky_share;
        } else {
            light = vec3<f32>(shade * chart_light(slope, fade));
        }
    }
    color = color*light + vec3<f32>(0.0,0.0,night) + sunlight*glint;
    if lights.w > 0.0 { color += lights.rgb*darkness; }
    if airy {
        let grazing = (1.0-incidence)*(1.0-incidence);
        let share = AIR_NADIR_SHARE + (1.0-AIR_NADIR_SHARE)*grazing*grazing;
        let through = vec3<f32>(1.0) + (crossed - vec3<f32>(1.0))*share;
        let light = glow*(AIR_EXPOSURE*share);
        color = sqrt(color*color*through + light/(vec3<f32>(1.0)+light));
    }
    // The aurora glows in the air in front (`finish_surface`).
    if view.figure.w > 0.0 {
        let glow = aurora_glow(camera, dir, t);
        color = color + glow * (vec3<f32>(1.0) - color);
    }
    put(i, vec4<f32>(color, 1.0));
}
