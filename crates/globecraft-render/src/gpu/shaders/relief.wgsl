// The relief march on the GPU (roadmap D1, steps 1 and 2; `gpu::relief`). Each
// pixel's ray walks the blocks of the elevation tiles' maxima (`tops`) while it stays
// above them: the CPU's exact march (`terrain::march`) starts where that ended. It
// reads every level the CPU may read at that distance (the next coarser one where
// heights blend, both near a level's edge), the maxima are widened by two texels for
// the f32 coordinates and the steps' straight lines over the map, and heights get a margin: the ray it hands over has never passed
// through ground the CPU would draw. The walk goes on to the ground's heights and
// from there toward the sun as `terrain::sun_light` does: the share of the sun seen
// there, which the CPU takes where it meets the ground at the same place. A tile it
// does not hold ends a walk without an answer.
struct Relief {
    camera: vec4<f32>, // xyz; w: the view's aspect ratio
    right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    q: vec4<f32>, // 1 / the surface's semi-axes squared
    axes: vec4<f32>, // the semi-axes (Earth radii); w: the highest summit (world)
    march: vec4<f32>, // world units per metre, view angle per pixel, height margin (world), heights blend (1) or not (0)
    raster: vec4<u32>, // width, height, tile table mask, most steps
    sun: vec4<f32>, // toward the sun (unit); w: 1 with a sun
    shade: vec4<f32>, // the relief's fade; x: unused
    // Near the camera, where an f32 map coordinate is too coarse for the finest levels:
    // the camera's texel at zoom 15 (whole part; fraction in `near_frac.xy`, range of
    // the near map squared in `near_frac.z`) and the zoom-15 texel coordinates' first
    // (`near_grad_*`, per world unit) and second derivatives (`near_hess_*`: xx, yy,
    // zz, xy, then xz, yz) at the camera, for offsets from it.
    near: vec4<u32>,
    near_frac: vec4<f32>,
    near_grad_x: vec4<f32>, near_grad_y: vec4<f32>,
    near_hess_x: vec4<f32>, near_hess_x2: vec4<f32>,
    near_hess_y: vec4<f32>, near_hess_y2: vec4<f32>,
}
@group(0) @binding(0) var<uniform> relief: Relief;
// Per pixel: where the CPU's march starts, then (after all of those) where the ray
// met the ground (-1: nowhere it knows), then the share of the sun seen there (-1:
// not known), then whether the CPU may take that ground as its own (1) or marches (0;
// -1: the walk gave up without an answer), then the ground's slope east and north
// there as the CPU shades it (`terrain::hit_on`; NO_SLOPE where the walk does not
// hold what that needs), for the overview to draw the ground (roadmap D8, G4).
@group(0) @binding(1) var<storage, read_write> starts: array<f32>;
@group(0) @binding(2) var tops: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> tiles: array<vec4<u32>>;
// Per tile held: whether a walk read it (1), so it stays held next frame.
@group(0) @binding(5) var<storage, read_write> read_tiles: array<u32>;
// The tiles' heights in quarter metres, and the most of each texel's 3×3 of them.
@group(0) @binding(4) var heights: texture_2d_array<u32>;
@group(0) @binding(6) var peaks: texture_2d_array<u32>;

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
// Past a block's edge by this share of a texel, as the CPU (`BLOCK_EXIT_TEXELS`), or
// by two steps of an f32 map coordinate where those are longer: a smaller nudge left
// the point where it was, and the walk crept a thousandth of a texel a step.
const EXIT_NUDGE_TEXELS: f32 = 0.001;
const UV_STEP: f32 = 1.2e-7;
// A nudge drops the ray at most this far (metres), well within the heights' margin:
// along a ray straight down, half a texel across the map is kilometres of ray.
const NUDGE_DROP_M: f32 = 1.0;
// From this level on an f32 map coordinate is a quarter texel or coarser: points
// there take their texel from the camera's (`near`), and outside the near map's range
// the walk ends without an answer.
const NEAR_LEVEL: i32 = 13;
const COARSE_COORDINATE_LEVEL: i32 = 14;
const NEAR_ZOOM: i32 = 15;
// The walk toward the sun steps at most this long under the maxima.
const LOOSE_STEP_TEXELS: f32 = 0.25;
// The exact piece's root is bracketed this closely (share of the way along the piece)
// for the refinement, which measures the ground there itself.
const ROOT_BRACKET: f32 = 0.01;
// Texels the CPU walks again before the ground the GPU found.
const BACK_TEXELS: f32 = 1.0;
// As `terrain::sun_light` and `SunMargin`.
const SUN_RADIUS_RAD: f32 = __SUN_RADIUS__;
const SHADOW_SOFT_TEXEL_SHARE: f32 = __SHADOW_SOFT__;
const SHADOW_LIFT_M: f32 = __SHADOW_LIFT_M__;
const SHADOW_LIFT_TEXELS: f32 = __SHADOW_LIFT_TEXELS__;
const SHADOW_DETAIL_COARSER: f32 = __SHADOW_COARSER__;
const SUN_RISE_START_COSINE: f32 = __SUN_RISE_START__;
const HIT_REFINEMENTS: i32 = 3;
const MAX_SLOPE: f32 = __MAX_SLOPE__;
const NO_SLOPE: f32 = 3.4e38;
// In the slope's plane: the ray surely meets no ground (it misses the relief's shell,
// or stays over the maxima to where it leaves it, never reaching the ellipsoid).
const SKY: f32 = -3.4e38;
// A tile not held.
const NOT_HELD: f32 = -1.0;

// As `overview::tile_hash`.
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
// at `at`, widened by a block; NOT_HELD without the tile.
fn block_top(at: Place, mip: i32) -> f32 {
    if at.layer < 0 { return NOT_HELD; }
    let side = 256 >> u32(mip);
    let block = clamp(vec2<i32>(at.at / f32(2 << u32(mip))), vec2<i32>(0), vec2<i32>(side - 1));
    return textureLoad(tops, block, at.layer, mip).x;
}
// A height (metres) of `level`'s tile at texel (x, y), clamped to the tile; the
// tile's layer `layer`.
fn height_of(layer: i32, texel: vec2<i32>) -> f32 {
    let last = i32(DEM_TEXELS) - 1;
    return f32(textureLoad(heights, clamp(texel, vec2<i32>(0), vec2<i32>(last)), layer, 0).x) * 0.25;
}
// Where `uv` falls in `level`'s tile: the tile's layer (or -1) and the position in
// texels.
struct Place { layer: i32, at: vec2<f32> }
// The tile last looked up for one of the walk's levels: the same tile is not looked up
// again (the hash table every step cost more than the walk's own sums).
struct Looked { key: vec3<u32>, layer: i32 }
fn place_kept(level: i32, uv: vec2<f32>, looked: ptr<function, Looked>) -> Place {
    let n = f32(1u << u32(level));
    let tile = min(floor(uv * n), vec2<f32>(n - 1.0));
    return place_held(level, vec2<u32>(tile), (uv * n - tile) * DEM_TEXELS, looked);
}
// Whether a point `d` from the camera takes its texel from the camera's at `level`.
fn is_near(level: i32, d: vec3<f32>) -> bool {
    return level >= NEAR_LEVEL && dot(d, d) < relief.near_frac.z;
}
// `level`'s tile and texel position for the point `d` from the camera (`is_near`):
// whole texels in integers, so the fraction keeps f32's precision at every level.
fn place_near(level: i32, d: vec3<f32>, looked: ptr<function, Looked>) -> Place {
    let hx = relief.near_hess_x; let hx2 = relief.near_hess_x2;
    let hy = relief.near_hess_y; let hy2 = relief.near_hess_y2;
    let dd = d * d;
    let cross = vec3<f32>(d.x * d.y, d.x * d.z, d.y * d.z);
    let off = relief.near_frac.xy + vec2<f32>(
        dot(relief.near_grad_x.xyz, d) + 0.5 * dot(hx.xyz, dd) + dot(vec3<f32>(hx.w, hx2.x, hx2.y), cross),
        dot(relief.near_grad_y.xyz, d) + 0.5 * dot(hy.xyz, dd) + dot(vec3<f32>(hy.w, hy2.x, hy2.y), cross),
    );
    let whole = floor(off);
    let side = i32(DEM_TEXELS) << u32(NEAR_ZOOM);
    let x = ((i32(relief.near.x) + i32(whole.x)) % side + side) % side;
    let y = clamp(i32(relief.near.y) + i32(whole.y), 0, side - 1);
    let shift = u32(NEAR_ZOOM - level);
    let g = vec2<u32>(u32(x), u32(y));
    let fraction = (vec2<f32>(g & vec2<u32>((1u << shift) - 1u)) + (off - whole)) / f32(1u << shift);
    let texel = g >> vec2<u32>(shift);
    let last = u32(DEM_TEXELS) - 1u;
    return place_held(level, texel / u32(DEM_TEXELS), vec2<f32>(texel & vec2<u32>(last)) + fraction, looked);
}
// The place at `level` of the point `d` from the camera, on the map at `uv`.
fn locate(level: i32, d: vec3<f32>, uv: vec2<f32>, looked: ptr<function, Looked>) -> Place {
    if is_near(level, d) { return place_near(level, d, looked); }
    return place_kept(level, uv, looked);
}
fn place_held(level: i32, tile: vec2<u32>, at: vec2<f32>, looked: ptr<function, Looked>) -> Place {
    let key = vec3<u32>(u32(level), tile.x, tile.y);
    if any(key != (*looked).key) {
        let layer = layer_of(key.x, key.y, key.z);
        // Stored only when not yet: every pixel storing to the same few words made
        // the walk wait on them.
        if layer >= 0 && read_tiles[layer] == 0u { read_tiles[layer] = 1u; }
        *looked = Looked(key, layer);
    }
    return Place((*looked).layer, at);
}
// The highest bilinear height (metres) inside the texel at `at` (the most of its 3×3
// heights, `peaks`, as the CPU's finest block maxima); NOT_HELD without the tile.
fn texel_top(at: Place) -> f32 {
    if at.layer < 0 { return NOT_HELD; }
    let last = i32(DEM_TEXELS) - 1;
    let texel = clamp(vec2<i32>(at.at), vec2<i32>(0), vec2<i32>(last));
    return f32(textureLoad(peaks, texel, at.layer, 0).x) * 0.25 + 0.125;
}
// The bilinear height (metres) at `at`, as `Elevation::height_m`; NOT_HELD.
fn bilinear(at: Place) -> f32 {
    if at.layer < 0 { return NOT_HELD; }
    let p = at.at - vec2<f32>(0.5);
    let corner = vec2<i32>(floor(p));
    let f = clamp(p - floor(p), vec2<f32>(0.0), vec2<f32>(1.0));
    let a = height_of(at.layer, corner);
    let b = height_of(at.layer, corner + vec2<i32>(1, 0));
    let c = height_of(at.layer, corner + vec2<i32>(0, 1));
    let d = height_of(at.layer, corner + vec2<i32>(1, 1));
    let top = a + (b - a) * f.x;
    return top + (c + (d - c) * f.x - top) * f.y;
}
// The bilinear height (metres) at `at` as it goes on along the map at `rate` (texels
// per unit along the ray), within the same four samples: h0 + h1·s + h2·s² after s.
fn bilinear_along(at: Place, rate: vec2<f32>) -> vec3<f32> {
    let p = at.at - vec2<f32>(0.5);
    let corner = vec2<i32>(floor(p));
    let f = clamp(p - floor(p), vec2<f32>(0.0), vec2<f32>(1.0));
    let a = height_of(at.layer, corner);
    let b = height_of(at.layer, corner + vec2<i32>(1, 0));
    let c = height_of(at.layer, corner + vec2<i32>(0, 1));
    let d = height_of(at.layer, corner + vec2<i32>(1, 1));
    let top = a + (b - a) * f.x;
    let k = a - b - c + d;
    return vec3<f32>(
        top + (c + (d - c) * f.x - top) * f.y,
        (b - a + k * f.y) * rate.x + (c - a + k * f.x) * rate.y,
        k * rate.x * rate.y,
    );
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
fn height_above(p: vec3<f32>) -> f32 {
    let g = sqrt(dot(p, p * relief.q.xyz));
    return length(p) * (1.0 - 1.0 / g);
}
// Where a point lies on the map and how the ray moves over it (`terrain::Anchor`).
struct Spot {
    uv: vec2<f32>,
    rate: vec2<f32>,
    latitude_cosine: f32,
    climb: f32,
    past_limit: bool,
}
fn spot(p: vec3<f32>, dir: vec3<f32>) -> Spot {
    let normal = normalize(p * relief.q.xyz);
    let across_sq = max(dot(normal.xy, normal.xy), 1e-24);
    var u = atan2(normal.y, normal.x) / (2.0 * PI) + 0.5;
    if u >= 1.0 { u -= 1.0; }
    let s = clamp(normal.z, -MERCATOR_MAX_SINE, MERCATOR_MAX_SINE);
    let uv = vec2<f32>(max(u, 0.0), clamp(0.5 - log((1.0 + s) / (1.0 - s)) / (4.0 * PI), 0.0, 1.0));
    let turn = (dir - normal * dot(dir, normal)) / length(p);
    let sine = clamp(normal.z, -0.999999, 0.999999);
    let rate = vec2<f32>(
        (normal.x * turn.y - normal.y * turn.x) / across_sq / (2.0 * PI),
        -turn.z / ((1.0 - sine * sine) * 2.0 * PI),
    );
    return Spot(uv, rate, sqrt(across_sq), dot(dir, normal), abs(normal.z) > MERCATOR_MAX_SINE);
}

// How a walk ended.
struct Walk {
    // The last point found above the maxima (with the margin), and a texel there.
    above: f32,
    texel_t: f32,
    // Where it met the heights (-1: it did not).
    hit: f32,
    // Whether it ended without an answer (a tile not held, past the map's limit).
    unknown: bool,
    // Toward the sun: the least clearance over the margin (`SunMargin`).
    least: f32,
}

// Where a walk met the ground: the last point above it and the first under it (their
// clearances), at `level` (blended from the next coarser one by `weight`; -1: not met).
struct Met { t0: f32, c0: f32, t1: f32, c1: f32, level: i32, blends: bool, weight: f32 }

// Walks the ray from `rel` (from the camera) along `dir` from `from` to `end`, at levels by distance
// for `angle` (view angle per pixel). `safe`: maxima with the margin and the levels
// on both sides of an edge, for a start the CPU can trust. `lift`: toward the sun, the
// height it started above the ground (0: the view's ray).
fn walk(rel: vec3<f32>, dir: vec3<f32>, start: f32, end: f32, angle: f32, safe: bool, lift: f32) -> Walk {
    let to_world = relief.march.x;
    let margin = select(0.0, relief.march.z, safe);
    let exaggeration = to_world * EARTH_RADIUS_M;
    var result = Walk(start, 0.0, -1.0, false, 3.4e38);
    var t = start;
    var last = vec2<f32>(-1.0, 0.0); // the last point above the heights and its clearance
    // Whether the walk went under the maxima since `above` (it may have passed a ridge
    // the CPU's steps meet).
    var loose = false;
    var next_mip = TOP_MIP;
    // The tiles last looked up for the step's own level and the ones on either side.
    var own_looked = Looked(vec3<u32>(0xffffffffu), -1);
    var coarse_looked = own_looked;
    var fine_looked = own_looked;
    var met = Met(0.0, 0.0, 0.0, 0.0, -1, false, 1.0);
    for (var step = 0u; step < relief.raster.w; step++) {
        let d = rel + dir * t;
        let p = relief.camera.xyz + d;
        let h = height_above(p);
        if t >= end || h <= 0.0 {
            if !loose { result.above = t; }
            return result;
        }
        let at = spot(p, dir);
        if at.past_limit { result.unknown = true; return result; }
        let zoom = log2(2.0 * PI * EARTH_RADIUS_M * max(at.latitude_cosine, MIN_LATITUDE_COSINE) / (DEM_TEXELS * max(t * EARTH_RADIUS_M * angle, 0.01) * PIXELS_PER_TEXEL));
        let level = clamp(i32(floor(zoom)), 0, MAX_ZOOM);
        if level >= COARSE_COORDINATE_LEVEL && !is_near(level, d) { result.unknown = true; return result; }
        let fraction = zoom - f32(level);
        let blends = relief.march.w > 0.5 && level > 0 && fraction < BLEND_LEVELS;
        let weight = select(1.0, clamp(fraction / BLEND_LEVELS, 0.0, 1.0), blends);
        let finer = safe && level < MAX_ZOOM && fraction > 1.0 - LEVEL_EDGE;
        let coarser = level > 0 && ((safe && fraction < LEVEL_EDGE) || (relief.march.w > 0.5 && fraction < BLEND_LEVELS + select(0.0, LEVEL_EDGE, safe)));
        let texels = DEM_TEXELS * exp2(f32(level));
        let speed = max(length(at.rate) * texels, 1e-12);
        var nudge = max(EXIT_NUDGE_TEXELS, UV_STEP * texels) / speed;
        if at.climb < 0.0 { nudge = min(nudge, NUDGE_DROP_M * to_world / -at.climb); }
        result.texel_t = 1.0 / speed;
        // Exits come from the map's rate here, but the ray's path over the map bends
        // (meridians converge, Mercator stretches north): a step stays short enough
        // that it strays less than a texel, within the maxima's widening of two.
        let texel_world = 2.0 * PI * max(at.latitude_cosine, MIN_LATITUDE_COSINE) / texels;
        let tangent = sqrt(max(1.0 - at.latitude_cosine * at.latitude_cosine, 0.0)) / max(at.latitude_cosine, MIN_LATITUDE_COSINE);
        let longest = sqrt(2.0 / (texel_world * (1.0 + tangent))) / speed;
        // The tiles under the point, looked up once a step.
        let own = locate(level, d, at.uv, &own_looked);
        var coarse_at = Place(-1, vec2<f32>(0.0));
        var fine_at = Place(-1, vec2<f32>(0.0));
        if coarser { coarse_at = locate(level - 1, d, at.uv, &coarse_looked); }
        if finer { fine_at = locate(level + 1, d, at.uv, &fine_looked); }
        if own.layer < 0 || (coarser && coarse_at.layer < 0) || (finer && fine_at.layer < 0) {
            result.unknown = true;
            return result;
        }
        var moved = false;
        // One level coarser than the last block cleared at most (the max-mipmap walk).
        for (var mip = min(next_mip, TOP_MIP); mip >= 1; mip--) {
            var top = block_top(own, mip);
            if coarser { top = max(top, block_top(coarse_at, mip - 1)); }
            if finer { top = max(top, block_top(fine_at, mip + 1)); }
            let top_world = top * to_world + margin;
            if h > top_world {
                let size = exp2(f32(mip + 1));
                let inside = own.at - floor(own.at / size) * size;
                let rate = at.rate * texels;
                var exit = min(min(leave(inside.x, rate.x, size), leave(inside.y, rate.y, size)), longest);
                if at.climb < 0.0 { exit = min(exit, (h - top_world) / -at.climb); }
                last = vec2<f32>(t, h - top_world);
                t += max(exit, 0.0) + nudge;
                // Past a stretch under the maxima the open air no longer counts.
                if !loose { result.above = t; }
                next_mip = mip + 1;
                moved = true;
                break;
            }
        }
        if moved { continue; }
        // Within a block of the ground: texel by texel.
        next_mip = 1;
        var top = texel_top(own);
        if coarser { top = max(top, texel_top(coarse_at)); }
        // The finer level's texels are half as wide: its blocks of two (widened by
        // two of its texels) cover the whole texel this step may cross.
        if finer { top = max(top, block_top(fine_at, 0)); }
        var ground_world = top * to_world + margin;
        let over_top = h > ground_world;
        let rate = at.rate * texels;
        let inside = own.at - floor(own.at);
        var exit = min(leave(inside.x, rate.x, 1.0), leave(inside.y, rate.y, 1.0));
        // The view's ray under the texel's highest point: the ground along the ray to
        // the next of the heights' bilinear cells, exactly (the coarser level's cells
        // end at this level's texel edges), in place of quarter-texel samples.
        var along = vec3<f32>(0.0);
        if !over_top {
            // Under the texel's highest point: the heights themselves, as the CPU's.
            var ground: f32;
            if lift <= 0.0 {
                along = bilinear_along(own, rate);
                if blends {
                    let coarse = bilinear_along(coarse_at, rate * 0.5);
                    along = coarse + (along - coarse) * weight;
                }
                ground = along.x;
            } else {
                ground = bilinear(own);
                if blends {
                    let coarse_m = bilinear(coarse_at);
                    ground = coarse_m + (ground - coarse_m) * weight;
                }
            }
            ground_world = ground * to_world;
            if h <= ground_world {
                met = Met(last.x, last.y, t, h - ground_world, level, blends, weight);
                break;
            }
            if lift > 0.0 {
                let texel_world = 2.0 * PI * at.latitude_cosine / texels;
                let soft = (t - start) * SUN_RADIUS_RAD + min(texel_world * SHADOW_SOFT_TEXEL_SHARE * exaggeration, lift / 2.0);
                if soft > 0.0 { result.least = min(result.least, (h - ground_world) / soft); }
            }
        }
        last = vec2<f32>(t, h - ground_world);
        if !over_top && lift <= 0.0 {
            let cell = own.at - vec2<f32>(0.5) - floor(own.at - vec2<f32>(0.5));
            exit = min(min(exit, longest), min(leave(cell.x, rate.x, 1.0), leave(cell.y, rate.y, 1.0)));
            // A ray going down ends the piece where it would reach the ground's height
            // here: a ray straight down crosses no cell.
            if at.climb < 0.0 { exit = min(exit, 2.0 * (h - ground_world) / -at.climb); }
            // The clearance over the piece: c0 + c1·s + c2·s² (the ray rises at `climb`).
            let c0 = h - ground_world;
            let c1 = at.climb - along.y * to_world;
            let c2 = -along.z * to_world;
            var low_s = max(exit, 0.0);
            var low = c0 + low_s * (c1 + low_s * c2);
            if c2 > 0.0 {
                let s = -c1 / (2.0 * c2);
                let there = c0 + s * (c1 + s * c2);
                if s > 0.0 && s < low_s && there < low { low_s = s; low = there; }
            }
            if low <= 0.0 {
                // The first root, bracketed closely for the refinement after the loop.
                var root = low_s;
                if abs(c2) * low_s <= 1e-6 * abs(c1) {
                    if c1 < 0.0 { root = min(-c0 / c1, low_s); }
                } else {
                    let disc = max(c1 * c1 - 4.0 * c2 * c0, 0.0);
                    let q = -0.5 * (c1 + select(-1.0, 1.0, c1 >= 0.0) * sqrt(disc));
                    let r1 = q / c2;
                    let r2 = c0 / q;
                    let first = min(select(3.4e38, r1, r1 > 0.0), select(3.4e38, r2, r2 > 0.0));
                    root = min(first, low_s);
                }
                let before = root * (1.0 - ROOT_BRACKET);
                let after = min(root * (1.0 + ROOT_BRACKET), low_s);
                met = Met(
                    t + before, c0 + before * (c1 + before * c2),
                    t + after, min(c0 + after * (c1 + after * c2), 0.0),
                    level, blends, weight,
                );
                break;
            }
        } else {
            if at.climb < 0.0 { exit = min(exit, (h - ground_world) / -at.climb); }
            // Toward the sun under the maxima the heights are read more densely than
            // the CPU reads them: a ridge the CPU's steps meet, these meet too.
            if !over_top { exit = min(exit, LOOSE_STEP_TEXELS / speed); }
        }
        t += max(exit, 0.0) + nudge;
        if over_top && !loose { result.above = t; }
        loose = loose || !over_top;
    }
    if met.level < 0 {
        result.unknown = true;
        return result;
    }
    // Met the ground: refined between the last point above and this one. Here, after
    // the loop, a group of threads refines once; inside it, once for every step any of
    // its threads met the ground on. Toward the sun only meeting it counts.
    var t0 = met.t0; var c0 = met.c0;
    var t1 = met.t1; var c1 = met.c1;
    if t0 >= 0.0 && lift <= 0.0 {
        for (var i = 0; i < HIT_REFINEMENTS; i++) {
            if c0 - c1 <= 0.0 { break; }
            let tm = t0 + (t1 - t0) * c0 / (c0 - c1);
            let dm = rel + dir * tm;
            let pm = relief.camera.xyz + dm;
            let sm = spot(pm, dir);
            var gm = bilinear(locate(met.level, dm, sm.uv, &own_looked));
            if met.blends {
                let cm = bilinear(locate(met.level - 1, dm, sm.uv, &coarse_looked));
                gm = cm + (gm - cm) * met.weight;
            }
            let c = height_above(pm) - gm * to_world;
            if c > 0.0 { t0 = tm; c0 = c; } else { t1 = tm; c1 = c; }
        }
    }
    result.hit = t1;
    return result;
}

// The share of the sun seen at the relief's ground `p` met `distance` along the view
// (`terrain::sun_light`); -1 when not known.
fn sun_seen(rel: vec3<f32>, distance: f32) -> f32 {
    let p = relief.camera.xyz + rel;
    let sun = relief.sun.xyz;
    let up = normalize(p * relief.q.xyz);
    let to_world = relief.march.x;
    let exaggeration = to_world * EARTH_RADIUS_M;
    let fade = relief.shade.y;
    let dip = sqrt(2.0 * max(height_above(p) / max(exaggeration, 1e-30), 0.0)) * fade;
    if fade <= 0.0 || dot(sun, up) + dip <= SUN_RISE_START_COSINE { return 1.0; }
    let angle = relief.march.y * SHADOW_DETAIL_COARSER;
    let texel = distance * angle * PIXELS_PER_TEXEL;
    let lift = max(SHADOW_LIFT_M * to_world, texel * SHADOW_LIFT_TEXELS * exaggeration);
    let start_rel = rel + up * lift - sun * distance;
    let shell = span(relief.camera.xyz + start_rel, sun, relief.axes.xyz + vec3<f32>(relief.axes.w));
    if shell.x > shell.y { return 1.0; }
    let toward = walk(start_rel, sun, max(shell.x, distance), shell.y, angle, false, lift);
    if toward.hit >= 0.0 { return 1.0 - fade; }
    if toward.unknown { return -1.0; }
    let x = clamp(toward.least, 0.0, 1.0);
    return 1.0 - fade * (1.0 - x * x * (3.0 - 2.0 * x));
}

// The relief's slope (rise over run east and north, exaggerated, capped) at `at` of
// `level`, as `terrain::slope` reads `Elevation::slopes_m`.
fn slope_of(at: Place, level: i32, latitude_cosine: f32) -> vec2<f32> {
    let p = at.at - vec2<f32>(0.5);
    let corner = vec2<i32>(floor(p));
    let f = p - floor(p);
    let layer = at.layer;
    var sample = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    let offsets = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    for (var k = 0; k < 4; k++) {
        let c = corner + offsets[k];
        let a = height_of(layer, c);
        let b = height_of(layer, c + vec2<i32>(1, 0));
        let cc = height_of(layer, c + vec2<i32>(0, 1));
        let d = height_of(layer, c + vec2<i32>(1, 1));
        let top = a + (b - a) * f.x;
        sample[k] = top + (cc + (d - cc) * f.x - top) * f.y;
    }
    let run_m = 2.0 * PI * EARTH_RADIUS_M * max(latitude_cosine, MIN_LATITUDE_COSINE) / (exp2(f32(level)) * DEM_TEXELS);
    let exaggeration = relief.march.x * EARTH_RADIUS_M;
    let rise = vec2<f32>(sample[0] - sample[1], sample[2] - sample[3]);
    return clamp(rise / (2.0 * run_m) * exaggeration, vec2<f32>(-MAX_SLOPE), vec2<f32>(MAX_SLOPE));
}
// The slope at the ground `t` along `dir`, as `terrain::met_at` and `hit_on` shade it:
// the level for that distance held as itself, blended from the next coarser one's
// slope over the level's band where the heights blend; NO_SLOPE where a tile that
// needs is not held (or past the map's limit), and the CPU shades it.
fn hit_slope(dir: vec3<f32>, t: f32) -> vec2<f32> {
    let d = dir * t;
    let p = relief.camera.xyz + d;
    if abs(p.z) / length(p) > MERCATOR_MAX_SINE { return vec2<f32>(NO_SLOPE); }
    let at = spot(p, dir);
    let zoom = log2(2.0 * PI * EARTH_RADIUS_M * max(at.latitude_cosine, MIN_LATITUDE_COSINE) / (DEM_TEXELS * max(t * EARTH_RADIUS_M * relief.march.y, 0.01) * PIXELS_PER_TEXEL));
    let level = clamp(i32(floor(zoom)), 0, MAX_ZOOM);
    if level >= COARSE_COORDINATE_LEVEL && !is_near(level, d) { return vec2<f32>(NO_SLOPE); }
    var looked = Looked(vec3<u32>(0xffffffffu), -1);
    let own = locate(level, d, at.uv, &looked);
    if own.layer < 0 { return vec2<f32>(NO_SLOPE); }
    var slope = slope_of(own, level, at.latitude_cosine);
    let toward = clamp(zoom - f32(level), 0.0, 1.0);
    if toward < 1.0 && relief.march.w > 0.5 && level > 0 {
        var coarse_looked = Looked(vec3<u32>(0xffffffffu), -1);
        let coarse = locate(level - 1, d, at.uv, &coarse_looked);
        if coarse.layer < 0 { return vec2<f32>(NO_SLOPE); }
        let before = slope_of(coarse, level - 1, at.latitude_cosine);
        slope = before + (slope - before) * toward;
    }
    return slope;
}

// The view's ray for pixel `i` of `width` (unit).
fn ray(i: u32, width: u32, height: u32) -> vec3<f32> {
    let x = i % width; let y = i / width;
    return ray_through(vec2<f32>(f32(x) + 0.5, f32(y) + 0.5), width, height);
}
// The view's ray through the point `at` (in pixels, from the top left corner).
fn ray_through(at: vec2<f32>, width: u32, height: u32) -> vec3<f32> {
    let nx = at.x / f32(width) * 2.0 - 1.0;
    let ny = 1.0 - at.y / f32(height) * 2.0;
    return normalize(relief.right.xyz * (nx * relief.camera.w * FOV_HALF_TAN) + relief.up.xyz * (ny * FOV_HALF_TAN) - relief.forward.xyz);
}

// Square blocks of pixels whose rays a cone walks first (`cone`): their own walks
// start where it ended. Up there the rays of a block walk alike, the open air was
// four in ten of their steps (2026-11-24), and one walk for 64 rays costs little.
const CONE_SIDE: u32 = 8u;
// The planes per pixel before the cones' starts (`relief::PLANES`).
const PLANES: u32 = 6u;
// A cone's rays stay within this share of a block of its middle ray's: the block and
// the eight around it hold them all.
const CONE_SHARE: f32 = 0.9;
// Not held, or no answer: a height no ray clears.
const UNCLEARED: f32 = 3.4e38;

// The highest top (metres) of mip `mip` at `offset` (texels) from `at`, a place in
// the tile `key` (level, x, y): in the neighbouring tile past its edge. UNCLEARED where
// that tile is not held or past the map.
fn corner_top(key: vec3<u32>, at: Place, offset: vec2<f32>, mip: i32) -> f32 {
    let corner = at.at + offset;
    let shift = floor(corner / DEM_TEXELS);
    var layer = at.layer;
    if any(shift != vec2<f32>(0.0)) {
        let n = i32(1u << key.x);
        let x = ((i32(key.y) + i32(shift.x)) % n + n) % n;
        let y = i32(key.z) + i32(shift.y);
        if y < 0 || y >= n { return UNCLEARED; }
        layer = layer_of(key.x, u32(x), u32(y));
        if layer >= 0 && read_tiles[layer] == 0u { read_tiles[layer] = 1u; }
    }
    if layer < 0 { return UNCLEARED; }
    return block_top(Place(layer, corner - shift * DEM_TEXELS), mip);
}
// The highest top (metres) of mip `mip` (blocks `size` texels wide) over the blocks a
// cone sweeps: from `at`, `path` texels on, `radius` texels around (at most three
// blocks across: the path stays within a block, the cone narrower than one).
fn swept_top(key: vec3<u32>, at: Place, path: vec2<f32>, radius: f32, size: f32, mip: i32) -> f32 {
    let low = floor((min(at.at, at.at + path) - vec2<f32>(radius)) / size);
    let high = floor((max(at.at, at.at + path) + vec2<f32>(radius)) / size);
    let origin = Place(at.layer, vec2<f32>(0.0));
    var top = 0.0;
    for (var y = low.y; y <= high.y; y += 1.0) {
        for (var x = low.x; x <= high.x; x += 1.0) {
            top = max(top, corner_top(key, origin, (vec2<f32>(x, y) + vec2<f32>(0.5)) * size, mip));
        }
    }
    return top;
}

// How far along `dir` (from the camera, from `start` to `end`) every ray within
// `spread` of it (world units per unit along the ray) stays over the maxima, with
// the margin and the levels either side of an edge as the view's `walk`: the
// max-mipmap walk over the blocks around the ray's, while the cone is narrower than
// a block.
fn cone_walk(dir: vec3<f32>, start: f32, end: f32, spread: f32) -> f32 {
    let to_world = relief.march.x;
    let margin = relief.march.z;
    var t = start;
    var next_mip = TOP_MIP;
    var own_looked = Looked(vec3<u32>(0xffffffffu), -1);
    var coarse_looked = own_looked;
    var fine_looked = own_looked;
    for (var step = 0u; step < relief.raster.w; step++) {
        let d = dir * t;
        let p = relief.camera.xyz + d;
        let h = height_above(p);
        if t >= end || h <= 0.0 { return t; }
        let at = spot(p, dir);
        if at.past_limit || at.latitude_cosine < MIN_LATITUDE_COSINE { return t; }
        let zoom = log2(2.0 * PI * EARTH_RADIUS_M * at.latitude_cosine / (DEM_TEXELS * max(t * EARTH_RADIUS_M * relief.march.y, 0.01) * PIXELS_PER_TEXEL));
        let level = clamp(i32(floor(zoom)), 0, MAX_ZOOM);
        if level >= COARSE_COORDINATE_LEVEL && !is_near(level, d) { return t; }
        let fraction = zoom - f32(level);
        let finer = level < MAX_ZOOM && fraction > 1.0 - LEVEL_EDGE;
        let coarser = level > 0 && (fraction < LEVEL_EDGE || (relief.march.w > 0.5 && fraction < BLEND_LEVELS + LEVEL_EDGE));
        let texels = DEM_TEXELS * exp2(f32(level));
        let speed = max(length(at.rate) * texels, 1e-12);
        var nudge = max(EXIT_NUDGE_TEXELS, UV_STEP * texels) / speed;
        if at.climb < 0.0 { nudge = min(nudge, NUDGE_DROP_M * to_world / -at.climb); }
        // A texel's width here (world units): the cone's rays and blocks in texels.
        let texel_world = 2.0 * PI * at.latitude_cosine / texels;
        let tangent = sqrt(max(1.0 - at.latitude_cosine * at.latitude_cosine, 0.0)) / at.latitude_cosine;
        let longest = sqrt(2.0 / (texel_world * (1.0 + tangent))) / speed;
        let sine = sqrt(max(1.0 - at.latitude_cosine * at.latitude_cosine, 0.0));
        let own = locate(level, d, at.uv, &own_looked);
        var coarse_at = Place(-1, vec2<f32>(0.0));
        var fine_at = Place(-1, vec2<f32>(0.0));
        if coarser { coarse_at = locate(level - 1, d, at.uv, &coarse_looked); }
        if finer { fine_at = locate(level + 1, d, at.uv, &fine_looked); }
        if own.layer < 0 || (coarser && coarse_at.layer < 0) || (finer && fine_at.layer < 0) { return t; }
        let radius = spread * t;
        var moved = false;
        for (var mip = min(next_mip, TOP_MIP); mip >= 1; mip--) {
            let size = exp2(f32(mip + 1));
            // The nine blocks (world units, middle to corner) within the map's limit
            // and, where the rays need it, the near map's range.
            let reach = 2.13 * size * texel_world;
            if sine + reach > MERCATOR_MAX_SINE { continue; }
            let far = length(d) + reach;
            if level >= COARSE_COORDINATE_LEVEL && far * far >= relief.near_frac.z { continue; }
            let widest = CONE_SHARE * size * texel_world;
            if radius >= widest { continue; }
            // To the block's edge, as the view's walk, while the cone stays narrow enough.
            let inside = own.at - floor(own.at / size) * size;
            let rate = at.rate * texels;
            var exit = min(min(leave(inside.x, rate.x, size), leave(inside.y, rate.y, size)), longest);
            exit = min(exit, (widest - radius) / spread);
            // The blocks the cone sweeps to there, at each level the rays read.
            let path = rate * (exit + nudge);
            let swept = (radius + spread * (exit + nudge)) / texel_world;
            var top = swept_top(own_looked.key, own, path, swept, size, mip);
            if coarser { top = max(top, swept_top(coarse_looked.key, coarse_at, path / 2.0, swept / 2.0, size / 2.0, mip - 1)); }
            if finer { top = max(top, swept_top(fine_looked.key, fine_at, path * 2.0, swept * 2.0, size * 2.0, mip + 1)); }
            let clear = h - radius - (top * to_world + margin);
            if clear <= 0.0 { continue; }
            // The cone's lowest ray nears the maxima at most this fast (a shorter step
            // sweeps less).
            let sink = spread - at.climb;
            if sink > 0.0 { exit = min(exit, clear / sink); }
            if exit <= t * 1e-6 { return t; }
            t += exit + nudge;
            next_mip = mip + 1;
            moved = true;
            break;
        }
        if !moved { return t; }
    }
    return t;
}

// One cone a block of `CONE_SIDE` pixels across: where its rays' own walks start
// (after the pixels' planes; 0 where it does not help).
@compute @workgroup_size(8, 8)
fn cone(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = relief.raster.x; let height = relief.raster.y;
    let across = (width + CONE_SIDE - 1u) / CONE_SIDE;
    if id.x >= across || id.y * CONE_SIDE >= height { return; }
    let slot = PLANES * width * height + id.y * across + id.x;
    starts[slot] = 0.0;
    let low = id.xy * CONE_SIDE;
    let high = min(low + vec2<u32>(CONE_SIDE - 1u), vec2<u32>(width - 1u, height - 1u));
    let dir = ray_through(vec2<f32>(low + high) / 2.0 + vec2<f32>(0.5), width, height);
    // The block's rays are no farther from the middle one than its corners' (the rays
    // within an angle of it fill a cone, whose section with the screen is convex).
    var spread = 0.0;
    for (var corner = 0u; corner < 4u; corner++) {
        let at = vec2<u32>(select(low.x, high.x, (corner & 1u) == 1u), select(low.y, high.y, corner >= 2u));
        spread = max(spread, length(ray_through(vec2<f32>(at) + vec2<f32>(0.5), width, height) - dir));
    }
    spread = spread * 1.01 + 1e-7;
    let camera = relief.camera.xyz;
    let shell = span(camera, dir, relief.axes.xyz + vec3<f32>(relief.axes.w));
    if shell.x > shell.y || shell.y <= 0.0 { return; }
    // Before the shell widened by the cone as wide as where the middle ray enters
    // it, every ray of the block is outside the shell.
    let wide = span(camera, dir, relief.axes.xyz + vec3<f32>(relief.axes.w + spread * max(shell.x, 0.0) * 1.01));
    starts[slot] = cone_walk(dir, max(wide.x, 0.0), shell.y, spread);
}

// The view's rays (`starts`, hits and trust); the sun's share comes in a pass of its
// own (`sun`): the two walks in one kernel kept fewer pixels in flight.
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = relief.raster.x; let height = relief.raster.y;
    if id.x >= width || id.y >= height { return; }
    let i = id.y * width + id.x;
    let count = width * height;
    starts[count + i] = -1.0;
    starts[2u * count + i] = -1.0;
    starts[3u * count + i] = 0.0;
    starts[4u * count + i] = NO_SLOPE;
    starts[5u * count + i] = NO_SLOPE;
    let dir = ray(i, width, height);
    let camera = relief.camera.xyz;
    let shell = span(camera, dir, relief.axes.xyz + vec3<f32>(relief.axes.w));
    if shell.x > shell.y || shell.y <= 0.0 { starts[i] = 0.0; starts[4u * count + i] = SKY; return; }
    var end = shell.y;
    let floor_hit = span(camera, dir, relief.axes.xyz);
    let floor_met = floor_hit.x <= floor_hit.y && floor_hit.x > 0.0;
    if floor_met { end = min(end, floor_hit.x); }
    // Where the block's cone ended (`cone`), every ray of the block is over the maxima.
    let across = (width + CONE_SIDE - 1u) / CONE_SIDE;
    let cone_start = starts[PLANES * count + (id.y / CONE_SIDE) * across + id.x / CONE_SIDE];
    let first = max(max(shell.x, 0.0), cone_start);
    let view = walk(vec3<f32>(0.0), dir, first, end, relief.march.y, true, 0.0);
    // The CPU walks the last texel again: its own steps between exact measurements put
    // the ground up to a texel from where the GPU sees it.
    starts[i] = select(max(view.above - BACK_TEXELS * view.texel_t, 0.0), end, view.above >= end);
    // -1: the walk gave up without an answer (`relief::helps`).
    if view.unknown && view.hit < 0.0 { starts[3u * count + i] = -1.0; }
    if !floor_met && !view.unknown && view.hit < 0.0 && view.above >= end { starts[4u * count + i] = SKY; }
    // The CPU takes every ground the walk met as its own: under the maxima the walk
    // meets the heights exactly in each bilinear cell (on synthetic relief one ray in
    // 8,000 then meets the ground past a ridge tip the CPU's march measured at every
    // texel meets, where the f32 level falls on the other side of a level's edge; the
    // frame's march, measured every 48 texels, differs four times as often).
    if view.hit >= 0.0 {
        starts[count + i] = view.hit;
        starts[3u * count + i] = 1.0;
        let slope = hit_slope(dir, view.hit);
        starts[4u * count + i] = slope.x;
        starts[5u * count + i] = slope.y;
    }
}

// The share of the sun seen at pixel `i` where `main` met the ground.
fn sun_at(i: u32, width: u32, height: u32) {
    let count = width * height;
    let hit = starts[count + i];
    if hit >= 0.0 { starts[2u * count + i] = sun_seen(ray(i, width, height) * hit, hit); }
}
// The share of the sun seen where `main` met the ground, walked at every other pixel
// of every other row (and at those with no neighbour on one side to share from: a
// last odd column or row); `sun_rows` then the rest of the even columns, from above
// and below, and `sun_shared` the odd columns, from left and right.
@compute @workgroup_size(8, 8)
fn sun(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = relief.raster.x; let height = relief.raster.y;
    if id.x >= (width + 1u) / 2u || id.y >= (height + 1u) / 2u || relief.sun.w < 0.5 { return; }
    let x = id.x * 2u; let y = id.y * 2u;
    let last_column = x + 2u == width;
    let last_row = y + 2u == height;
    sun_at(y * width + x, width, height);
    if last_column { sun_at(y * width + x + 1u, width, height); }
    if last_row { sun_at((y + 1u) * width + x, width, height); }
    if last_column && last_row { sun_at((y + 1u) * width + x + 1u, width, height); }
}
// Neighbours count as at the same depth within this share of the distance (the CPU's
// `SAME_DEPTH_SHARE`).
const SAME_DEPTH_SHARE: f32 = __SAME_DEPTH_SHARE__;
// Whether pixel `i` between the pixels `a` and `b`, both wholly lit or both wholly in
// shadow at its own depth, takes their share, as the CPU's `SunSeen` (which looks left
// only). A walk toward the sun was half of the relief walk's time on an M1 (2026-11-24).
fn shares(i: u32, a: u32, b: u32, count: u32) -> bool {
    let hit = starts[count + i];
    let before = starts[count + a];
    let after = starts[count + b];
    let seen = starts[2u * count + a];
    let full_shade = 1.0 - relief.shade.y;
    return before >= 0.0 && after >= 0.0 && abs(before - hit) <= hit * SAME_DEPTH_SHARE
        && abs(after - hit) <= hit * SAME_DEPTH_SHARE && seen == starts[2u * count + b]
        && (seen == 1.0 || seen == full_shade);
}
// The pixels still to walk are gathered in each workgroup first and walked by its first
// threads: a group of threads takes as long as its longest walk, so a walk or two among
// shared pixels otherwise cost the group a whole walk.
const SHARED_GROUP: u32 = 256u;
var<workgroup> pending: array<u32, SHARED_GROUP>;
var<workgroup> pending_count: atomic<u32>;
// Takes the share of `a` and `b` at pixel `i` (with a ground), or queues `i` to walk.
fn share_or_queue(i: u32, a: u32, b: u32, count: u32) {
    if starts[count + i] < 0.0 { return; }
    if shares(i, a, b, count) {
        starts[2u * count + i] = starts[2u * count + a];
    } else {
        pending[atomicAdd(&pending_count, 1u)] = i;
    }
}
fn walk_pending(local: u32, width: u32, height: u32) {
    if local < atomicLoad(&pending_count) { sun_at(pending[local], width, height); }
}
// The even columns' odd rows (and a last odd column's), from the rows above and below.
@compute @workgroup_size(16, 16)
fn sun_rows(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    if local == 0u { atomicStore(&pending_count, 0u); }
    workgroupBarrier();
    let width = relief.raster.x; let height = relief.raster.y;
    let count = width * height;
    let x = id.x;
    let y = id.y * 2u + 1u;
    if relief.sun.w > 0.5 && x < width && y + 1u < height && (x % 2u == 0u || x + 1u == width) {
        let i = y * width + x;
        share_or_queue(i, i - width, i + width, count);
    }
    workgroupBarrier();
    walk_pending(local, width, height);
}
// The odd columns, from the columns on either side.
@compute @workgroup_size(16, 16)
fn sun_shared(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    if local == 0u { atomicStore(&pending_count, 0u); }
    workgroupBarrier();
    let width = relief.raster.x; let height = relief.raster.y;
    let count = width * height;
    let x = id.x * 2u + 1u;
    if relief.sun.w > 0.5 && id.y < height && x + 1u < width {
        let i = id.y * width + x;
        share_or_queue(i, i - 1u, i + 1u, count);
    }
    workgroupBarrier();
    walk_pending(local, width, height);
}
