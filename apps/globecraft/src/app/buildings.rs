//! Buildings as blocks close up (roadmap 8.6); the geometry is projected by the
//! live-layer worker (`globecraft_render::live_layers::BuildingMesh`). Where the
//! interface is presented by wgpu the blocks are drawn into their own picture with a
//! depth buffer, so a block behind never shows through one in front; the OpenGL
//! fallback draws the faces far to near, which big roofs can get wrong.
use eframe::egui;
use globecraft_data::buildings::BuildingSnapshot;
use globecraft_render::live_layers::{BuildingMesh, LiveView};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Samples per pixel of the blocks' picture: smooth edges.
const SAMPLES: u32 = 4;
/// Not sRGB: egui takes a texture's values as the gamma bytes it draws with (an
/// sRGB picture came out dark, 2026-11-16).
const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// A vertex in the buffer: clip position (two `f32`), nearness (`f32`), colour.
const VERTEX_BYTES: u64 = 16;

#[derive(Default)]
pub(super) struct BuildingOverlay {
    /// Overture's building tiles.
    pub(super) tiles: globecraft_data::buildings::BuildingTiles,
    /// The tiles of blocks drawn.
    pub(super) current: Option<Arc<BuildingSnapshot>>,
    /// The interface's GPU, when wgpu presents it.
    pub(super) gpu: Option<egui_wgpu::RenderState>,
    picture: Option<Picture>,
    /// The mesh last drawn, and the shape made from it.
    drawn: Option<(Arc<BuildingMesh>, egui::Shape)>,
}

/// The blocks' own picture on the GPU, shown as an egui texture.
struct Picture {
    pipeline: wgpu::RenderPipeline,
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
        if self
            .drawn
            .as_ref()
            .is_none_or(|(old, _)| !Arc::ptr_eq(old, mesh))
        {
            let shape = match &self.gpu {
                Some(gpu) => {
                    let id = render(gpu, &mut self.picture, mesh, view);
                    let whole = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
                    egui::Shape::image(id, view.rect, whole, egui::Color32::WHITE)
                }
                None => egui::Shape::mesh(egui::Mesh {
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
                }),
            };
            self.drawn = Some((mesh.clone(), shape));
        }
        if let Some((_, shape)) = &self.drawn {
            painter.add(shape.clone());
        }
    }
}

fn premultiplied([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Draws `mesh` into the picture (made, or made again at a new size) and returns
/// the texture showing it.
fn render(
    gpu: &egui_wgpu::RenderState,
    picture: &mut Option<Picture>,
    mesh: &BuildingMesh,
    view: &LiveView,
) -> egui::TextureId {
    let device = &gpu.device;
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

    let mut vertices = Vec::with_capacity(mesh.vertices.len() * VERTEX_BYTES as usize);
    let (min, span) = (view.rect.min, view.rect.size());
    for vertex in &mesh.vertices {
        let clip = [
            (vertex.at.x - min.x) / span.x * 2.0 - 1.0,
            1.0 - (vertex.at.y - min.y) / span.y * 2.0,
            vertex.nearness,
        ];
        vertices.extend(clip.iter().flat_map(|value| value.to_ne_bytes()));
        vertices.extend(premultiplied(vertex.colour).to_array());
    }
    let buffer = |label, contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage,
        })
    };
    let vertices = buffer("building corners", &vertices, wgpu::BufferUsages::VERTEX);
    let indices: Vec<u8> = mesh
        .indices
        .iter()
        .flat_map(|index| index.to_ne_bytes())
        .collect();
    let indices = buffer("building triangles", &indices, wgpu::BufferUsages::INDEX);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("buildings"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("buildings"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &picture.samples,
                depth_slice: None,
                resolve_target: Some(&picture.shown),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &picture.depth,
                // Greater is nearer: nothing is at zero.
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&picture.pipeline);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
    }
    gpu.queue.submit([encoder.finish()]);
    picture.id
}

impl Picture {
    fn new(gpu: &egui_wgpu::RenderState, size: [u32; 2]) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("buildings_depth.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("buildings"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Unorm8x4],
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
        });
        let texture = |label, format, sample_count| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
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
        let shown = texture("buildings", COLOUR_FORMAT, 1);
        let id =
            gpu.renderer
                .write()
                .register_native_texture(device, &shown, wgpu::FilterMode::Linear);
        Self {
            pipeline,
            size,
            samples: texture("building samples", COLOUR_FORMAT, SAMPLES),
            depth: texture("building depth", DEPTH_FORMAT, SAMPLES),
            shown,
            id,
        }
    }
}
