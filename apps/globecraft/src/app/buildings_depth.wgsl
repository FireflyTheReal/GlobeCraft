// Blocks with a depth buffer: positions arrive in clip space, colours as egui's
// premultiplied gamma bytes, which egui reads back unchanged from the picture.
struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn vertex(
    @location(0) at: vec2<f32>,
    @location(1) nearness: f32,
    @location(2) colour: vec4<f32>,
) -> Corner {
    return Corner(vec4<f32>(at, nearness, 1.0), colour);
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4<f32> {
    return corner.colour;
}
