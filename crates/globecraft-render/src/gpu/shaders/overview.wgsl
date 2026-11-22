// Overview geometry uses f32 only after the host checks the altitude/LOD range.
// Close views use the shared f64 planner. CPU repairs the exact sky/horizon.
struct View {
    camera: vec4<f32>,
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    sun: vec4<f32>, // toward the sun; w: the glint on (1) or off (0)
    shape: vec4<f32>, // 1 / the surface's semi-axes squared; the camera in that metric, squared
    figure: vec4<f32>, // semi-major axis (m), eccentricity squared, the eye's twilight adaptation (`atmosphere::eye_weight`), aurora opacity (0: off)
    raster: vec4<u32>, // width, height, imagery, provider max zoom
    effects: vec4<u32>, // sunlight, atmosphere, level-of-detail raster height, reserved
    limits: vec4<u32>, // tracked max zoom, dependency count, table mask, water map width
    lights: vec4<u32>, // night lights shown, their max zoom, where their needs start, tracked max zoom
}
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var atlas: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> tiles: array<vec4<u32>>;
@group(0) @binding(4) var<storage, read_write> needs: array<atomic<u32>>;
// The air (the CPU's `atmosphere`): the light that crosses it from the ground by view
// angle, and the sunlight scattered along rays entering from space.
@group(0) @binding(5) var<storage, read> air_transmittance: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> air_scattering: array<vec4<f32>>;
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
const MIN_LIGHTS_DARKNESS: f32 = __MIN_LIGHTS_DARKNESS__;
// Marks the night lights' tiles in the tile table's level word.
const LIGHTS_TAG: u32 = __LIGHTS_TAG__u;
fn blend(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> { return a * (1.0 - t) + b * t; }
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
// The colour of `source` (0: the base map, 1: the night lights) from level `zoom` or
// the nearest coarser level loaded; `w` is 1 when a level was found.
fn chain(source: u32, zoom: u32, uv: vec2<f32>, distance: f32, base: u32, frac: f32) -> vec4<f32> {
    let tag = select(0u, LIGHTS_TAG, source == 1u);
    let first = select(0u, view.lights.z, source == 1u);
    let tracked = select(view.limits.x, view.lights.w, source == 1u);
    var z = i32(zoom);
    loop {
        let n = 1u << u32(z);
        // Preserve the CPU sampler's signed floor before saturating key coordinates.
        let coordinate = min(floor(uv * f32(n)), vec2<f32>(f32(n-1u)));
        let coord = vec2<u32>(max(coordinate,vec2<f32>(0.0)));
        // Track sampled keys as well as misses, so residency follows this view
        // instead of growing with the entire decoded cache.
        if u32(z) <= tracked {
            let index = first + ((1u << (2u * u32(z))) - 1u) / 3u + coord.y * n + coord.x;
            let priority = bitcast<u32>(distance);
            if priority < atomicLoad(&needs[index]) { atomicMin(&needs[index],priority); }
        } else { atomicStore(&needs[view.limits.y],1u); }
        let layer = find_tile(u32(z) | tag,coord.x,coord.y);
        if layer >= 0 {
            let local = uv * f32(n) - coordinate;
            let stride = stride_at(z, i32(base), frac);
            if stride <= 1.0 { return vec4<f32>(sample_tile(local,layer),1.0); }
            return vec4<f32>(sample_area(local,layer,min(stride,MAX_SAMPLE_STRIDE)),1.0);
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
    return air_lerp(air_transmittance[i],air_transmittance[i+1u],c.y);
}
fn air_entry(view: u32, sun: u32, phase: vec2<f32>) -> vec4<f32> {
    let first = (view*AIR_SUNS+sun)*AIR_PHASES+u32(phase.x);
    return air_lerp(air_scattering[first],air_scattering[first+1u],phase.y);
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
    let spread = HAZE_SPREAD - HAZE_FORWARD*nu;
    let both = 1.0 + nu*nu;
    var haze = vec3<f32>(0.0);
    if entry.x > 0.0 { let ratio = entry.w/entry.x; haze = vec3<f32>(entry.w,entry.y*ratio*HAZE_GREEN,entry.z*ratio*HAZE_BLUE); }
    return entry.xyz*(AIR_PHASE*both) + haze*(HAZE_PHASE*both/(spread*sqrt(spread)));
}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
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
    let disc = bb * bb - a * (view.shape.w - 1.0);
    if disc < HORIZON_PRECISION_MARGIN || bb >= 0.0 { output[i] = 0u; return; }
    let t = (-bb - sqrt(disc)) / a;
    let point = camera + dir * t;
    let normal = normalize(point * q);
    let incidence = max(-dot(normal,dir),0.0);
    let latitude_cosine = length(normal.xy);
    let lat = atan2(normal.z,latitude_cosine);
    let lon = atan2(normal.y,normal.x);
    // The polar caps have their own imagery, projection and colour matching, and the
    // night lights their own fill past their tiles: the precise CPU repair draws them
    // (a small share of an overview).
    if (view.raster.z != 0u || view.lights.x != 0u) && abs(lat) > POLAR_CAP_START { output[i] = 0u; return; }
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
    let scaled_uv = uv*f32(1u << seam_zoom);
    if any(abs(scaled_uv-round(scaled_uv)) < vec2<f32>(TILE_SEAM_PRECISION_MARGIN)) { output[i] = 0u; return; }
    let offset = vec2<f32>(f32(x)-f32(width)/2.0,f32(y)-f32(height)/2.0);
    let distance = dot(offset,offset);
    var color = chain(0u,z,uv,distance,z,fraction).rgb;
    if fade > MIN_BLEND {
        // Sample the finer level first so the miss is recorded as a need, then
        // only blend when that level is actually resident; a coarser fallback
        // blended on top would flash while tiles load.
        let fine = chain(0u,z+1u,uv,distance,z,fraction).rgb;
        let n1 = 1u << (z+1u);
        let c1 = vec2<u32>(max(min(floor(uv*f32(n1)),vec2<f32>(f32(n1-1u))),vec2<f32>(0.0)));
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
        // The eye adapted to twilight (`view.figure.z`) keeps the blue hour longer.
        let night_end = TWILIGHT_OFFSET+(EYE_TWILIGHT-TWILIGHT_OFFSET)*view.figure.z;
        var shaded = SHADE_RISE+(SHADE_DAY-SHADE_RISE)*smooth01((e-SUN_RISE_START)/(SKY_DAY-SUN_RISE_START));
        if e <= SUN_RISE_START {
            shaded = NIGHT_BRIGHTNESS+(SHADE_RISE-NIGHT_BRIGHTNESS)*smooth01((e+night_end)/(SUN_RISE_START+night_end));
        }
        let risen = smooth01((e-SUN_RISE_START)/(SUN_RISE_FULL-SUN_RISE_START));
        let facing = risen*(DIRECT_RISE+(1.0-SHADE_DAY-DIRECT_RISE)*smooth01(e/SKY_DAY));
        let colour = mix(TWILIGHT_SKY,DAY_SKY,smooth01(e/SKY_DAY));
        sky = colour/dot(colour,LUMINANCE)*shaded;
        direct = facing*max(e,0.0);
    }
    // Night lights without the sun: night everywhere (`RenderCfg::night_everywhere`).
    let all_night = !sunlit && view.lights.x != 0u;
    if all_night {
        shade = NIGHT_BRIGHTNESS; night = NIGHT_BLUE; darkness = 1.0;
        sky = TWILIGHT_SKY/dot(TWILIGHT_SKY,LUMINANCE)*NIGHT_BRIGHTNESS;
    }
    // As the CPU's `night_lights` and `finish_surface`: the lights glow where it is dark.
    var lights = vec4<f32>(0.0);
    if view.lights.x != 0u && darkness >= MIN_LIGHTS_DARKNESS { lights = chain(1u,lights_zoom,uv,distance,z,fraction); }
    // The air in front of the ground (`atmosphere::Tables::ground_from_space`,
    // `finish_surface`): low sunlight is orange, the air dims and adds its glow.
    let airy = view.effects.y != 0u;
    var sunlight = vec3<f32>(1.0);
    var sun_tint = vec3<f32>(1.0);
    var glow = vec3<f32>(0.0);
    var crossed = vec3<f32>(1.0);
    if airy {
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
    let rgb = vec3<u32>(clamp(color,vec3<f32>(0.0),vec3<f32>(1.0))*255.0);
    output[i] = rgb.r | (rgb.g<<8u) | (rgb.b<<16u) | (255u<<24u);
}
