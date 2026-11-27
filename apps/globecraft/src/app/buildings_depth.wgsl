// Blocks with a depth buffer: positions arrive in clip space, colours as egui's
// premultiplied gamma bytes, which egui reads back unchanged from the picture.
// A roof's corner on an imagery basemap also carries where the frame's picture
// shows the ground under it; the roof then shows that photograph, its colour the
// light on it (`BuildingVertex::photo`, `tint`).

// `NO_PHOTO` and `PHOTO_SCALE` in `live_layers::buildings`.
const NO_PHOTO: u32 = 65535u;
const PHOTO_SCALE: f32 = 65534.0;

@group(0) @binding(0) var photograph: texture_2d<f32>;
@group(0) @binding(1) var photograph_sampler: sampler;

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) photo: vec2<f32>,
    // 1 where the face is photographed.
    @location(2) @interpolate(flat) photographed: f32,
}

@vertex
fn vertex(
    @location(0) at: vec2<f32>,
    @location(1) nearness: f32,
    @location(2) colour: vec4<f32>,
    @location(3) photo: vec2<u32>,
) -> Corner {
    let photographed = select(1.0, 0.0, photo.x == NO_PHOTO);
    return Corner(
        vec4<f32>(at, nearness, 1.0),
        colour,
        vec2<f32>(photo) / PHOTO_SCALE,
        photographed,
    );
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4<f32> {
    // Sampled everywhere (uniform control flow), used only on photographed faces.
    let photo = textureSample(photograph, photograph_sampler, corner.photo).rgb;
    if (corner.photographed < 0.5) {
        return corner.colour;
    }
    // The tint is premultiplied: its alpha already scales its light.
    return vec4<f32>(photo * corner.colour.rgb, corner.colour.a);
}
