// Blocks with a depth buffer: positions arrive in clip space, colours as egui's
// premultiplied gamma bytes, which egui reads back unchanged from the picture.
// A roof's corner on an imagery basemap also carries its block of 2×2 imagery tiles
// and where in the block it lies; the roof is drawn as those tiles (each cell's own
// or a coarser stand-in), its colour the light on it (`BuildingVertex::roof`,
// `roof_at`, `tint`).

// `NO_ROOF` in `live_layers::roof_imagery`, `NO_TILE` in `app::buildings`.
const NO_ROOF: u32 = 0xffffffffu;
const NO_TILE: u32 = 0xffffffffu;
// A cell without a tile (none loaded, or no room in the atlas): a plain roof grey,
// `ROOF` in `live_layers::buildings`.
const ROOF_GREY: vec3<f32> = vec3<f32>(0.74, 0.72, 0.70);

// A block: its first tile at its zoom, and its cells (x, y), (x + 1, y),
// (x, y + 1), (x + 1, y + 1), each an atlas layer and the levels its tile is coarser.
struct Block {
    x: u32,
    y: u32,
    cells: array<u32, 4>,
}

@group(0) @binding(0) var roof_tiles: texture_2d_array<f32>;
@group(0) @binding(1) var roof_sampler: sampler;
@group(0) @binding(2) var<storage, read> blocks: array<Block>;

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    // 0 to 1 across the block's two tiles.
    @location(1) roof_at: vec2<f32>,
    @location(2) @interpolate(flat) roof: u32,
}

@vertex
fn vertex(
    @location(0) at: vec2<f32>,
    @location(1) nearness: f32,
    @location(2) colour: vec4<f32>,
    @location(3) roof_at: vec2<f32>,
    @location(4) roof: u32,
) -> Corner {
    return Corner(vec4<f32>(at, nearness, 1.0), colour, roof_at, roof);
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4<f32> {
    if (corner.roof == NO_ROOF) {
        return corner.colour;
    }
    let block = blocks[corner.roof];
    let place = clamp(corner.roof_at * 2.0, vec2<f32>(0.0), vec2<f32>(1.999999));
    let cell = vec2<u32>(floor(place));
    let entry = block.cells[cell.y * 2u + cell.x];
    // The tint is premultiplied: its alpha already scales its light.
    if (entry == NO_TILE) {
        return vec4<f32>(ROOF_GREY * corner.colour.rgb, corner.colour.a);
    }
    // The cell's part of its tile, which is `coarser` levels up.
    let coarser = entry >> 16u;
    let span = 1u << coarser;
    let tile = vec2<u32>(block.x, block.y) + cell;
    let within = (vec2<f32>(tile & vec2<u32>(span - 1u)) + fract(place)) / f32(span);
    let texel = textureSampleLevel(roof_tiles, roof_sampler, within, i32(entry & 0xffffu), 0.0);
    return vec4<f32>(texel.rgb * corner.colour.rgb, corner.colour.a);
}
