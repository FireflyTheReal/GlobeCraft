//! The relief march on the GPU (roadmap D1, steps 1 to 3). Relief frames cost four
//! to seven flat ones on the CPU: the rays' walk down the elevation tiles' block
//! maxima before they meet the ground, and as much again from there toward the sun
//! (`terrain_views`, 2026-11-22). The GPU walks each pixel's ray as far as the maxima
//! let it (`shaders/relief.wgsl`) and the CPU's exact march starts there
//! (`render_row`), in place of the 8×8-pixel cones (`terrain::beam`): the picture is
//! the CPU's. The GPU goes on to the ground and toward the sun as
//! `terrain::sun_light` does; where the CPU meets the ground at the same place it
//! takes the share of the sun the GPU saw there. Where the GPU's walk went under the
//! maxima for at most a texel before the ground, the CPU takes that ground as its own
//! and only shades it (`terrain::met_at`).
//!
//! The GPU holds the elevation tiles the last relief frame held (the view moves a
//! little between frames): their heights, and their maxima over blocks of two texels
//! and coarser, widened by a block. Tiles still growing in are left out: their heights
//! are between levels. Whether the walk pays is the hardware's to say
//! (`backend::ReliefTiming`).
use super::*;
use crate::terrain;
use globecraft_core::{EARTH_RADIUS_M, VERTICAL_FOV_HALF_TAN};
use globecraft_data::tiles::{Elevation, Lookup, Provider, unkey};

/// Texels per side of a tile's finest maxima (blocks of two of its 512 texels).
const TOPS_EDGE: u32 = 256;
/// Its levels: blocks of 2 to 512 texels.
const TOPS_MIPS: u32 = 9;
/// Tiles held at most (about 110 MiB with their heights).
const MAX_TOPS_LAYERS: u32 = 128;
const TABLE_SIZE: usize = 512;
/// Steps per ray, as the CPU's (`MAX_MARCH_STEPS`): open air and valleys take a few
/// dozen, grazing rays over the last texels more.
const MAX_STEPS: u32 = 512;
/// Heights are taken this much higher (metres): the f32 positions are within half a
/// metre, a tile's edge may be read from its neighbour.
const HEIGHT_MARGIN_M: f64 = 10.0;
const WORKGROUP: u32 = 64;
/// Heights are held in quarter metres (to 16 km).
const HEIGHT_STEPS_PER_M: f32 = 4.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct View {
    camera: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    q: [f32; 4],
    axes: [f32; 4],
    march: [f32; 4],
    raster: [u32; 4],
    sun: [f32; 4],
    shade: [f32; 4],
}

pub(super) struct ReliefMarch {
    pipeline: wgpu::ComputePipeline,
    uniform: wgpu::Buffer,
    table: wgpu::Buffer,
    output: wgpu::Buffer,
    staging: wgpu::Buffer,
    pixels: usize,
    tops: wgpu::Texture,
    tops_view: wgpu::TextureView,
    /// The tiles' heights, in quarter metres.
    heights: wgpu::Texture,
    heights_view: wgpu::TextureView,
    slots: Vec<Option<Slot>>,
}

fn source() -> String {
    let mut source = include_str!("shaders/relief.wgsl").to_string();
    for (name, value) in [
        ("FOV", VERTICAL_FOV_HALF_TAN),
        ("RADIUS", EARTH_RADIUS_M),
        (
            "MERCATOR_SINE",
            crate::tuning::WEB_MERCATOR_MAX_LATITUDE_DEG
                .to_radians()
                .sin(),
        ),
        ("DEM_TEXELS", terrain::DEM_TILE_TEXELS),
        ("PIXELS_PER_TEXEL", terrain::DEM_PIXELS_PER_TEXEL),
        ("MIN_LATITUDE_COSINE", terrain::MIN_LATITUDE_COSINE),
        ("BLEND_LEVELS", terrain::HEIGHT_BLEND_LEVELS),
        ("SUN_RADIUS", terrain::SUN_RADIUS_RAD),
        ("SHADOW_SOFT", terrain::SHADOW_SOFT_TEXEL_SHARE),
        ("SHADOW_LIFT_M", terrain::SHADOW_LIFT_M),
        ("SHADOW_LIFT_TEXELS", terrain::SHADOW_LIFT_TEXELS),
        ("SHADOW_COARSER", terrain::SHADOW_DETAIL_COARSER),
        ("SUN_RISE_START", crate::tuning::SUN_RISE_START_COSINE),
    ] {
        source = source.replace(&format!("__{name}__"), &format!("{:?}", value as f32));
    }
    source.replace("__MAX_ZOOM__", &terrain::DEM_MAX_ZOOM.to_string())
}

impl ReliefMarch {
    pub(super) fn new(device: &wgpu::Device, layers: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GlobeCraft relief march"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("GlobeCraft relief march"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let layers = layers.min(MAX_TOPS_LAYERS);
        let tops = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GlobeCraft elevation maxima"),
            size: wgpu::Extent3d {
                width: TOPS_EDGE,
                height: TOPS_EDGE,
                depth_or_array_layers: layers,
            },
            mip_level_count: TOPS_MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let tops_view = tops.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let heights = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GlobeCraft elevation heights"),
            size: wgpu::Extent3d {
                width: 2 * TOPS_EDGE,
                height: 2 * TOPS_EDGE,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let heights_view = heights.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Self {
            pipeline,
            uniform: overview::buffer(
                device,
                "GlobeCraft relief view",
                std::mem::size_of::<View>(),
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            table: overview::buffer(device, "GlobeCraft relief tiles", TABLE_SIZE * 16, storage),
            output: overview::buffer(device, "GlobeCraft relief starts", 16, storage),
            staging: overview::buffer(
                device,
                "GlobeCraft relief readback",
                4,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            pixels: 1,
            tops,
            tops_view,
            heights,
            heights_view,
            slots: (0..layers).map(|_| None).collect(),
        }
    }

    /// Per pixel (row by row): where its relief march may start, then where the GPU
    /// met the ground (-1: it did not), then the share of the sun seen there (-1: not
    /// known), then whether the CPU may take that ground as its own (1), walking the elevation tiles `tiles` (the ones the last relief frame
    /// held).
    pub(super) fn starts(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: &RenderRequest,
        tiles: &[(TileKey, Arc<Tile>)],
    ) -> Result<Vec<f32>, String> {
        let Some(relief) = request.cfg.terrain else {
            return Err("No relief in this view".into());
        };
        let slots = self.hold(queue, tiles);
        let count = request.width * request.height;
        if count > self.pixels {
            self.pixels = count;
            let storage = wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC;
            self.output = overview::buffer(device, "GlobeCraft relief starts", count * 16, storage);
            self.staging = overview::buffer(
                device,
                "GlobeCraft relief readback",
                count * 16,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            );
        }
        let rotation = request.basis.rotation_from_view();
        let v4 = |vector: glam::DVec3, w: f64| {
            [vector.x as f32, vector.y as f32, vector.z as f32, w as f32]
        };
        let geometry = request.basis.model.geometry();
        let to_world = relief.exaggeration() / EARTH_RADIUS_M;
        let view = View {
            camera: v4(request.basis.c, request.basis.aspect_ratio()),
            right: v4(rotation.x_axis, 0.0),
            up: v4(rotation.y_axis, 0.0),
            forward: v4(rotation.z_axis, 0.0),
            q: v4(geometry.inverse_axes_squared(), 0.0),
            axes: v4(geometry.axes(), terrain::MAX_TERRAIN_M * to_world),
            march: [
                to_world as f32,
                crate::pixel_angle(request.height, &request.cfg) as f32,
                (HEIGHT_MARGIN_M / EARTH_RADIUS_M) as f32,
                if relief.smooth() { 1.0 } else { 0.0 },
            ],
            raster: [
                request.width as u32,
                request.height as u32,
                (TABLE_SIZE - 1) as u32,
                MAX_STEPS,
            ],
            sun: match request.cfg.sun {
                Some(sun) => v4(sun, 1.0),
                None => [0.0; 4],
            },
            shade: [0.0, relief.fade, 0.0, 0.0],
        };
        let mut table = vec![[u32::MAX; 4]; TABLE_SIZE];
        for (key, layer) in slots {
            let (_, z, x, y, _) = unkey(key);
            let mut index = tile_hash(u32::from(z), x, y);
            while table[index][0] != u32::MAX {
                index = (index + 1) & (TABLE_SIZE - 1);
            }
            table[index] = [u32::from(z), x, y, layer];
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&view));
        queue.write_buffer(&self.table, 0, bytemuck::cast_slice(&table));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GlobeCraft relief march"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(&self.output, (count * 16) as u64),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.tops_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.table.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.heights_view),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GlobeCraft relief march"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("GlobeCraft relief march"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((count as u32).div_ceil(WORKGROUP), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.staging, 0, (count * 16) as u64);
        queue.submit(Some(encoder.finish()));
        let slice = self.staging.slice(..(count * 16) as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(overview::FRAME_WAIT_LIMIT),
            })
            .map_err(|error| format!("GPU relief march not finished: {error}"))?;
        rx.recv()
            .map_err(|error| error.to_string())
            .and_then(|result| result.map_err(|error| error.to_string()))?;
        let starts = {
            let mapped = slice
                .get_mapped_range()
                .map_err(|error| error.to_string())?;
            bytemuck::cast_slice::<u8, f32>(&mapped).to_vec()
        };
        self.staging.unmap();
        Ok(starts)
    }

    /// Holds `tiles` (fully grown ones, as many as fit, coarse first), keeping those
    /// already held; where each sits.
    fn hold(&mut self, queue: &wgpu::Queue, tiles: &[(TileKey, Arc<Tile>)]) -> Vec<(TileKey, u32)> {
        let mut wanted: Vec<_> = tiles
            .iter()
            .filter(|(_, tile)| {
                tile.elevation()
                    .is_some_and(|elevation| elevation.size() == 2 * TOPS_EDGE as usize)
            })
            .collect();
        wanted.sort_by_key(|(key, _)| unkey(*key).1);
        wanted.truncate(self.slots.len());
        let keys: std::collections::HashSet<_> = wanted.iter().map(|(key, _)| *key).collect();
        let mut held = Vec::with_capacity(wanted.len());
        for (key, tile) in wanted {
            let current = self.slots.iter().position(|slot| {
                slot.as_ref().is_some_and(|slot| {
                    slot.key == *key
                        && slot
                            .source
                            .upgrade()
                            .is_some_and(|source| Arc::ptr_eq(&source, tile))
                })
            });
            let layer = match current {
                Some(layer) => layer,
                None => {
                    let Some(layer) = self.slots.iter().position(|slot| {
                        slot.as_ref().is_none_or(|slot| {
                            !keys.contains(&slot.key) || slot.source.strong_count() == 0
                        })
                    }) else {
                        continue;
                    };
                    let Some(elevation) = tile.elevation() else {
                        continue;
                    };
                    self.upload(queue, layer as u32, elevation);
                    self.slots[layer] = Some(Slot {
                        key: *key,
                        source: Arc::downgrade(tile),
                    });
                    layer
                }
            };
            held.push((*key, layer as u32));
        }
        held
    }

    fn upload(&self, queue: &wgpu::Queue, layer: u32, elevation: &Elevation) {
        let side = 2 * TOPS_EDGE;
        let quarters: Vec<u16> = (0..side * side)
            .map(|i| {
                let (x, y) = ((i % side) as usize, (i / side) as usize);
                (elevation.height_m(x as f64 + 0.5, y as f64 + 0.5) * HEIGHT_STEPS_PER_M)
                    .round()
                    .clamp(0.0, f32::from(u16::MAX)) as u16
            })
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.heights,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&quarters),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 2),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
        for (mip, tops) in tops(elevation).iter().enumerate() {
            let side = TOPS_EDGE >> mip;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.tops,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(tops),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 4),
                    rows_per_image: Some(side),
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

/// A tile's maxima as the GPU holds them: over blocks of two texels, each widened by
/// its neighbouring blocks, then over blocks twice as wide, down to the whole tile.
fn tops(elevation: &Elevation) -> Vec<Vec<f32>> {
    let side = TOPS_EDGE as usize;
    let finest = |x: isize, y: isize| {
        let clamp = |c: isize| c.clamp(0, side as isize - 1) as usize;
        elevation.block_max_m(1, clamp(x), clamp(y))
    };
    let mut level: Vec<f32> = (0..side * side)
        .map(|i| {
            let (x, y) = ((i % side) as isize, (i / side) as isize);
            let mut top = f32::MIN;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    top = top.max(finest(x + dx, y + dy));
                }
            }
            top
        })
        .collect();
    let mut levels = Vec::with_capacity(TOPS_MIPS as usize);
    let mut side = side;
    loop {
        let half = side / 2;
        let next: Vec<f32> = (0..half * half)
            .map(|i| {
                let (x, y) = (2 * (i % half), 2 * (i / half));
                level[y * side + x]
                    .max(level[y * side + x + 1])
                    .max(level[(y + 1) * side + x])
                    .max(level[(y + 1) * side + x + 1])
            })
            .collect();
        levels.push(level);
        if half == 0 {
            break;
        }
        (level, side) = (next, half);
    }
    levels
}

impl ReliefMarch {
    /// Renders a small tilted view of made-up mountains on the CPU with and without
    /// this adapter's starts, and refuses the relief march when the pictures differ
    /// beyond the overview's bounds: a driver whose walk ends inside the ground would
    /// show what lies behind it. Run once per device, before its first relief frame.
    pub(super) fn self_check(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(), String> {
        const WIDTH: usize = 120;
        const HEIGHT: usize = 80;
        let cam = globecraft_core::camera::Cam {
            lat: 30.3,
            lon: 79.8,
            heading: 30.0,
            alt: 12.0,
            tilt: 72.0,
            look_up: 0.0,
        };
        let request = RenderRequest {
            width: WIDTH,
            height: HEIGHT,
            cam,
            basis: cam.basis_for_model(WIDTH as f64 / HEIGHT as f64, Default::default()),
            cfg: crate::RenderCfg {
                sky: None,
                provider: Provider::Satellite,
                terrain: Some(crate::TerrainCfg::new(1.0, 1.0)),
                rel: 0,
                grid: false,
                grid_step: 10.0,
                atmo: true,
                sun: Some(globecraft_core::math::ll_to_v3(20.0, 45.0)),
                lod_scale: 1.0,
                level_blend: true,
                fixed_level: None,
                night_lights: None,
                stars: None,
                sky_behind: false,
                clouds: None,
                glint: None,
                aurora: None,
            },
            overlays: None,
            aircraft: None,
            live_layers: None,
            presentation: 0,
            compare: None,
        };
        let made: Mutex<HashMap<TileKey, Arc<Tile>>> = Mutex::new(HashMap::new());
        let lookup = |key: TileKey| {
            let (provider, z, x, y, _) = unkey(key);
            if provider != Provider::TerrainDem {
                return Lookup::Unavailable;
            }
            let mut made = made
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Lookup::Ready(Arc::clone(
                made.entry(key)
                    .or_insert_with(|| Arc::new(made_up_mountains(z, x, y))),
            ))
        };
        let render = |starts: Option<&[f32]>| {
            crate::render_pixels_with(
                WIDTH,
                HEIGHT,
                0..HEIGHT,
                &request.basis,
                &request.cfg,
                &lookup,
                Vec::new(),
                starts,
            )
        };
        let held = render(None).relief_tiles;
        let tiles = grown_tiles(&held, lookup, std::time::Instant::now());
        let starts = self.starts(device, queue, &request, &tiles)?;
        let hits = crate::relief_hits(WIDTH, HEIGHT, &request.basis, &request.cfg, &lookup);
        let past = past_the_ground(&starts[..WIDTH * HEIGHT], &hits)
            + trusted_past_the_ground(&starts, &hits);
        if past > 0 {
            return Err(format!(
                "GPU relief self-check: {past} of {} rays handed over past the ground",
                starts.len()
            ));
        }
        Ok(())
    }
}

/// The CPU's own steps between exact measurements put the ground up to a texel from
/// where the GPU sees it: a start may lie this share of the way past the hit.
pub(super) const PAST_THE_GROUND_SHARE: f64 = 0.02;

/// How many of the GPU's grounds the CPU may take as its own (`trusted`, from
/// `ReliefMarch::starts`) lie past the ground its march meets (`hits`): a ridge the
/// walk passed under the maxima.
pub(super) fn trusted_past_the_ground(gpu: &[f32], hits: &[(f64, f32)]) -> usize {
    let count = hits.len();
    hits.iter()
        .enumerate()
        .filter(|&(i, &(hit, _))| {
            gpu[3 * count + i] > 0.5
                && f64::from(gpu[count + i]) > hit * (1.0 + PAST_THE_GROUND_SHARE)
        })
        .count()
}

/// How many of `starts` lie past the ground the CPU's march meets (`hits`).
pub(super) fn past_the_ground(starts: &[f32], hits: &[(f64, f32)]) -> usize {
    starts
        .iter()
        .zip(hits)
        .filter(|&(&start, &(hit, _))| f64::from(start) > hit * (1.0 + PAST_THE_GROUND_SHARE))
        .count()
}

impl ReliefMarch {}

/// Ridges about 10 km apart up to 3,000 m, the same at every level.
fn made_up_mountains(z: u8, x: u32, y: u32) -> Tile {
    let size = 2 * TOPS_EDGE as usize;
    let n = f64::from(1u32 << z);
    let heights = (0..size * size)
        .map(|i| {
            let u = (f64::from(x) + ((i % size) as f64 + 0.5) / size as f64) / n;
            let v = (f64::from(y) + ((i / size) as f64 + 0.5) / size as f64) / n;
            let ridge = |c: f64| 0.5 + 0.5 * (c * std::f64::consts::TAU * 4000.0).sin();
            (3000.0 * ridge(u) * ridge(v + 0.3 * u)) as f32
        })
        .collect();
    Tile::from_elevation(Elevation::from_heights(size, heights))
}

fn tile_hash(z: u32, x: u32, y: u32) -> usize {
    (z.wrapping_mul(73856093) ^ x.wrapping_mul(19349663) ^ y.wrapping_mul(83492791)) as usize
        & (TABLE_SIZE - 1)
}

/// The elevation tiles among `keys` that are loaded and fully grown, as `lookup` has
/// them.
pub(super) fn grown_tiles(
    keys: &[TileKey],
    lookup: impl Fn(TileKey) -> Lookup,
    now: std::time::Instant,
) -> Vec<(TileKey, Arc<Tile>)> {
    keys.iter()
        .filter(|key| unkey(**key).0 == Provider::TerrainDem)
        .filter_map(|&key| match lookup(key) {
            Lookup::Ready(tile) if tile.grown(now) >= 1.0 => Some((key, tile)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(&source()).expect("relief shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("relief shader validates");
    }

    /// The GPU's maxima lie over every height of the tile within a block of two
    /// texels around each block.
    #[test]
    fn the_maxima_lie_over_the_heights_around_each_block() {
        let size = 2 * TOPS_EDGE as usize;
        let heights: Vec<f32> = (0..size * size)
            .map(|i| ((i % size) as f32 * 0.37).sin() * 900.0 + (i / size) as f32)
            .collect();
        let elevation = Elevation::from_heights(size, heights);
        let tops = tops(&elevation);
        assert_eq!(tops.len(), TOPS_MIPS as usize);
        assert_eq!(tops.last().unwrap().len(), 1);
        for (mip, level) in tops.iter().enumerate() {
            let side = TOPS_EDGE as usize >> mip;
            let block = 2usize << mip;
            for by in (0..side).step_by(7) {
                for bx in (0..side).step_by(5) {
                    let reach =
                        |b: usize| (b * block).saturating_sub(2)..((b + 1) * block + 2).min(size);
                    for y in reach(by) {
                        for x in reach(bx) {
                            let height = elevation.height_m(x as f64 + 0.5, y as f64 + 0.5);
                            assert!(level[by * side + bx] >= height, "{mip} {bx} {by}");
                        }
                    }
                }
            }
        }
    }
}
