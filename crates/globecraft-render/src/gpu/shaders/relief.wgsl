// The relief march's open air on the GPU (roadmap D1, first step; `gpu::relief`).
// Each pixel's ray walks the blocks of the elevation tiles' maxima (`tops`) while it
// stays above them and writes how far it got: the CPU's exact march (`terrain::march`)
// starts there. It reads every level the CPU may read at that distance (the next
// coarser one where heights blend, both near a level's edge), the maxima are widened
// by a block for the f32 coordinates, and heights get a margin: the ray it hands over
// has never passed through ground the CPU would draw. A tile it does not hold ends
// its walk.
struct Relief {
    camera: vec4<f32>, // xyz; w: the view's aspect ratio
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    q: vec4<f32>, // 1 / the surface's semi-axes squared
    axes: vec4<f32>, // the semi-axes (Earth radii); w: the highest summit (world)
    march: vec4<f32>, // world units per metre, view angle per pixel, height margin (world), heights blend (1) or not (0)
    raster: vec4<u32>, // width, height, tile table mask, most steps
}
@group(0) @binding(0) var<uniform> relief: Relief;
@group(0) @binding(1) var<storage, read_write> starts: array<f32>;
@group(0) @binding(2) var tops: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> tiles: array<vec4<u32>>;
// The tiles' texel maxima (the CPU's level 0) in quarter metres, rounded up.
@group(0) @binding(4) var heights: texture_2d_array<u32>;

const PI: f32 = 3.141592653589793;
const FOV_HALF_TAN: f32 = __FOV__;
const EARTH_RADIUS_M: f32 = __RADIUS__;
const MERCATOR_MAX_SINE: f32 = __MERCATOR_SINE__;
const DEM_TEXELS: f32 = __DEM_TEXELS__;
const PIXELS_PER_TEXEL: f32 = __PIXELS_PER_TEXEL__;
const MAX_ZOOM: i32 = __MAX_ZOOM__;
const MIN_LATITUDE_COSINE: f32 = __MIN_LATITUDE_COSINE__;
const BLEND_LEVELS: f32 = __BLEND_LEVELS__;
// Within this much of a level's edge the f32 zoom may fall on the other side of the
// CPU's: both levels count.
const LEVEL_EDGE: f32 = 0.02;
// The coarsest block walked: half a tile (the next finer level's whole tile).
const TOP_MIP: i32 = 7;
// Past a block's edge by this share of a texel, as the CPU (`BLOCK_EXIT_TEXELS`).
const EXIT_NUDGE_TEXELS: f32 = 0.001;
// Texels the CPU walks again before the ground the GPU found.
const BACK_TEXELS: f32 = 1.0;
// A tile not held.
const NOT_HELD: f32 = -1.0;

fn tile_hash(z: u32, x: u32, y: u32) -> u32 {
    return (z * 73856093u) ^ (x * 19349663u) ^ (y * 83492791u);
}
fn layer_of(level: u32, x: u32, y: u32) -> i32 {
    let mask = relief.raster.z;
    var index = tile_hash(level, x, y) & mask;
    for (var probe = 0u; probe <= mask; probe++) {
        let entry = tiles[index];
        if entry.x == 0xffffffffu { return -1; }
        if entry.x == level && entry.y == x && entry.z == y { return i32(entry.w); }
        index = (index + 1u) & mask;
    }
    return -1;
}
// The highest height (metres) in the block of mip `mip` (blocks of 2^(mip+1) texels)
// of `level`'s tile under `uv`, widened by a block; NOT_HELD without the tile.
fn block_top(level: i32, mip: i32, uv: vec2<f32>) -> f32 {
    let n = f32(1u << u32(level));
    let tile = min(floor(uv * n), vec2<f32>(n - 1.0));
    let layer = layer_of(u32(level), u32(tile.x), u32(tile.y));
    if layer < 0 { return NOT_HELD; }
    let side = 256 >> u32(mip);
    let block = clamp(vec2<i32>((uv * n - tile) * f32(side)), vec2<i32>(0), vec2<i32>(side - 1));
    return textureLoad(tops, block, layer, mip).x;
}
// The highest bilinear height (metres) inside the texel of `level`'s tile under
// `uv` (the CPU's finest block maxima); NOT_HELD without the tile.
fn texel_top(level: i32, uv: vec2<f32>) -> f32 {
    let n = f32(1u << u32(level));
    let tile = min(floor(uv * n), vec2<f32>(n - 1.0));
    let layer = layer_of(u32(level), u32(tile.x), u32(tile.y));
    if layer < 0 { return NOT_HELD; }
    let texel = clamp(vec2<i32>((uv * n - tile) * DEM_TEXELS), vec2<i32>(0), vec2<i32>(i32(DEM_TEXELS) - 1));
    return f32(textureLoad(heights, texel, layer, 0).x) * 0.25;
}
// Where the ray enters and leaves the ellipsoid with semi-axes `axes` (x > y: missed).
fn span(camera: vec3<f32>, dir: vec3<f32>, axes: vec3<f32>) -> vec2<f32> {
    let q = 1.0 / (axes * axes);
    let a = dot(dir, q * dir);
    let b = dot(camera, q * dir);
    let c = dot(camera, q * camera) - 1.0;
    let disc = b * b - a * c;
    if disc < 0.0 || a <= 0.0 { return vec2<f32>(1.0, 0.0); }
    let root = sqrt(disc);
    return vec2<f32>((-b - root) / a, (-b + root) / a);
}
// Distance along the ray to leave a block `size` wide from `inside` it, at rate `rate`.
fn leave(inside: f32, rate: f32, size: f32) -> f32 {
    if rate > 0.0 { return (size - inside) / rate; }
    if rate < 0.0 { return -inside / rate; }
    return 3.4e38;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let width = relief.raster.x; let height = relief.raster.y;
    if i >= width * height { return; }
    let x = i % width; let y = i / width;
    let nx = (f32(x) + 0.5) / f32(width) * 2.0 - 1.0;
    let ny = 1.0 - (f32(y) + 0.5) / f32(height) * 2.0;
    let dir = normalize(relief.right.xyz * (nx * relief.camera.w * FOV_HALF_TAN) + relief.up.xyz * (ny * FOV_HALF_TAN) - relief.forward.xyz);
    let camera = relief.camera.xyz;
    let shell = span(camera, dir, relief.axes.xyz + vec3<f32>(relief.axes.w));
    if shell.x > shell.y || shell.y <= 0.0 { starts[i] = 0.0; return; }
    var end = shell.y;
    let floor_hit = span(camera, dir, relief.axes.xyz);
    if floor_hit.x <= floor_hit.y && floor_hit.x > 0.0 { end = min(end, floor_hit.x); }
    let to_world = relief.march.x;
    let margin = relief.march.z;
    var t = max(shell.x, 0.0);
    // The last point found above the ground's heights (the walk hands it over).
    var above = t;
    // A texel at the level last walked, along the ray.
    var texel_t = 0.0;
    for (var step = 0u; step < relief.raster.w; step++) {
        if t >= end { break; }
        let p = camera + dir * t;
        let g = sqrt(dot(p, p * relief.q.xyz));
        let h = length(p) * (1.0 - 1.0 / g);
        if h <= 0.0 { break; }
        let normal = normalize(p * relief.q.xyz);
        // Past Web Mercator's limit the CPU reads a coarse level (`terrain::polar_fade`).
        if abs(normal.z) > MERCATOR_MAX_SINE { break; }
        let across_sq = max(dot(normal.xy, normal.xy), 1e-24);
        let latitude_cosine = sqrt(across_sq);
        var u = atan2(normal.y, normal.x) / (2.0 * PI) + 0.5;
        if u >= 1.0 { u -= 1.0; }
        let s = clamp(normal.z, -MERCATOR_MAX_SINE, MERCATOR_MAX_SINE);
        let uv = vec2<f32>(max(u, 0.0), clamp(0.5 - log((1.0 + s) / (1.0 - s)) / (4.0 * PI), 0.0, 1.0));
        // The level by distance, as `terrain::dem_zoom_exact`.
        let metres_per_pixel = max(t * EARTH_RADIUS_M * relief.march.y, 0.01);
        let zoom = log2(2.0 * PI * EARTH_RADIUS_M * max(latitude_cosine, MIN_LATITUDE_COSINE) / (DEM_TEXELS * metres_per_pixel * PIXELS_PER_TEXEL));
        let level = clamp(i32(floor(zoom)), 0, MAX_ZOOM);
        let fraction = zoom - f32(level);
        let finer = level < MAX_ZOOM && fraction > 1.0 - LEVEL_EDGE;
        let coarser = level > 0 && (fraction < LEVEL_EDGE || (relief.march.w > 0.5 && fraction < BLEND_LEVELS + LEVEL_EDGE));
        // How Mercator coordinates move along the ray (as `terrain::Anchor`).
        let turn = (dir - normal * dot(dir, normal)) / length(p);
        let du = (normal.x * turn.y - normal.y * turn.x) / across_sq / (2.0 * PI);
        let sine = clamp(normal.z, -0.999999, 0.999999);
        let dv = -turn.z / ((1.0 - sine * sine) * 2.0 * PI);
        let climb = dot(dir, normal);
        let texels = DEM_TEXELS * exp2(f32(level));
        let speed = max(length(vec2<f32>(du, dv)) * texels, 1e-12);
        texel_t = 1.0 / speed;
        var moved = false;
        var held = true;
        for (var mip = TOP_MIP; mip >= 1; mip--) {
            var top = block_top(level, mip, uv);
            if coarser && top != NOT_HELD {
                let coarse = block_top(level - 1, mip - 1, uv);
                top = select(max(top, coarse), NOT_HELD, coarse == NOT_HELD);
            }
            if finer && top != NOT_HELD {
                let fine = block_top(level + 1, mip + 1, uv);
                top = select(max(top, fine), NOT_HELD, fine == NOT_HELD);
            }
            if top == NOT_HELD { held = false; break; }
            let top_world = top * to_world + margin;
            if h > top_world {
                let size = exp2(f32(mip + 1)) / texels;
                let inside = uv - floor(uv / size) * size;
                var exit = min(leave(inside.x, du, size), leave(inside.y, dv, size));
                if climb < 0.0 { exit = min(exit, (h - top_world) / -climb); }
                // f32 can put the point a hair past the block's edge.
                exit = max(exit, 0.0);
                t += exit + EXIT_NUDGE_TEXELS / speed;
                above = t;
                moved = true;
                break;
            }
        }
        if !held { break; }
        if !moved {
            // Within a block of the ground: texel by texel, each the highest its
            // bilinear heights reach, as far as the ray stays above them.
            var ground = texel_top(level, uv);
            if coarser && ground != NOT_HELD {
                let coarse = texel_top(level - 1, uv);
                ground = select(max(ground, coarse), NOT_HELD, coarse == NOT_HELD);
            }
            if finer && ground != NOT_HELD {
                let fine = texel_top(level + 1, uv);
                ground = select(max(ground, fine), NOT_HELD, fine == NOT_HELD);
            }
            let ground_world = ground * to_world + margin;
            if ground == NOT_HELD || h <= ground_world { break; }
            above = t;
            let size = 1.0 / texels;
            let inside = uv - floor(uv / size) * size;
            var exit = min(leave(inside.x, du, size), leave(inside.y, dv, size));
            if climb < 0.0 { exit = min(exit, (h - ground_world) / -climb); }
            t += max(exit, 0.0) + EXIT_NUDGE_TEXELS / speed;
        }
    }
    // Past the end nothing is met: the CPU's march ends at once. Otherwise the CPU
    // walks the last texels again: its own steps between exact measurements put the
    // ground up to a texel from where the GPU sees it.
    starts[i] = select(max(above - BACK_TEXELS * texel_t, 0.0), t, t >= end);
}
