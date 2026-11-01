// Overview geometry uses f32 only after the host checks the altitude/LOD range.
// Close views use the shared f64 planner. CPU repairs the exact sky/horizon.
struct View {
    camera: vec4<f32>,
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    sun: vec4<f32>,
    raster: vec4<u32>, // width, height, imagery, provider max zoom
    effects: vec4<u32>, // sunlight, atmosphere, level-of-detail raster height, reserved
    limits: vec4<u32>, // tracked max zoom, dependency count, table mask, unused
}
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var atlas: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> tiles: array<vec4<u32>>;
@group(0) @binding(4) var<storage, read_write> needs: array<atomic<u32>>;
const FOV_HALF_TAN: f32 = __FOV__;
const EARTH_RADIUS_M: f32 = __RADIUS__;
const MERCATOR_LIMIT_DEG: f32 = __MERCATOR__;
const TILE_SEAM_PRECISION_MARGIN: f32 = 0.0001;
const MERCATOR_EDGE_EPSILON: f32 = 0.00000011920928955078125;
const HORIZON_PRECISION_MARGIN: f32 = 0.01;
const PI: f32 = 3.141592653589793;
const MISSING: vec3<f32> = __MISSING__;
const RIM: vec3<f32> = __RIM__;
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
const RIM_BLEND: f32 = __RIM_BLEND__;
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
fn chain(zoom: u32, uv: vec2<f32>, distance: f32, base: u32, frac: f32) -> vec3<f32> {
    var z = i32(zoom);
    loop {
        let n = 1u << u32(z);
        // Preserve the CPU sampler's signed floor before saturating key coordinates.
        let coordinate = min(floor(uv * f32(n)), vec2<f32>(f32(n-1u)));
        let coord = vec2<u32>(max(coordinate,vec2<f32>(0.0)));
        // Track sampled keys as well as misses, so residency follows this view
        // instead of growing with the entire decoded cache.
        if u32(z) <= view.limits.x {
            let index = ((1u << (2u * u32(z))) - 1u) / 3u + coord.y * n + coord.x;
            let priority = bitcast<u32>(distance);
            if priority < atomicLoad(&needs[index]) { atomicMin(&needs[index],priority); }
        } else { atomicStore(&needs[view.limits.y],1u); }
        let layer = find_tile(u32(z),coord.x,coord.y);
        if layer >= 0 {
            let local = uv * f32(n) - coordinate;
            let stride = stride_at(z, i32(base), frac);
            if stride <= 1.0 { return sample_tile(local,layer); }
            return sample_area(local,layer,min(stride,MAX_SAMPLE_STRIDE));
        }
        z -= 1;
        if z < 0 { break; }
    }
    return MISSING;
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
    let camera = view.camera.xyz;
    let bb = dot(camera,dir); let cc = dot(camera,camera);
    let disc = bb * bb - (cc - 1.0);
    if disc < HORIZON_PRECISION_MARGIN || bb >= 0.0 { output[i] = 0u; return; }
    let t = -bb - sqrt(disc);
    let point = camera + dir * t;
    let incidence = max(-dot(point,dir),0.0);
    let lat = asin(clamp(point.z,-1.0,1.0));
    let lon = atan2(point.y,point.x);
    let mpp = t * EARTH_RADIUS_M * (2.0 * FOV_HALF_TAN / f32(view.effects.z)) / sqrt(max(incidence,LOD_INC_COS));
    let lod = log2(2.0 * PI * EARTH_RADIUS_M * max(cos(lat),LOD_LAT_COS) / (256.0 * mpp)) + LOD_BIAS;
    var zl = floor(lod); var fraction = lod - zl;
    let z = u32(clamp(zl,0.0,f32(view.raster.w)));
    if z >= view.raster.w { fraction = 0.0; }
    // As the CPU: imagery fades over the last 30% of the zoom band (its base is
    // oversampled), text maps over the last 10%, so labels stay sharp.
    var fade = clamp((fraction - IMAGERY_ZOOM_FADE_START) / (1.0 - IMAGERY_ZOOM_FADE_START), 0.0, 1.0);
    if view.raster.z == 0u { fade = clamp((fraction - TEXT_ZOOM_FADE_START) / (1.0 - TEXT_ZOOM_FADE_START), 0.0, 1.0); }
    let latc = clamp(lat,-MERCATOR_LIMIT_DEG * PI/180.0,MERCATOR_LIMIT_DEG * PI/180.0);
    let s = sin(latc);
    var v = 0.5 - log((1.0+s)/(1.0-s))/(4.0*PI);
    // The f64 Mercator clamp lands just beyond [0,1]. Reproduce its
    // tile-local edge choice instead of silently changing polar imagery.
    if lat >= MERCATOR_LIMIT_DEG*PI/180.0 { v = -MERCATOR_EDGE_EPSILON; }
    if lat <= -MERCATOR_LIMIT_DEG*PI/180.0 { v = 1.0+MERCATOR_EDGE_EPSILON; }
    let uv = vec2<f32>(fract((lon+PI)/(2.0*PI)), v);
    // f32 coordinate rounding must not choose the opposite side of a tile
    // seam. Repair these narrow strips with the shared precise CPU sampler.
    let seam_zoom = z + select(0u,1u,fade > MIN_BLEND);
    let scaled_uv = uv*f32(1u << seam_zoom);
    if any(abs(scaled_uv-round(scaled_uv)) < vec2<f32>(TILE_SEAM_PRECISION_MARGIN)) { output[i] = 0u; return; }
    let offset = vec2<f32>(f32(x)-f32(width)/2.0,f32(y)-f32(height)/2.0);
    let distance = dot(offset,offset);
    var color = chain(z,uv,distance,z,fraction);
    if fade > MIN_BLEND {
        // Sample the finer level first so the miss is recorded as a need, then
        // only blend when that level is actually resident; a coarser fallback
        // blended on top would flash while tiles load.
        let fine = chain(z+1u,uv,distance,z,fraction);
        let n1 = 1u << (z+1u);
        let c1 = vec2<u32>(max(min(floor(uv*f32(n1)),vec2<f32>(f32(n1-1u))),vec2<f32>(0.0)));
        if find_tile(z+1u,c1.x,c1.y) >= 0 { color = blend(color,fine,fade); }
    }
    if view.effects.x != 0u {
        var k = clamp((dot(point,view.sun.xyz)+TWILIGHT_OFFSET)/TWILIGHT_WIDTH,0.0,1.0);
        k = k*k*(3.0-2.0*k);
        color = color*(NIGHT_BRIGHTNESS+DAY_RANGE*k) + vec3<f32>(0.0,0.0,NIGHT_BLUE*(1.0-k));
    }
    if view.effects.y != 0u { let rim_base = 1.0-incidence; let rim_squared = rim_base*rim_base; let rim = rim_squared*rim_squared*RIM_BLEND; color = blend(color,RIM,rim); }
    let rgb = vec3<u32>(clamp(color,vec3<f32>(0.0),vec3<f32>(1.0))*255.0);
    output[i] = rgb.r | (rgb.g<<8u) | (rgb.b<<16u) | (255u<<24u);
}
