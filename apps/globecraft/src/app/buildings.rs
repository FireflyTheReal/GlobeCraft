//! Buildings as blocks close up (roadmap 8.6); the geometry is projected by the
//! live-layer worker (`globecraft_render::live_layers::BuildingMesh`). Where the
//! interface is presented by wgpu the blocks are drawn into their own picture with a
//! depth buffer, so a block behind never shows through one in front, and roofs on an
//! imagery basemap are drawn as the imagery tiles under them (D3), kept in an atlas
//! on the GPU; the OpenGL fallback draws the faces far to near in their colours,
//! which big roofs can get wrong.
use eframe::egui;
use globecraft_data::buildings::BuildingSnapshot;
use globecraft_data::tiles::TileKey;
use globecraft_render::live_layers::{BuildingMesh, LiveView, ROOF_TILE_PX, RoofImagery};
use std::collections::HashMap;
use std::sync::Arc;

/// Samples per pixel of the blocks' picture: smooth edges.
const SAMPLES: u32 = 4;
/// Not sRGB: egui takes a texture's values as the gamma bytes it draws with (an
/// sRGB picture came out dark, 2026-11-16). The roofs' tiles are read as the bytes
/// they hold, as the globe's are.
const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// A vertex in the buffer: clip position (two `f32`), nearness (`f32`), colour, the
/// place in the roof's block (two `u16`), the block (`u32`).
const VERTEX_BYTES: u64 = 24;
/// Roof tiles held on the GPU at once: 64 MiB of 256-pixel tiles.
const ROOF_TILES: u32 = 256;
/// A block's cell with no tile in the atlas (`buildings_depth.wgsl`).
const NO_TILE: u32 = u32::MAX;
/// A block in the records the shader reads: its first tile and its four cells.
const BLOCK_BYTES: usize = 24;

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
    corners: KeptBuffer,
    triangles: KeptBuffer,
    blocks: KeptBuffer,
    pipeline: wgpu::RenderPipeline,
    atlas: RoofAtlas,
    sampler: wgpu::Sampler,
    size: [u32; 2],
    samples: wgpu::TextureView,
    depth: wgpu::TextureView,
    shown: wgpu::TextureView,
    id: egui::TextureId,
}

/// Imagery tiles the roofs are drawn from, one per layer, kept from mesh to mesh:
/// each tile is uploaded once while meshes keep drawing it.
struct RoofAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    layers: HashMap<TileKey, u32>,
    /// The mesh that last drew each layer (`stamp`): the layer drawn longest ago
    /// takes the next new tile.
    used: Vec<u64>,
    keys: Vec<Option<TileKey>>,
    stamp: u64,
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
    let blocks = picture.atlas.blocks(&gpu.queue, &mesh.roof_imagery);
    picture
        .blocks
        .write(gpu, &blocks, wgpu::BufferUsages::STORAGE);
    let (Some(corners), Some(triangles), Some(records)) = (
        &picture.corners.buffer,
        &picture.triangles.buffer,
        &picture.blocks.buffer,
    ) else {
        return picture.id;
    };
    let roofs = roof_bindings(
        device,
        &picture.pipeline,
        &picture.atlas.view,
        &picture.sampler,
        records,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("buildings"),
    });
    draw_pass(
        &mut encoder,
        &picture.pipeline,
        &roofs,
        Targets {
            samples: &picture.samples,
            depth: &picture.depth,
            shown: &picture.shown,
        },
        Drawn {
            corners: (corners, vertices.len() as u64),
            triangles: (triangles, mesh.index_bytes().len() as u64),
            count: mesh.indices.len() as u32,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    picture.id
}

/// The roofs' tiles and blocks bound for the pass.
fn roof_bindings(
    device: &wgpu::Device,
    pipeline: &wgpu::RenderPipeline,
    tiles: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    blocks: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("building roofs"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(tiles),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: blocks.as_entire_binding(),
            },
        ],
    })
}

/// Where the blocks are drawn: the samples, their depth, the picture shown.
struct Targets<'a> {
    samples: &'a wgpu::TextureView,
    depth: &'a wgpu::TextureView,
    shown: &'a wgpu::TextureView,
}

/// What is drawn: the corners and triangles (each buffer with the bytes of it in
/// use) and how many triangle corners.
struct Drawn<'a> {
    corners: (&'a wgpu::Buffer, u64),
    triangles: (&'a wgpu::Buffer, u64),
    count: u32,
}

/// The blocks drawn into their picture.
fn draw_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    roofs: &wgpu::BindGroup,
    targets: Targets<'_>,
    drawn: Drawn<'_>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("buildings"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: targets.samples,
            depth_slice: None,
            resolve_target: Some(targets.shown),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Discard,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: targets.depth,
            // Greater is nearer: nothing is at zero.
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, roofs, &[]);
    pass.set_vertex_buffer(0, drawn.corners.0.slice(..drawn.corners.1));
    pass.set_index_buffer(
        drawn.triangles.0.slice(..drawn.triangles.1),
        wgpu::IndexFormat::Uint32,
    );
    pass.draw_indexed(0..drawn.count, 0, 0..1);
}

impl RoofAtlas {
    fn new(device: &wgpu::Device) -> Self {
        let size = ROOF_TILE_PX as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("roof tiles"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: ROOF_TILES,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOUR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self {
            texture,
            view,
            layers: HashMap::new(),
            used: vec![0; ROOF_TILES as usize],
            keys: vec![None; ROOF_TILES as usize],
            stamp: 0,
        }
    }

    /// The mesh's blocks as the shader reads them (`BLOCK_BYTES` each), with its
    /// tiles in the atlas: those it holds kept, new ones uploaded into the layers
    /// drawn longest ago. A tile there is no room for (more than `ROOF_TILES` in one
    /// mesh) leaves its cells to the roofs' colours.
    fn blocks(&mut self, queue: &wgpu::Queue, roofs: &RoofImagery) -> Vec<u8> {
        self.stamp += 1;
        let stamp = self.stamp;
        for (key, _) in &roofs.tiles {
            if let Some(&layer) = self.layers.get(key) {
                self.used[layer as usize] = stamp;
            }
        }
        for (key, tile) in &roofs.tiles {
            let (width, height, rgba) = tile.rgba();
            if self.layers.contains_key(key) || width != ROOF_TILE_PX || height != ROOF_TILE_PX {
                continue;
            }
            let Some(layer) = (0..ROOF_TILES)
                .filter(|&layer| self.used[layer as usize] < stamp)
                .min_by_key(|&layer| self.used[layer as usize])
            else {
                break;
            };
            if let Some(old) = self.keys[layer as usize].replace(*key) {
                self.layers.remove(&old);
            }
            self.layers.insert(*key, layer);
            self.used[layer as usize] = stamp;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * ROOF_TILE_PX as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: ROOF_TILE_PX as u32,
                    height: ROOF_TILE_PX as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        let mut bytes = Vec::with_capacity(BLOCK_BYTES * roofs.blocks.len().max(1));
        for block in &roofs.blocks {
            bytes.extend(block.x.to_le_bytes());
            bytes.extend(block.y.to_le_bytes());
            for cell in block.cells {
                let entry = cell
                    .and_then(|cell| {
                        let layer = *self.layers.get(&cell.tile)?;
                        Some(layer | (u32::from(cell.coarser) << 16))
                    })
                    .unwrap_or(NO_TILE);
                bytes.extend(entry.to_le_bytes());
            }
        }
        // A binding is never empty: one block of nothing.
        if bytes.is_empty() {
            bytes.resize(BLOCK_BYTES, 0xFF);
        }
        bytes
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
            corners: KeptBuffer::default(),
            triangles: KeptBuffer::default(),
            blocks: KeptBuffer::default(),
            pipeline: pipeline(device),
            atlas: RoofAtlas::new(device),
            sampler: roof_sampler(device),
            size,
            samples: texture("building samples", COLOUR_FORMAT, SAMPLES),
            depth: texture("building depth", DEPTH_FORMAT, SAMPLES),
            shown,
            id,
        }
    }
}

/// Roof tiles filtered between texels; at a tile's edge the texels are held (the
/// next cell's tile is another layer).
fn roof_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("roof tiles"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
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
                    0 => Float32x2, 1 => Float32, 2 => Unorm8x4, 3 => Unorm16x2, 4 => Uint32
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
    use globecraft_render::live_layers::{NO_ROOF, ROOF_AT_SCALE, RoofBlock, RoofCell};

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
    fn corner(at: [f32; 2], colour: [u8; 4], roof_at: [u16; 2], roof: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [at[0], at[1], 0.5] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(colour);
        for value in roof_at {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(roof.to_le_bytes());
        bytes
    }

    /// A tile of one colour.
    fn tile(rgba: [u8; 4]) -> Arc<globecraft_data::tiles::Tile> {
        let size = ROOF_TILE_PX as u32;
        let picture = image::RgbaImage::from_pixel(size, size, image::Rgba(rgba));
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Arc::new(globecraft_data::tiles::Tile::decode(&png).unwrap())
    }

    #[test]
    fn roofs_show_their_tiles_and_other_faces_their_colour() {
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
        // The roof's block: one tile in its first cell, three cells without. The
        // left half of the picture is the roof (in that cell), the right half green.
        let mut atlas = RoofAtlas::new(&device);
        let key = globecraft_data::tiles::make_key(
            globecraft_data::tiles::Provider::Satellite,
            20,
            10,
            10,
            0,
        );
        let roofs = RoofImagery {
            blocks: vec![RoofBlock {
                x: 10,
                y: 10,
                cells: [
                    Some(RoofCell {
                        tile: key,
                        coarser: 0,
                    }),
                    None,
                    None,
                    None,
                ],
            }],
            tiles: vec![(key, tile([200, 100, 50, 255]))],
        };
        let blocks = atlas.blocks(&queue, &roofs);
        let quarter = (ROOF_AT_SCALE / 4.0) as u16;
        let (half, green) = ([128, 128, 128, 255], [0, 255, 0, 255]);
        let mut corners = Vec::new();
        for (at, colour, place, roof) in [
            ([-1.0, -1.0], half, [0, 0], 0),
            ([0.0, -1.0], half, [quarter, 0], 0),
            ([0.0, 1.0], half, [quarter, quarter], 0),
            ([-1.0, 1.0], half, [0, quarter], 0),
            ([0.0, -1.0], green, [0, 0], NO_ROOF),
            ([1.0, -1.0], green, [0, 0], NO_ROOF),
            ([1.0, 1.0], green, [0, 0], NO_ROOF),
            ([0.0, 1.0], green, [0, 0], NO_ROOF),
        ] {
            corners.extend(corner(at, colour, place, roof));
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
        let records = buffer(&blocks, wgpu::BufferUsages::STORAGE);
        let roofs = roof_bindings(
            &device,
            &pipeline,
            &atlas.view,
            &roof_sampler(&device),
            &records,
        );
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let samples = texture(COLOUR_FORMAT, SAMPLES, attachment).create_view(&Default::default());
        let depth = texture(DEPTH_FORMAT, SAMPLES, attachment).create_view(&Default::default());
        let shown = texture(COLOUR_FORMAT, 1, attachment | wgpu::TextureUsages::COPY_SRC);
        let shown_view = shown.create_view(&Default::default());
        let (corner_buffer, triangle_buffer) = (
            buffer(&corners, wgpu::BufferUsages::VERTEX),
            buffer(&triangles, wgpu::BufferUsages::INDEX),
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        draw_pass(
            &mut encoder,
            &pipeline,
            &roofs,
            Targets {
                samples: &samples,
                depth: &depth,
                shown: &shown_view,
            },
            Drawn {
                corners: (&corner_buffer, corners.len() as u64),
                triangles: (&triangle_buffer, triangles.len() as u64),
                count: 12,
            },
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
        // The tile at half light.
        let left = pixel(1, 4);
        for (channel, expected) in [100, 50, 25].into_iter().enumerate() {
            assert!(left[channel].abs_diff(expected) <= 2, "{left:?}");
        }
        assert_eq!(left[3], 255);
        assert_eq!(pixel(6, 4), green);
    }

    #[test]
    fn the_atlas_keeps_tiles_in_use_and_replaces_the_oldest() {
        let Some((device, queue)) = device() else {
            eprintln!("No GPU adapter: skipped");
            return;
        };
        let mut atlas = RoofAtlas::new(&device);
        let key = |x| {
            globecraft_data::tiles::make_key(
                globecraft_data::tiles::Provider::Satellite,
                20,
                x,
                0,
                0,
            )
        };
        let one = tile([1, 2, 3, 255]);
        let meshes = |range: std::ops::Range<u32>| RoofImagery {
            blocks: Vec::new(),
            tiles: range.map(|x| (key(x), one.clone())).collect(),
        };
        // A full atlas, then a mesh of its first tile and a new one: the new tile
        // takes the layer drawn longest ago, never one this mesh draws.
        atlas.blocks(&queue, &meshes(0..ROOF_TILES));
        let mut next = meshes(0..1);
        next.tiles.push((key(ROOF_TILES), one.clone()));
        atlas.blocks(&queue, &next);
        assert_eq!(atlas.layers.get(&key(0)), Some(&0));
        assert_eq!(atlas.layers.len(), ROOF_TILES as usize);
        assert!(atlas.layers.contains_key(&key(ROOF_TILES)));
        // More tiles than layers: those beyond keep no layer, and an empty mesh
        // still binds one block.
        let crowded = atlas.blocks(&queue, &meshes(1000..1000 + ROOF_TILES + 5));
        assert_eq!(crowded.len(), BLOCK_BYTES);
        assert_eq!(atlas.layers.len(), ROOF_TILES as usize);
    }
}
