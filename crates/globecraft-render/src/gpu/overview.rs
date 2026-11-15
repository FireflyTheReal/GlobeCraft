//! Full GPU overview ray casting; bounded exact-coordinate tile lookup.
use super::*;
use crate::tuning;
use globecraft_core::{EARTH_RADIUS_M, VERTICAL_FOV_HALF_TAN, earth_model::SurfaceGeometry};
use globecraft_data::tiles::{Lookup, Provider, make_key, unkey};
const MIN_GPU_ALTITUDE_KM: f64 = 1000.0;
const MAX_TRACKED_ZOOM: u8 = 10;
const TILE_TABLE_SIZE: usize = 2048;
const MAX_RASTER_PIXELS: usize = 2000 * 2000;
/// How long a frame may take on the GPU before the device counts as failed. Frames
/// take milliseconds; the slowest software adapter measured took 0.15 s.
const FRAME_WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);
/// Marks the night lights' tiles in the tile table's level word (levels are below 32).
pub(super) const LIGHTS_TAG: u32 = 0x100;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct View {
    camera: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    sun: [f32; 4],
    /// One over the surface's semi-axes squared, and the camera in that metric, squared.
    shape: [f32; 4],
    /// The semi-major axis in metres and the eccentricity squared.
    figure: [f32; 4],
    raster: [u32; 4],
    effects: [u32; 4],
    limits: [u32; 4],
    /// Night lights: shown, their finest level, where their needs start, tracked level.
    lights: [u32; 4],
}
pub(super) struct Context<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub atlas: &'a wgpu::TextureView,
}
pub(super) struct Frame {
    pub view: RenderedView,
    pub sampled: Vec<TileKey>,
}
pub(super) struct Overview {
    pipeline: wgpu::ComputePipeline,
    uniform: wgpu::Buffer,
    table: wgpu::Buffer,
    output: wgpu::Buffer,
    needs: wgpu::Buffer,
    staging: wgpu::Buffer,
    pixels: usize,
    dependency_count: usize,
    empty: Vec<u32>,
}
fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: usize,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage,
        mapped_at_creation: false,
    })
}
impl Overview {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GlobeCraft overview ray casting"),
            source: wgpu::ShaderSource::Wgsl(super::shader::source().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("GlobeCraft overview renderer"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC;
        Self {
            pipeline,
            uniform: buffer(
                device,
                "GlobeCraft camera",
                std::mem::size_of::<View>(),
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            table: buffer(
                device,
                "GlobeCraft tile table",
                TILE_TABLE_SIZE * 16,
                storage,
            ),
            output: buffer(device, "GlobeCraft overview pixels", 4, storage),
            needs: buffer(device, "GlobeCraft overview dependencies", 8, storage),
            staging: buffer(
                device,
                "GlobeCraft overview readback",
                12,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            pixels: 1,
            dependency_count: 1,
            empty: vec![u32::MAX; 2],
        }
    }
    fn reserve(&mut self, device: &wgpu::Device, pixels: usize, dependencies: usize) {
        if pixels <= self.pixels && dependencies <= self.dependency_count {
            return;
        }
        self.pixels = self.pixels.max(pixels);
        self.dependency_count = self.dependency_count.max(dependencies);
        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC;
        self.output = buffer(
            device,
            "GlobeCraft overview pixels",
            self.pixels * 4,
            storage,
        );
        self.needs = buffer(
            device,
            "GlobeCraft overview dependencies",
            (self.dependency_count + 1) * 4,
            storage,
        );
        self.staging = buffer(
            device,
            "GlobeCraft overview readback",
            (self.pixels + self.dependency_count + 1) * 4,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        self.empty.resize(self.dependency_count + 1, u32::MAX);
    }
    pub(super) fn render(
        &mut self,
        context: Context<'_>,
        request: &RenderRequest,
        slots: &HashMap<TileKey, u32>,
        lookup: &(impl Fn(TileKey) -> Lookup + Sync),
        mut pixels: Vec<u8>,
    ) -> Result<Frame, (String, Vec<u8>, bool)> {
        let Context {
            device,
            queue,
            atlas,
        } = context;
        let Some(max_zoom) = supported_zoom(request) else {
            return Err(("View requires the precise CPU renderer (close altitude, exact grid, or raster limits)".into(),pixels,false));
        };
        let count = request.width * request.height;
        // One slot per tile of every tracked level: the base map's, then the night lights'.
        let pyramid = |levels: u8| ((1usize << (2 * (levels as usize + 1))) - 1) / 3;
        let base_dependencies = pyramid(max_zoom);
        let lights_zoom = Provider::NightLights.max_zoom().min(max_zoom);
        let dependencies =
            base_dependencies + request.cfg.night_lights.map_or(0, |_| pyramid(lights_zoom));
        self.reserve(device, count, dependencies);
        let rotation = request.basis.rotation_from_view();
        let v4 = |vector: glam::DVec3, w| [vector.x as f32, vector.y as f32, vector.z as f32, w];
        let (shape, figure) = figure(request.basis.model.geometry(), request.basis.c);
        let view = View {
            camera: v4(request.basis.c, request.basis.aspect_ratio() as f32),
            right: v4(rotation.x_axis, 0.0),
            up: v4(rotation.y_axis, 0.0),
            forward: v4(rotation.z_axis, 0.0),
            sun: v4(request.cfg.sun.unwrap_or_default(), 0.0),
            shape,
            figure,
            raster: [
                request.width as u32,
                request.height as u32,
                u32::from(request.cfg.provider.is_imagery()),
                request.cfg.provider.max_zoom() as u32,
            ],
            effects: [
                u32::from(request.cfg.sun.is_some()),
                u32::from(request.cfg.atmo),
                lod_height(request),
                0,
            ],
            limits: [
                max_zoom as u32,
                dependencies as u32,
                (TILE_TABLE_SIZE - 1) as u32,
                0,
            ],
            lights: [
                u32::from(request.cfg.night_lights.is_some()),
                u32::from(Provider::NightLights.max_zoom()),
                base_dependencies as u32,
                u32::from(lights_zoom),
            ],
        };
        let mut table = vec![[u32::MAX; 4]; TILE_TABLE_SIZE];
        for (&key, &layer) in slots {
            let (provider, z, x, y, _) = unkey(key);
            let level = u32::from(z)
                | if provider == Provider::NightLights {
                    LIGHTS_TAG
                } else {
                    0
                };
            let mut index = tile_hash(level, x, y);
            while table[index][0] != u32::MAX {
                index = (index + 1) & (TILE_TABLE_SIZE - 1);
            }
            table[index] = [level, x, y, layer];
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&view));
        queue.write_buffer(&self.table, 0, bytemuck::cast_slice(&table));
        queue.write_buffer(
            &self.needs,
            0,
            bytemuck::cast_slice(&self.empty[..dependencies + 1]),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GlobeCraft overview view"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(&self.output, (count * 4) as u64),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.table.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: binding(&self.needs, ((dependencies + 1) * 4) as u64),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GlobeCraft GPU overview"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("GlobeCraft overview rays and shading"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((count as u32).div_ceil(WORKGROUP_SIZE), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.staging, 0, (count * 4) as u64);
        encoder.copy_buffer_to_buffer(
            &self.needs,
            0,
            &self.staging,
            (count * 4) as u64,
            ((dependencies + 1) * 4) as u64,
        );
        queue.submit(Some(encoder.finish()));
        let slice = self
            .staging
            .slice(..((count + dependencies + 1) * 4) as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        // Never wait on the driver without a limit: a device that stops answering
        // must not stop the globe (the frame is then drawn on the CPU).
        // shortcut: bounds the wait for the frame only, not a driver call that never
        // returns; move GPU work to its own watched thread if such hangs are reported.
        if let Err(error) = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(FRAME_WAIT_LIMIT),
        }) {
            return Err((
                format!("GPU frame not finished within {FRAME_WAIT_LIMIT:?}: {error}"),
                pixels,
                true,
            ));
        }
        if let Err(error) = rx
            .recv()
            .map_err(|error| error.to_string())
            .and_then(|result| result.map_err(|error| error.to_string()))
        {
            return Err((error, pixels, true));
        }
        let mut required = Vec::new();
        {
            let mapped = match slice.get_mapped_range() {
                Ok(mapped) => mapped,
                Err(error) => {
                    self.staging.unmap();
                    return Err((error.to_string(), pixels, true));
                }
            };
            pixels.resize(count * 4, 0);
            pixels.copy_from_slice(&mapped[..count * 4]);
            let needs: &[u32] = bytemuck::cast_slice(&mapped[count * 4..]);
            if needs[dependencies] != u32::MAX {
                drop(mapped);
                self.staging.unmap();
                return Err((
                    "GPU LOD exceeded its checked dependency bounds".into(),
                    pixels,
                    false,
                ));
            }
            let lights = request.cfg.night_lights.map(|edition| {
                (
                    Provider::NightLights,
                    edition,
                    lights_zoom,
                    base_dependencies,
                )
            });
            for (provider, release, levels, first) in
                std::iter::once((request.cfg.provider, request.cfg.rel, max_zoom, 0)).chain(lights)
            {
                for z in 0..=levels {
                    let start = first + pyramid(z) - (1usize << (2 * z as usize));
                    let n = 1usize << z;
                    for (i, &dist) in needs[start..start + n * n].iter().enumerate() {
                        if dist != u32::MAX {
                            required.push((
                                make_key(provider, z, (i % n) as u32, (i / n) as u32, release),
                                f32::from_bits(dist),
                            ));
                        }
                    }
                }
            }
        }
        self.staging.unmap();
        let sampled = required.iter().map(|entry| entry.0).collect();
        let mut required_map: HashMap<_, _> = required
            .into_iter()
            .filter(|(key, _)| !slots.contains_key(key))
            .collect();
        for (key, distance) in super::repair::pixels(request, &mut pixels, lookup) {
            let distance = distance.unwrap_or(f32::MAX);
            required_map
                .entry(key)
                .and_modify(|previous| *previous = previous.min(distance))
                .or_insert(distance);
        }
        let mut required: Vec<_> = required_map.into_iter().collect();
        required.sort_by(|a, b| unkey(a.0).1.cmp(&unkey(b.0).1).then(a.1.total_cmp(&b.1)));
        let waiting: Vec<_> = required.iter().map(|entry| entry.0).collect();
        let requests = required
            .into_iter()
            .filter(|entry| matches!(lookup(entry.0), Lookup::Absent))
            .map(|entry| entry.0)
            .collect();
        Ok(Frame {
            view: RenderedView {
                pixels,
                waiting,
                requests,
            },
            sampled,
        })
    }
}
/// The surface as the shader takes it (`View::shape`, `View::figure`): a sphere is the
/// ellipsoid whose axes are all one.
fn figure(geometry: SurfaceGeometry, camera: glam::DVec3) -> ([f32; 4], [f32; 4]) {
    let q = geometry.inverse_axes_squared();
    let (semi_major_m, eccentricity_squared) = match geometry {
        SurfaceGeometry::Sphere { radius_km } => (radius_km * globecraft_core::METERS_PER_KM, 0.0),
        SurfaceGeometry::Ellipsoid {
            semi_major_km,
            flattening,
        } => (
            semi_major_km * globecraft_core::METERS_PER_KM,
            flattening * (2.0 - flattening),
        ),
    };
    (
        [
            q.x as f32,
            q.y as f32,
            q.z as f32,
            camera.dot(camera * q) as f32,
        ],
        [semi_major_m as f32, eccentricity_squared as f32, 0.0, 0.0],
    )
}
pub(super) fn supported_zoom(request: &RenderRequest) -> Option<u8> {
    let count = request.width.checked_mul(request.height)?;
    if request.width == 0
        || request.height == 0
        || count > MAX_RASTER_PIXELS
        || request.cam.alt < MIN_GPU_ALTITUDE_KM
        || !request.cam.alt.is_finite()
        || request.cfg.grid
    {
        return None;
    }
    let pa = 2.0 * VERTICAL_FOV_HALF_TAN / f64::from(lod_height(request));
    let max_lod = ((2.0 * std::f64::consts::PI * EARTH_RADIUS_M)
        / (tuning::TILE_SIZE_PX * request.cam.alt * globecraft_core::METERS_PER_KM * pa))
        .log2()
        + tuning::LOD_SHARPNESS_BIAS;
    // Below level 0 the shader clamps to level 0 but keeps the fraction, and blends
    // in level 1 near the top of that band (a whole globe far out on a text map):
    // level 1 is a dependency whenever the view is less than a level below 0.
    // Imagery always blends from level 0 to 1.
    let blends_level_one = request.cfg.provider.is_imagery() || max_lod > -1.0;
    let max_zoom = (max_lod.ceil().max(if blends_level_one { 1.0 } else { 0.0 }) as u8)
        .min(request.cfg.provider.max_zoom());
    (max_zoom <= MAX_TRACKED_ZOOM).then_some(max_zoom)
}
/// The raster height the level of detail follows: the full-quality raster's, not this
/// one's. Motion frames render smaller (`RenderCfg::lod_scale`) but must pick the same
/// imagery level as idle frames, or the view switches mosaics at every motion/idle
/// change (as the CPU renderer did before 2026-10-23).
fn lod_height(request: &RenderRequest) -> u32 {
    let scale = f64::from(request.cfg.lod_scale);
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    ((request.height as f64 / scale).round() as u32).max(1)
}
fn tile_hash(z: u32, x: u32, y: u32) -> usize {
    (z.wrapping_mul(73856093) ^ x.wrapping_mul(19349663) ^ y.wrapping_mul(83492791)) as usize
        & (TILE_TABLE_SIZE - 1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_validates_without_an_adapter_and_uniform_layout_matches() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(&super::super::shader::source())
            .expect("valid embedded WGSL");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("portable shader operations");
        assert_eq!(std::mem::size_of::<View>(), 176);
        assert_eq!(std::mem::offset_of!(View, lights), 160);
        assert_eq!(std::mem::offset_of!(View, raster), 112);
        assert_eq!(std::mem::offset_of!(View, effects), 128);
        assert_eq!(std::mem::offset_of!(View, limits), 144);
        let shader_view = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("View"))
            .unwrap()
            .1;
        let naga::TypeInner::Struct { members, span } = &shader_view.inner else {
            panic!("View must remain a uniform struct");
        };
        assert_eq!(*span as usize, std::mem::size_of::<View>());
        assert_eq!(
            members[7].offset as usize,
            std::mem::offset_of!(View, raster)
        );
    }
}
