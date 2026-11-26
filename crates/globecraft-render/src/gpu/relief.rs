//! The relief march on the GPU (roadmap D1, steps 1 to 3). Relief frames cost four
//! to seven flat ones on the CPU: the rays' walk down the elevation tiles' block
//! maxima before they meet the ground, and as much again from there toward the sun
//! (`terrain_views`, 2026-11-22). The GPU walks each pixel's ray as far as the maxima
//! let it (`shaders/relief.wgsl`) and the CPU's exact march starts there
//! (`render_row`), in place of the 8×8-pixel cones (`terrain::beam`): the picture is
//! the CPU's. The GPU goes on to the ground and toward the sun as
//! `terrain::sun_light` does; where the CPU meets the ground at the same place it
//! takes the share of the sun the GPU saw there. Every ground the GPU's walk met the CPU
//! takes as its own and only shades (`terrain::met_at`): under the maxima the walk
//! meets the heights exactly in each bilinear cell. On the GPU too a cone as wide as an 8×8
//! block of pixels walks first (`cone` in the shader), and the block's rays start where
//! it ended.
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
/// The passes' workgroups, square blocks of pixels (`main` and `sun`; `sun_rows`' and
/// `sun_shared`'s is `SHARED_SIDE`): a group of threads takes as long as its longest walk, and
/// neighbouring rays walk alike, a row of 32 not as much (2026-11-24).
const WORKGROUP_SIDE: u32 = 8;
/// Pixels across a cone's square block (`CONE_SIDE` in the shader).
const CONE_SIDE: usize = 8;
/// New tiles uploaded per frame at most (0.6 ms each to prepare in this container's
/// release build): a whole tilted view's tiles in one frame (six a frame took a camera
/// low among the relief 7 frames to be walked on an M1, 2026-11-23); after a jump the
/// rest come in the next frames, and the walk stops where one is not held yet.
const MAX_UPLOADS_PER_FRAME: usize = 64;
/// Heights are held in quarter metres (to 16 km).
const HEIGHT_STEPS_PER_M: f32 = 4.0;
/// `sun_rows`' and `sun_shared`'s workgroup side (`SHARED_GROUP` in the shader is its
/// square).
const SHARED_SIDE: u32 = 16;
/// The walk's planes per pixel (`starts` in the shader): start, ground, sun's share,
/// trust, slope east and north.
pub(super) const PLANES: usize = 6;
/// Their bytes, per pixel.
const PLANE_BYTES: usize = PLANES * 4;
/// The planes the CPU reads back (`Walked::pixels`): the first four; the slopes stay
/// on the device for the overview.
pub(super) const READ_PLANES: usize = 4;
const READ_BYTES: usize = READ_PLANES * 4;

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
    near: [u32; 4],
    near_frac: [f32; 4],
    near_derivatives: [[f32; 4]; 6],
}

pub(super) struct ReliefMarch {
    pipeline: wgpu::ComputePipeline,
    /// The sun's share where the view's rays met the ground (passes of their own:
    /// even columns, then odd ones, shared with their neighbours where they can).
    sun: wgpu::ComputePipeline,
    sun_rows: wgpu::ComputePipeline,
    sun_shared: wgpu::ComputePipeline,
    /// One cone a block of pixels first, where their rays' walks start (`cone`).
    cone: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    table: wgpu::Buffer,
    output: wgpu::Buffer,
    staging: wgpu::Buffer,
    tops: wgpu::Texture,
    tops_view: wgpu::TextureView,
    /// The tiles' heights, in quarter metres.
    heights: wgpu::Texture,
    heights_view: wgpu::TextureView,
    /// The most of each texel's 3×3 heights (quarter metres): one read for the
    /// texel's top where nine were.
    peaks: wgpu::Texture,
    peaks_view: wgpu::TextureView,
    /// Per tile held, whether the last walk read it (`read_tiles` in the shader).
    read: wgpu::Buffer,
    slots: Vec<Option<Slot>>,
}

/// What a walk found (`ReliefMarch::starts`).
pub(crate) struct Walked {
    /// Per pixel (row by row): where its relief march may start, then where the GPU
    /// met the ground (-1: it did not), then the share of the sun seen there (-1: not
    /// known), then whether the CPU may take that ground as its own (1; -1: the walk
    /// gave up on that ray without an answer).
    pub(crate) pixels: Vec<f32>,
    /// The tiles the walk read: they stay held for the next frame, though the CPU,
    /// spared the march, no longer reads them itself.
    pub(super) read: Vec<TileKey>,
    /// How many tiles this walk uploaded first.
    pub(super) uploads: usize,
    /// How long each pass took on the device (the cones, the view's walk, the sun's
    /// quarter, other rows and odd columns; where the adapter can time them), and the wait for the device
    /// and the copy of what it found.
    #[cfg(test)]
    pub(super) took: (
        Option<[std::time::Duration; TIMED_PASSES]>,
        std::time::Duration,
        std::time::Duration,
    ),
}

/// How far from the camera (world units, about 10 km, times the latitude's cosine as
/// the levels' own distances) points of the finest levels take their texel from the
/// camera's (`NearMap`).
const NEAR_RANGE: f64 = 1.6e-3;
/// Texels across the map at the zoom the near map counts in (the finest, 15).
const NEAR_SIDE: f64 = 512.0 * 32768.0;

/// The map near the camera for the finest levels, where an f32 map coordinate is a
/// quarter texel or coarser: the camera's texel on a map `side` texels across (whole
/// and fraction) and the first and second derivatives of its texel coordinates in
/// world units there, so a point's texel follows from its f32 offset from the camera.
/// The relief walk counts in zoom-15 elevation texels (`NEAR_SIDE`), the overview in
/// zoom-20 imagery texels (`overview::NEAR_SIDE`).
pub(super) struct NearMap {
    /// How far from the camera the map holds (world units).
    pub(super) range: f64,
    pub(super) whole: [u32; 2],
    pub(super) fraction: [f32; 2],
    /// x's gradient, y's gradient, x's second derivatives (xx, yy, zz, xy; xz, yz, -,
    /// -), y's the same.
    pub(super) derivatives: [[f32; 4]; 6],
    /// The third-order terms' coefficients (`CUBIC_TERMS`, already divided by their
    /// factorials: f_xxx/6, …, f_xxy/2, …, f_xyz), x's ten then y's ten: what keeps a
    /// zoom-20 place within a fiftieth of its level's texel tens of kilometres out
    /// (the overview's range; the relief walk's is short enough without them).
    pub(super) cubic: [[f32; 4]; 5],
}

/// The third-order monomials of an offset, as the axes they multiply:
/// x³, y³, z³, x²y, x²z, y²x, y²z, z²x, z²y, xyz.
const CUBIC_TERMS: [[usize; 3]; 10] = [
    [0, 0, 0],
    [1, 1, 1],
    [2, 2, 2],
    [0, 0, 1],
    [0, 0, 2],
    [1, 1, 0],
    [1, 1, 2],
    [2, 2, 0],
    [2, 2, 1],
    [0, 1, 2],
];

impl NearMap {
    /// `range` is in world units at the equator; it shrinks with the latitude's cosine,
    /// as the levels' own distances do.
    pub(super) fn at(camera: glam::DVec3, q: glam::DVec3, side: f64, range: f64) -> Self {
        let lon = (camera * q).y.atan2((camera * q).x);
        let texels = |d: glam::DVec3| near_texels(camera + d, q, lon, side);
        let centre = texels(glam::DVec3::ZERO);
        let floor = centre.floor();
        // Central differences 64 m apart: far below the map's own curvature.
        let h = 1e-5;
        let axis = |i: usize| glam::DVec3::AXES[i] * h;
        let grad = |i: usize| (texels(axis(i)) - texels(-axis(i))) / (2.0 * h);
        let second = |i: usize, j: usize| {
            if i == j {
                (texels(axis(i)) - 2.0 * centre + texels(-axis(i))) / (h * h)
            } else {
                (texels(axis(i) + axis(j)) - texels(axis(i) - axis(j)) - texels(axis(j) - axis(i))
                    + texels(-axis(i) - axis(j)))
                    / (4.0 * h * h)
            }
        };
        let (g, s) = (
            [grad(0), grad(1), grad(2)],
            [
                second(0, 0),
                second(1, 1),
                second(2, 2),
                second(0, 1),
                second(0, 2),
                second(1, 2),
            ],
        );
        let pick = |c: usize| {
            let part = |v: glam::DVec2| (if c == 0 { v.x } else { v.y }) as f32;
            [
                [part(g[0]), part(g[1]), part(g[2]), 0.0],
                [part(s[0]), part(s[1]), part(s[2]), part(s[3])],
                [part(s[4]), part(s[5]), 0.0, 0.0],
            ]
        };
        let (x, y) = (pick(0), pick(1));
        // Third derivatives as products of central differences (each 2 × 1.3 km wide:
        // f64 texels hold 1e-7, so narrower steps drown in rounding).
        let h3 = 2e-4;
        let third = |[i, j, k]: [usize; 3]| {
            let mut sum = glam::DVec2::ZERO;
            for signs in 0..8 {
                let sign = |bit: usize| if signs >> bit & 1 == 0 { 1.0 } else { -1.0 };
                let d = glam::DVec3::AXES[i] * sign(0)
                    + glam::DVec3::AXES[j] * sign(1)
                    + glam::DVec3::AXES[k] * sign(2);
                sum += texels(d * h3) * (sign(0) * sign(1) * sign(2));
            }
            sum / (8.0 * h3 * h3 * h3)
        };
        let mut coefficients = [0.0f32; 20];
        for (term, axes) in CUBIC_TERMS.iter().enumerate() {
            let [i, j, k] = *axes;
            let factorial = if i == j && j == k {
                6.0
            } else if i == j || j == k {
                2.0
            } else {
                1.0
            };
            let value = third(*axes) / factorial;
            coefficients[term] = value.x as f32;
            coefficients[10 + term] = value.y as f32;
        }
        let cubic =
            std::array::from_fn(|row| std::array::from_fn(|col| coefficients[row * 4 + col]));
        let n = (camera * q).normalize();
        Self {
            range: range * n.x.hypot(n.y).max(terrain::MIN_LATITUDE_COSINE),
            whole: [
                floor.x.rem_euclid(side) as u32,
                floor.y.clamp(0.0, side - 1.0) as u32,
            ],
            fraction: [(centre.x - floor.x) as f32, (centre.y - floor.y) as f32],
            derivatives: [x[0], y[0], x[1], x[2], y[1], y[2]],
            cubic,
        }
    }
}

/// Texel coordinates of the point `p` (world units) on a map `side` texels across
/// (as the shader's `spot`), longitudes counted on from `lon0` without wrapping.
pub(super) fn near_texels(p: glam::DVec3, q: glam::DVec3, lon0: f64, side: f64) -> glam::DVec2 {
    use std::f64::consts::{PI, TAU};
    let n = (p * q).normalize();
    let lon = (n.y.atan2(n.x) - lon0 + PI).rem_euclid(TAU) - PI + lon0;
    let limit = crate::tuning::WEB_MERCATOR_MAX_LATITUDE_DEG
        .to_radians()
        .sin();
    let s = n.z.clamp(-limit, limit);
    glam::DVec2::new(
        lon / TAU + 0.5,
        0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI),
    ) * side
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
        ("MAX_SLOPE", terrain::MAX_SLOPE),
        ("SAME_DEPTH_SHARE", crate::SAME_DEPTH_SHARE),
    ] {
        source = source.replace(&format!("__{name}__"), &format!("{:?}", value as f32));
    }
    let source = source.replace("__MAX_ZOOM__", &terrain::DEM_MAX_ZOOM.to_string());
    #[cfg(test)]
    let source = SHADER_PATCH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .fold(source, |source, (from, to)| {
            assert!(
                source.contains(from.as_str()),
                "shader patch anchor missing: {from}"
            );
            source.replacen(from.as_str(), to, 1)
        });
    source
}

/// Test only: text replacements in the shader before it is compiled (timing what a
/// part of the walk costs, `gpu_relief_walk_timing`'s `GLOBECRAFT_RELIEF_ABLATION`).
#[cfg(test)]
pub(super) static SHADER_PATCH: std::sync::Mutex<Vec<(String, String)>> =
    std::sync::Mutex::new(Vec::new());

impl ReliefMarch {
    pub(super) fn new(device: &wgpu::Device, layers: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GlobeCraft relief march"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        // One layout for every pass: a pass's own would leave out what it does not read
        // (the cones read no heights), and the bind group would match none but one.
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let texture = |sample_type| wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GlobeCraft relief march"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, storage(false)),
                entry(
                    2,
                    texture(wgpu::TextureSampleType::Float { filterable: false }),
                ),
                entry(3, storage(true)),
                entry(4, texture(wgpu::TextureSampleType::Uint)),
                entry(5, storage(false)),
                entry(6, texture(wgpu::TextureSampleType::Uint)),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GlobeCraft relief march"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("GlobeCraft relief march"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (pipeline, sun, sun_rows, sun_shared, cone) = (
            pipeline("main"),
            pipeline("sun"),
            pipeline("sun_rows"),
            pipeline("sun_shared"),
            pipeline("cone"),
        );
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
        let heights_descriptor = wgpu::TextureDescriptor {
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
        };
        let heights = device.create_texture(&heights_descriptor);
        let heights_view = heights.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let peaks = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GlobeCraft elevation texel maxima"),
            ..heights_descriptor
        });
        let peaks_view = peaks.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Self {
            pipeline,
            sun,
            sun_rows,
            sun_shared,
            cone,
            layout,
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
                16 + layers as usize * 4,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            tops,
            tops_view,
            heights,
            heights_view,
            peaks,
            peaks_view,
            read: overview::buffer(
                device,
                "GlobeCraft relief tiles read",
                layers as usize * 4,
                storage | wgpu::BufferUsages::COPY_SRC,
            ),
            slots: (0..layers).map(|_| None).collect(),
        }
    }

    /// Walks each pixel's ray over the elevation tiles `tiles` (the ones the last
    /// relief frame held).
    pub(super) fn starts(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: &RenderRequest,
        tiles: &[(TileKey, Arc<Tile>)],
    ) -> Result<Walked, String> {
        let Some(relief) = request.cfg.terrain else {
            return Err("No relief in this view".into());
        };
        let (slots, uploads) = self.hold(queue, tiles);
        let count = request.width * request.height;
        let layers = self.slots.len();
        // The pixels' planes, then the cones' starts.
        let cones = request.width.div_ceil(CONE_SIDE) * request.height.div_ceil(CONE_SIDE);
        let output_bytes = count * PLANE_BYTES + cones * 4;
        if output_bytes as u64 > self.output.size() {
            let storage = wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC;
            self.output =
                overview::buffer(device, "GlobeCraft relief starts", output_bytes, storage);
        }
        let read_bytes = count * READ_BYTES + layers * 4;
        if read_bytes as u64 > self.staging.size() {
            self.staging = overview::buffer(
                device,
                "GlobeCraft relief readback",
                read_bytes,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            );
        }
        let rotation = request.basis.rotation_from_view();
        let v4 = |vector: glam::DVec3, w: f64| {
            [vector.x as f32, vector.y as f32, vector.z as f32, w as f32]
        };
        let geometry = request.basis.model.geometry();
        let near = NearMap::at(
            request.basis.c,
            geometry.inverse_axes_squared(),
            NEAR_SIDE,
            NEAR_RANGE,
        );
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
            near: [near.whole[0], near.whole[1], 0, 0],
            near_frac: [
                near.fraction[0],
                near.fraction[1],
                (near.range * near.range) as f32,
                0.0,
            ],
            near_derivatives: near.derivatives,
        };
        let mut table = vec![[u32::MAX; 4]; TABLE_SIZE];
        for &(key, layer) in &slots {
            let (_, z, x, y, _) = unkey(key);
            let mut index = overview::tile_hash(u32::from(z), x, y, TABLE_SIZE);
            while table[index][0] != u32::MAX {
                index = (index + 1) & (TABLE_SIZE - 1);
            }
            table[index] = [u32::from(z), x, y, layer];
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&view));
        queue.write_buffer(&self.table, 0, bytemuck::cast_slice(&table));
        queue.write_buffer(&self.read, 0, &vec![0; layers * 4]);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GlobeCraft relief march"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(&self.output, output_bytes as u64),
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
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.read.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.peaks_view),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GlobeCraft relief march"),
        });
        #[cfg(test)]
        let timing = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| Timing::new(device));
        // The cones, the view's pixels, then the sun's quarter walked, its even columns'
        // other rows and its odd columns, in workgroups across and down (one axis held
        // at most 65,535 workgroups).
        let (width, height) = (request.width as u32, request.height as u32);
        let cone_side = CONE_SIDE as u32;
        for (index, (pipeline, (across, down, side))) in [
            (
                &self.cone,
                (
                    width.div_ceil(cone_side),
                    height.div_ceil(cone_side),
                    WORKGROUP_SIDE,
                ),
            ),
            (&self.pipeline, (width, height, WORKGROUP_SIDE)),
            (
                &self.sun,
                (width.div_ceil(2), height.div_ceil(2), WORKGROUP_SIDE),
            ),
            (&self.sun_rows, (width, height / 2, SHARED_SIDE)),
            (&self.sun_shared, (width / 2, height, SHARED_SIDE)),
        ]
        .into_iter()
        .enumerate()
        {
            #[cfg(test)]
            let timestamp_writes = timing.as_ref().map(|timing| timing.writes(index));
            #[cfg(not(test))]
            let (timestamp_writes, _) = (None, index);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("GlobeCraft relief march"),
                timestamp_writes,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(across.div_ceil(side), down.div_ceil(side), 1);
        }
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.resolve(&mut encoder);
        }
        encoder.copy_buffer_to_buffer(
            &self.output,
            0,
            &self.staging,
            0,
            (count * READ_BYTES) as u64,
        );
        encoder.copy_buffer_to_buffer(
            &self.read,
            0,
            &self.staging,
            (count * READ_BYTES) as u64,
            (layers * 4) as u64,
        );
        #[cfg(test)]
        let submitted = std::time::Instant::now();
        queue.submit(Some(encoder.finish()));
        let slice = self
            .staging
            .slice(..(count * READ_BYTES + layers * 4) as u64);
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
        #[cfg(test)]
        let (waited, copying) = (submitted.elapsed(), std::time::Instant::now());
        rx.recv()
            .map_err(|error| error.to_string())
            .and_then(|result| result.map_err(|error| error.to_string()))?;
        let (pixels, read) = {
            let mapped = slice
                .get_mapped_range()
                .map_err(|error| error.to_string())?;
            let (pixels, read) = mapped.split_at(count * READ_BYTES);
            (
                bytemuck::cast_slice::<u8, f32>(pixels).to_vec(),
                bytemuck::cast_slice::<u8, u32>(read).to_vec(),
            )
        };
        self.staging.unmap();
        #[cfg(test)]
        let took = (
            timing.map(|timing| timing.read(device, queue)),
            waited,
            copying.elapsed(),
        );
        let read = slots
            .iter()
            .filter(|(_, layer)| read[*layer as usize] != 0)
            .map(|(key, _)| *key)
            .collect();
        Ok(Walked {
            pixels,
            read,
            uploads,
            #[cfg(test)]
            took,
        })
    }

    /// Holds `tiles` (fully grown ones, as many as fit, coarse first), keeping those
    /// already held and uploading at most `MAX_UPLOADS_PER_FRAME` more; where each
    /// sits.
    fn hold(
        &mut self,
        queue: &wgpu::Queue,
        tiles: &[(TileKey, Arc<Tile>)],
    ) -> (Vec<(TileKey, u32)>, usize) {
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
        let mut uploads = 0;
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
                None if uploads == MAX_UPLOADS_PER_FRAME => continue,
                None => {
                    uploads += 1;
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
        (held, uploads)
    }

    fn upload(&self, queue: &wgpu::Queue, layer: u32, elevation: &Elevation) {
        let side = 2 * TOPS_EDGE;
        let (quarters, peaks) = texels(elevation);
        for (texture, texels) in [(&self.heights, &quarters), (&self.peaks, &peaks)] {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(texels),
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
        }
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

/// A tile's heights in quarter metres (clamped to `u16`) and the most of each texel's
/// 3×3 of them (clamped at the tile's edges), as the GPU holds them. Read straight
/// from the samples and maximised a row and then a column at a time: through
/// bilinear lookups and nine reads per texel an upload took 13 ms (2026-11-23).
fn texels(elevation: &Elevation) -> (Vec<u16>, Vec<u16>) {
    let side = elevation.size();
    // `as` rounds half up here and saturates: below sea level 0, above u16's range its
    // top.
    let quarters: Vec<u16> = elevation
        .heights()
        .iter()
        .map(|&metres| (metres * HEIGHT_STEPS_PER_M + 0.5) as u16)
        .collect();
    let peaks = widened(&quarters, side, u16::max);
    (quarters, peaks)
}

/// The most of each cell's 3×3 in the square grid `grid` (`side` across), clamped at
/// the edges: a row and then a column at a time.
fn widened<T: Copy + Default>(grid: &[T], side: usize, max: fn(T, T) -> T) -> Vec<T> {
    let mut across = vec![T::default(); side * side];
    for (row, out) in grid.chunks_exact(side).zip(across.chunks_exact_mut(side)) {
        out[0] = max(row[0], row[1]);
        for x in 1..side - 1 {
            out[x] = max(max(row[x - 1], row[x]), row[x + 1]);
        }
        out[side - 1] = max(row[side - 2], row[side - 1]);
    }
    let mut most = vec![T::default(); side * side];
    let row = |y: usize| &across[y * side..][..side];
    for (y, out) in most.chunks_exact_mut(side).enumerate() {
        let (above, here, below) = (row(y.saturating_sub(1)), row(y), row((y + 1).min(side - 1)));
        for x in 0..side {
            out[x] = max(max(above[x], here[x]), below[x]);
        }
    }
    most
}

/// A tile's maxima as the GPU holds them: over blocks of two texels, each widened by
/// its neighbouring blocks, then over blocks twice as wide, down to the whole tile.
fn tops(elevation: &Elevation) -> Vec<Vec<f32>> {
    let side = TOPS_EDGE as usize;
    let finest: Vec<f32> = (0..side * side)
        .map(|i| elevation.block_max_m(1, i % side, i / side))
        .collect();
    let mut level = widened(&finest, side, f32::max);
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
        let starts = self.starts(device, queue, &request, &tiles)?.pixels;
        let hits =
            |how| crate::relief_hits(WIDTH, HEIGHT, &request.basis, &request.cfg, &lookup, how);
        let past = past_the_ground(&starts[..WIDTH * HEIGHT], &hits(crate::HitMarch::Frame));
        let trusted_past = trusted_past_the_ground(&starts, &hits(crate::HitMarch::Exact));
        if past > 0 || trusted_past * TRUSTED_PAST_PER > WIDTH * HEIGHT {
            return Err(format!(
                "GPU relief self-check: {past} starts and {trusted_past} grounds of {} rays past the ground",
                WIDTH * HEIGHT
            ));
        }
        Ok(())
    }
}

/// The CPU's own steps between exact measurements put the ground up to a texel from
/// where the GPU sees it: a start may lie this share of the way past the hit.
pub(super) const PAST_THE_GROUND_SHARE: f64 = 0.02;

/// At most one ground in this many taken from the GPU may lie past a ridge tip the
/// CPU's march measured at every texel meets (the walk under the maxima meets the
/// heights exactly in each bilinear cell, in f32 and along a straight line per cell).
pub(super) const TRUSTED_PAST_PER: usize = 1_000;

/// How many of the GPU's grounds the CPU may take as its own (`trusted`, from
/// `ReliefMarch::starts`) lie past the ground the exact march meets (`hits`, with
/// `HitMarch::Exact`): a ridge the walk passed under the maxima.
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

/// The walk's passes: the cones, the view, the sun's quarter walked, its even columns'
/// other rows and its odd columns.
#[cfg(test)]
pub(super) const TIMED_PASSES: usize = 5;
/// A start and an end per pass.
#[cfg(test)]
const TIMESTAMP_BYTES: usize = 2 * TIMED_PASSES * 8;
/// The walk's passes timed on the device (tests, where the adapter can).
#[cfg(test)]
struct Timing {
    queries: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    staging: wgpu::Buffer,
}
#[cfg(test)]
impl Timing {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("GlobeCraft relief timing"),
                ty: wgpu::QueryType::Timestamp,
                count: 2 * TIMED_PASSES as u32,
            }),
            resolved: overview::buffer(
                device,
                "GlobeCraft relief timing",
                TIMESTAMP_BYTES,
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            staging: overview::buffer(
                device,
                "GlobeCraft relief timing readback",
                TIMESTAMP_BYTES,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
        }
    }
    fn writes(&self, pass: usize) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(2 * pass as u32),
            end_of_pass_write_index: Some(2 * pass as u32 + 1),
        }
    }
    fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.queries, 0..2 * TIMED_PASSES as u32, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.staging, 0, TIMESTAMP_BYTES as u64);
    }
    fn read(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> [std::time::Duration; TIMED_PASSES] {
        let slice = self.staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(overview::FRAME_WAIT_LIMIT),
            })
            .unwrap();
        let ticks: Vec<u64> = bytemuck::cast_slice(&slice.get_mapped_range().unwrap()).to_vec();
        self.staging.unmap();
        let period = f64::from(queue.get_timestamp_period());
        std::array::from_fn(|pass| {
            let ticks = ticks[2 * pass + 1].saturating_sub(ticks[2 * pass]);
            std::time::Duration::from_nanos((ticks as f64 * period) as u64)
        })
    }
}

impl ReliefMarch {
    /// The last walk's planes on the device (`starts` in the shader), for the overview.
    pub(super) fn output(&self) -> &wgpu::Buffer {
        &self.output
    }
}

/// Ridges about 10 km apart up to 3,000 m, the same at every level.
fn made_up_mountains(z: u8, x: u32, y: u32) -> Tile {
    let size = 2 * TOPS_EDGE as usize;
    let n = f64::from(1u32 << z);
    let heights = (0..size * size)
        .map(|i| {
            let u = (f64::from(x) + ((i % size) as f64 + 0.5) / size as f64) / n;
            let v = (f64::from(y) + ((i / size) as f64 + 0.5) / size as f64) / n;
            // Triangle waves: cheap to make at the first relief frame.
            let ridge = |c: f64| 1.0 - (2.0 * (c * 4000.0).fract() - 1.0).abs();
            (3000.0 * ridge(u) * ridge(v + 0.3 * u)) as f32
        })
        .collect();
    Tile::from_elevation(Elevation::from_heights(size, heights))
}

/// Whether the walk answered at least half the rays (`pixels` as in `Walked`): where
/// it gave up on more (a camera low among the relief starts every ray at levels too
/// fine for f32 map coordinates), the CPU's own cones start the rays further on.
pub(super) fn helps(pixels: &[f32]) -> bool {
    let count = pixels.len() / READ_PLANES;
    pixels[3 * count..4 * count]
        .iter()
        .filter(|&&trusted| trusted < 0.0)
        .count()
        * 2
        <= count
}

/// The elevation tiles to hold for the next frame: the ones the CPU's frame held
/// (`cpu`) and the ones the walk read (`read`).
pub(super) fn next_tiles(mut cpu: Vec<TileKey>, read: &[TileKey]) -> Vec<TileKey> {
    cpu.extend_from_slice(read);
    cpu.sort_unstable();
    cpu.dedup();
    cpu
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

    /// The near map's texel for a point within its range of the camera (as the
    /// shader works it out from the f32 derivatives) stays within a fiftieth of a
    /// zoom-15 texel of the exact one, also across the antimeridian and far north.
    #[test]
    fn the_near_map_finds_the_finest_texels_round_the_camera() {
        let q = globecraft_core::earth_model::EarthModel::Wgs84
            .geometry()
            .inverse_axes_squared();
        for (lat, lon, height_km) in [(30.3, 79.8, 5.0), (46.5, 179.99, 1.0), (80.0, -20.0, 9.0)] {
            let (lat, lon): (f64, f64) = (f64::to_radians(lat), f64::to_radians(lon));
            let up = glam::DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
            let camera = up * (1.0 + height_km * 1000.0 / EARTH_RADIUS_M);
            let near = NearMap::at(camera, q, NEAR_SIDE, NEAR_RANGE);
            let lon0 = (camera * q).y.atan2((camera * q).x);
            for i in 0..64 {
                let a = f64::from(i) * 2.399;
                let d = glam::DVec3::new(a.cos(), a.sin(), (a * 0.37).sin() * 0.5).normalize()
                    * near.range
                    * (f64::from(i % 8) + 1.0)
                    / 8.0;
                let [gx, gy, hx, hx2, hy, hy2] = near.derivatives.map(|v| v.map(f64::from));
                let quad = |h: [f64; 4], h2: [f64; 4]| {
                    0.5 * (h[0] * d.x * d.x + h[1] * d.y * d.y + h[2] * d.z * d.z)
                        + h[3] * d.x * d.y
                        + h2[0] * d.x * d.z
                        + h2[1] * d.y * d.z
                };
                let shader = glam::DVec2::new(
                    f64::from(near.whole[0])
                        + f64::from(near.fraction[0])
                        + gx[0] * d.x
                        + gx[1] * d.y
                        + gx[2] * d.z
                        + quad(hx, hx2),
                    f64::from(near.whole[1])
                        + f64::from(near.fraction[1])
                        + gy[0] * d.x
                        + gy[1] * d.y
                        + gy[2] * d.z
                        + quad(hy, hy2),
                );
                let exact = near_texels(camera + d, q, lon0, NEAR_SIDE);
                let mut off = shader - exact;
                off.x = (off.x + NEAR_SIDE / 2.0).rem_euclid(NEAR_SIDE) - NEAR_SIDE / 2.0;
                assert!(off.abs().max_element() < 0.02, "{lat} {lon} {d}: {off}");
            }
        }
    }

    /// The heights held are the samples in quarter metres, and each texel's top the
    /// most of its 3×3 of them, clamped at the tile's edges.
    #[test]
    fn the_texels_held_are_the_samples_and_their_3x3_tops() {
        let tile = crate::tests::probe_dem_tile(10, 700, 400);
        let elevation = tile.elevation().unwrap();
        let (quarters, peaks) = texels(elevation);
        let side = elevation.size();
        let quarter = |x: isize, y: isize| {
            let clamp = |c: isize| c.clamp(0, side as isize - 1) as usize;
            let metres = elevation.height_m(clamp(x) as f64 + 0.5, clamp(y) as f64 + 0.5);
            (metres * HEIGHT_STEPS_PER_M).round() as u16
        };
        for (i, (&held, &top)) in quarters.iter().zip(&peaks).enumerate() {
            let (x, y) = ((i % side) as isize, (i / side) as isize);
            assert_eq!(held, quarter(x, y));
            let most = (-1..=1)
                .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                .map(|(dx, dy)| quarter(x + dx, y + dy))
                .max()
                .unwrap();
            assert_eq!(top, most, "{x} {y}");
        }
    }

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
