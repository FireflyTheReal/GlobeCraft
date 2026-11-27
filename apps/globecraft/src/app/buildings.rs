//! Buildings as blocks close up (roadmap 8.6); the geometry is projected by the
//! live-layer worker (`globecraft_render::live_layers::BuildingMesh`). Where the
//! interface is presented by wgpu the blocks are drawn into their own picture with a
//! depth buffer, so a block behind never shows through one in front, and roofs on an
//! imagery basemap show the frame's photograph of the ground under them (D3); the
//! OpenGL fallback draws the faces far to near in their colours, which big roofs can
//! get wrong.
use eframe::egui;
use globecraft_data::buildings::BuildingSnapshot;
use globecraft_render::live_layers::{BuildingMesh, LiveView};
use std::sync::Arc;

/// Samples per pixel of the blocks' picture: smooth edges.
const SAMPLES: u32 = 4;
/// Not sRGB: egui takes a texture's values as the gamma bytes it draws with (an
/// sRGB picture came out dark, 2026-11-16). egui's own textures are the same, so
/// the frame's photograph is read as the bytes it shows.
const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// A vertex in the buffer: clip position (two `f32`), nearness (`f32`), colour, the
/// photo's place (two `u16`).
const VERTEX_BYTES: u64 = 20;

#[derive(Default)]
pub(super) struct BuildingOverlay {
    /// Overture's building tiles.
    pub(super) tiles: globecraft_data::buildings::BuildingTiles,
    /// The tiles of blocks drawn.
    pub(super) current: Option<Arc<BuildingSnapshot>>,
    /// The interface's GPU, when wgpu presents it.
    pub(super) gpu: Option<egui_wgpu::RenderState>,
    picture: Option<Picture>,
    /// What the shape last drawn shows, and the shape.
    drawn: Option<(Drawn, egui::Shape)>,
}

/// What the blocks' picture was drawn from: the mesh, and the frame whose
/// photograph its roofs show (its texture and its pixels, which change in place).
#[derive(Clone)]
struct Drawn {
    mesh: Arc<BuildingMesh>,
    frame: Option<ShownFrame>,
}

impl Drawn {
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.mesh, &other.mesh) && self.frame == other.frame
    }
}

/// The globe's frame on screen: its texture, its pixels (whose address changes with
/// each new frame) and its size.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ShownFrame {
    pub(super) texture: egui::TextureId,
    pub(super) pixels: usize,
    pub(super) size: [u32; 2],
}

/// The blocks' own picture on the GPU, shown as an egui texture.
struct Picture {
    corners: KeptBuffer,
    triangles: KeptBuffer,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// Sampled where there is no photograph: the roofs then keep their colours.
    blank: wgpu::TextureView,
    size: [u32; 2],
    samples: wgpu::TextureView,
    depth: wgpu::TextureView,
    shown: wgpu::TextureView,
    id: egui::TextureId,
}

impl BuildingOverlay {
    pub(super) fn draw(
        &mut self,
        painter: &egui::Painter,
        mesh: &Arc<BuildingMesh>,
        view: &LiveView,
        frame: Option<ShownFrame>,
    ) {
        if mesh.indices.is_empty() {
            // The picture is as large as the window, several times over: not kept
            // while there is nothing to show.
            self.drawn = None;
            if let (Some(gpu), Some(picture)) = (&self.gpu, self.picture.take()) {
                gpu.renderer.write().free_texture(&picture.id);
            }
            return;
        }
        let wanted = Drawn {
            mesh: mesh.clone(),
            frame,
        };
        if self
            .drawn
            .as_ref()
            .is_none_or(|(drawn, _)| !drawn.same(&wanted))
        {
            match &self.gpu {
                Some(gpu) => {
                    match render(gpu, &mut self.picture, mesh, view, frame) {
                        Some((pass, id)) => {
                            // The pass runs when egui paints, after it has uploaded
                            // this frame's pictures, so the roofs show the frame on
                            // screen.
                            painter.add(egui_wgpu::Callback::new_paint_callback(view.rect, pass));
                            let whole =
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
                            let shape =
                                egui::Shape::image(id, view.rect, whole, egui::Color32::WHITE);
                            self.drawn = Some((wanted, shape));
                        }
                        // The frame's picture is not on the GPU yet (a new or resized
                        // frame is uploaded when egui paints): the picture before
                        // stays a moment longer.
                        None => painter.ctx().request_repaint(),
                    }
                }
                None => {
                    let shape = egui::Shape::mesh(egui::Mesh {
                        vertices: mesh
                            .vertices
                            .iter()
                            .map(|vertex| egui::epaint::Vertex {
                                pos: vertex.at,
                                uv: egui::epaint::WHITE_UV,
                                color: premultiplied(vertex.colour),
                            })
                            .collect(),
                        indices: mesh.indices.clone(),
                        ..Default::default()
                    });
                    self.drawn = Some((wanted, shape));
                }
            }
        }
        if let Some((_, shape)) = &self.drawn {
            painter.add(shape.clone());
        }
    }
}

fn premultiplied([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Readies `mesh` for drawing into the picture (made, or made again at a new size):
/// the pass to run when egui paints and the texture showing the picture. None
/// while the frame's picture is not on the GPU at its size yet.
fn render(
    gpu: &egui_wgpu::RenderState,
    picture: &mut Option<Picture>,
    mesh: &BuildingMesh,
    view: &LiveView,
    frame: Option<ShownFrame>,
) -> Option<(BuildingPass, egui::TextureId)> {
    let device = &gpu.device;
    // The frame's texture as the GPU holds it, at the frame's size: one replaced
    // this frame is still the old one until egui paints.
    let photo = match frame {
        Some(frame) => {
            let renderer = gpu.renderer.read();
            let texture = renderer.texture(&frame.texture)?.texture.clone()?;
            if [texture.width(), texture.height()] != frame.size {
                return None;
            }
            Some(texture.create_view(&Default::default()))
        }
        None => None,
    };
    let limit = device.limits().max_texture_dimension_2d;
    let size = [view.rect.width(), view.rect.height()]
        .map(|points| ((points * view.pixels_per_point).round() as u32).clamp(1, limit));
    if picture.as_ref().is_some_and(|old| old.size != size)
        && let Some(old) = picture.take()
    {
        // Its pipeline is cheap to make again; a window is seldom resized.
        gpu.renderer.write().free_texture(&old.id);
    }
    let picture = picture.get_or_insert_with(|| Picture::new(gpu, size));

    // Laid out by the live-layer worker for the view the mesh was projected for, and
    // written into buffers kept from mesh to mesh: made anew for each, a dense city's
    // 16 MB of corners were allocated on every frame of a moving view.
    let vertices = mesh.gpu_vertex_bytes();
    picture
        .corners
        .write(gpu, vertices, wgpu::BufferUsages::VERTEX);
    picture
        .triangles
        .write(gpu, mesh.index_bytes(), wgpu::BufferUsages::INDEX);
    let (Some(corners), Some(triangles)) = (&picture.corners.buffer, &picture.triangles.buffer)
    else {
        return None;
    };
    let photograph = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("building roofs"),
        layout: &picture.pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    photo.as_ref().unwrap_or(&picture.blank),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&picture.sampler),
            },
        ],
    });
    let pass = BuildingPass {
        pipeline: picture.pipeline.clone(),
        photograph,
        corners: (corners.clone(), vertices.len() as u64),
        triangles: (triangles.clone(), mesh.index_bytes().len() as u64),
        count: mesh.indices.len() as u32,
        samples: picture.samples.clone(),
        depth: picture.depth.clone(),
        shown: picture.shown.clone(),
    };
    Some((pass, picture.id))
}

/// The blocks drawn into their picture, run by egui as it paints.
struct BuildingPass {
    pipeline: wgpu::RenderPipeline,
    photograph: wgpu::BindGroup,
    /// The buffers and the bytes of them in use.
    corners: (wgpu::Buffer, u64),
    triangles: (wgpu::Buffer, u64),
    count: u32,
    samples: wgpu::TextureView,
    depth: wgpu::TextureView,
    shown: wgpu::TextureView,
}

impl egui_wgpu::CallbackTrait for BuildingPass {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        _resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("buildings"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.samples,
                depth_slice: None,
                resolve_target: Some(&self.shown),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                // Greater is nearer: nothing is at zero.
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.photograph, &[]);
        pass.set_vertex_buffer(0, self.corners.0.slice(..self.corners.1));
        pass.set_index_buffer(
            self.triangles.0.slice(..self.triangles.1),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..self.count, 0, 0..1);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        // Drawn in `prepare`, into the picture egui shows as an image.
    }
}

/// A GPU buffer written again for each mesh, made larger only when a mesh needs it.
#[derive(Default)]
struct KeptBuffer {
    buffer: Option<wgpu::Buffer>,
}

impl KeptBuffer {
    fn write(&mut self, gpu: &egui_wgpu::RenderState, bytes: &[u8], usage: wgpu::BufferUsages) {
        // Writes go in whole multiples of four bytes; the corners and triangles are.
        let size = (bytes.len() as u64).max(wgpu::COPY_BUFFER_ALIGNMENT);
        if self
            .buffer
            .as_ref()
            .is_none_or(|buffer| buffer.size() < size)
        {
            self.buffer = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("buildings"),
                // Room to grow, so a mesh a little larger does not make a new one.
                size: size.next_power_of_two(),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.buffer {
            gpu.queue.write_buffer(buffer, 0, bytes);
        }
    }
}

impl Picture {
    fn new(gpu: &egui_wgpu::RenderState, size: [u32; 2]) -> Self {
        let device = &gpu.device;
        let pipeline = pipeline(device);
        let texture = |label, format, sample_count, [width, height]: [u32; 2]| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let shown = texture("buildings", COLOUR_FORMAT, 1, size);
        let id =
            gpu.renderer
                .write()
                .register_native_texture(device, &shown, wgpu::FilterMode::Linear);
        Self {
            corners: KeptBuffer::default(),
            triangles: KeptBuffer::default(),
            pipeline,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("building roofs"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            blank: texture("no photograph", COLOUR_FORMAT, 1, [1, 1]),
            size,
            samples: texture("building samples", COLOUR_FORMAT, SAMPLES, size),
            depth: texture("building depth", DEPTH_FORMAT, SAMPLES, size),
            shown,
            id,
        }
    }
}

/// The pipeline drawing the blocks into their picture (`buildings_depth.wgsl`).
fn pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::include_wgsl!("buildings_depth.wgsl"));
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("buildings"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2, 1 => Float32, 2 => Unorm8x4, 3 => Uint16x2
                ],
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: SAMPLES,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            // No blending: the depth test leaves the nearest face, already
            // premultiplied, and egui blends the picture over the globe.
            targets: &[Some(COLOUR_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_render::live_layers::{NO_PHOTO, PHOTO_SCALE};

    /// A GPU to draw on (a software one will do), or none on a machine without.
    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    /// A corner as `GpuCorner` lays it out.
    fn corner(at: [f32; 2], colour: [u8; 4], photo: [u16; 2]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [at[0], at[1], 0.5] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(colour);
        for value in photo {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn photographed_roofs_show_the_frame_and_other_faces_their_colour() {
        let Some((device, queue)) = device() else {
            eprintln!("No GPU adapter: skipped");
            return;
        };
        const SIZE: u32 = 8;
        let texture = |format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        // The frame: one colour all over.
        let frame = texture(
            COLOUR_FORMAT,
            1,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let photo = [200u8, 100, 50, 255];
        queue.write_texture(
            frame.as_image_copy(),
            &photo.repeat((SIZE * SIZE) as usize),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * SIZE),
                rows_per_image: None,
            },
            frame.size(),
        );
        // The left half photographed at half light, the right half green.
        let middle = (PHOTO_SCALE / 2.0) as u16;
        let (half, green) = ([128, 128, 128, 255], [0, 255, 0, 255]);
        let mut corners = Vec::new();
        for (at, colour, place) in [
            ([-1.0, -1.0], half, [0, 0]),
            ([0.0, -1.0], half, [middle, 0]),
            ([0.0, 1.0], half, [middle, middle]),
            ([-1.0, 1.0], half, [0, middle]),
            ([0.0, -1.0], green, NO_PHOTO),
            ([1.0, -1.0], green, NO_PHOTO),
            ([1.0, 1.0], green, NO_PHOTO),
            ([0.0, 1.0], green, NO_PHOTO),
        ] {
            corners.extend(corner(at, colour, place));
        }
        let triangles: Vec<u8> = [0u32, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let buffer = |bytes: &[u8], usage| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes.len() as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, bytes);
            buffer
        };
        let pipeline = pipeline(&device);
        let sampler = device.create_sampler(&Default::default());
        let frame_view = frame.create_view(&Default::default());
        let photograph = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&frame_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let samples = texture(COLOUR_FORMAT, SAMPLES, attachment).create_view(&Default::default());
        let depth = texture(DEPTH_FORMAT, SAMPLES, attachment).create_view(&Default::default());
        let shown = texture(COLOUR_FORMAT, 1, attachment | wgpu::TextureUsages::COPY_SRC);
        let pass = BuildingPass {
            pipeline,
            photograph,
            corners: (
                buffer(&corners, wgpu::BufferUsages::VERTEX),
                corners.len() as u64,
            ),
            triangles: (
                buffer(&triangles, wgpu::BufferUsages::INDEX),
                triangles.len() as u64,
            ),
            count: 12,
            samples,
            depth,
            shown: shown.create_view(&Default::default()),
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [SIZE; 2],
            pixels_per_point: 1.0,
        };
        let mut resources = egui_wgpu::CallbackResources::default();
        egui_wgpu::CallbackTrait::prepare(
            &pass,
            &device,
            &queue,
            &screen,
            &mut encoder,
            &mut resources,
        );
        // Rows are copied in whole multiples of 256 bytes.
        let row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            shown.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            shown.size(),
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let pixels = read.slice(..).get_mapped_range().unwrap().to_vec();
        let pixel = |x: u32, y: u32| {
            let at = (y * row + 4 * x) as usize;
            [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
        };
        let left = pixel(1, 4);
        for (channel, expected) in [100, 50, 25].into_iter().enumerate() {
            assert!(left[channel].abs_diff(expected) <= 2, "{left:?}");
        }
        assert_eq!(left[3], 255);
        assert_eq!(pixel(6, 4), green);
    }
}
