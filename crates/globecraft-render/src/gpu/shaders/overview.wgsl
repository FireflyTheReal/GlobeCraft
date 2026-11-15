// Overview geometry uses f32 only after the host checks the altitude/LOD range.
// Close views use the shared f64 planner. CPU repairs the exact sky/horizon.
struct View {
    camera: vec4<f32>,
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    sun: vec4<f32>,
    shape: vec4<f32>, // 1 / the surface's semi-axes squared; the camera in that metric, squared
    figure: vec4<f32>, // semi-major axis (m), eccentricity squared, unused
    raster: vec4<u32>, // width, height, imagery, provider max zoom
    effects: vec4<u32>, // sunlight, atmosphere, level-of-detail raster height, reserved
    limits: vec4<u32>, // tracked max zoom, dependency count, table mask, unused
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
const DAY_RANGE: f32 = __DAY_RANGE__;
const NIGHT_BLUE: f32 = __NIGHT_BLUE__;
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
    // The polar caps have their own imagery, projection and colour matching: the
    // precise CPU repair draws them (a small share of an overview).
    if view.raster.z != 0u && abs(lat) > POLAR_CAP_START { output[i] = 0u; return; }
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
    if sunlit {
        var k = clamp((dot(normal,view.sun.xyz)+TWILIGHT_OFFSET)/TWILIGHT_WIDTH,0.0,1.0);
        k = k*k*(3.0-2.0*k);
        shade = NIGHT_BRIGHTNESS+DAY_RANGE*k; night = NIGHT_BLUE*(1.0-k); darkness = 1.0-k;
    }
    // As the CPU's `night_lights` and `finish_surface`: without the sun the lights are
    // the picture; with it they glow where it is dark.
    var lights = vec4<f32>(0.0);
    if view.lights.x != 0u && (!sunlit || darkness >= MIN_LIGHTS_DARKNESS) { lights = chain(1u,lights_zoom,uv,distance,z,fraction); }
    if lights.w > 0.0 && !sunlit { color = lights.rgb; }
    // The air in front of the ground (`atmosphere::Tables::ground_from_space`,
    // `finish_surface`): low sunlight is orange, the air dims and adds its glow.
    let airy = view.effects.y != 0u;
    var sunlight = vec3<f32>(1.0);
    var glow = vec3<f32>(0.0);
    var crossed = vec3<f32>(1.0);
    if airy {
        let up = air_up(incidence);
        crossed = air_crossed(up).xyz;
        var mu_s = AIR_UNLIT_SUN;
        var nu = -incidence*AIR_UNLIT_SUN;
        if sunlit {
            let ground_sun = dot(normal,view.sun.xyz);
            nu = dot(dir,view.sun.xyz);
            mu_s = clamp((AIR_GROUND*ground_sun - up*nu)/AIR_TOP,-1.0,1.0);
            let lit = air_crossed(air_up(max(ground_sun,0.0)));
            let red = max(lit.x,AIR_TINY);
            let tint = vec3<f32>(1.0,sqrt(min(lit.y/red*AIR_NOON_GREEN,1.0)),sqrt(min(lit.z/red*AIR_NOON_BLUE,1.0)));
            sunlight = vec3<f32>(1.0) + (tint - vec3<f32>(1.0))*(1.0-darkness);
        }
        glow = air_scattered(up,mu_s,nu);
    }
    color = color*sunlight*shade + vec3<f32>(0.0,0.0,night);
    if lights.w > 0.0 && sunlit { color += lights.rgb*darkness; }
    if airy {
        let grazing = (1.0-incidence)*(1.0-incidence);
        let share = AIR_NADIR_SHARE + (1.0-AIR_NADIR_SHARE)*grazing*grazing;
        let through = vec3<f32>(1.0) + (crossed - vec3<f32>(1.0))*share;
        let light = glow*(AIR_EXPOSURE*share);
        color = sqrt(color*color*through + light/(vec3<f32>(1.0)+light));
    }
    let rgb = vec3<u32>(clamp(color,vec3<f32>(0.0),vec3<f32>(1.0))*255.0);
    output[i] = rgb.r | (rgb.g<<8u) | (rgb.b<<16u) | (255u<<24u);
}
