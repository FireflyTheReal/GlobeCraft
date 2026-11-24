//! Full GPU overview ray casting; bounded exact-coordinate tile lookup.
use super::*;
use crate::tuning;
use globecraft_core::{EARTH_RADIUS_M, VERTICAL_FOV_HALF_TAN, earth_model::SurfaceGeometry};
use globecraft_data::tiles::{Lookup, Provider, make_key, unkey};
/// The finest level the GPU draws (`MAP_ZOOM` in the shader, the finest any map has).
/// Past `GLOBAL_MAX_LEVEL`, where an f32 map coordinate is a texel or coarser, a
/// point's place on the map comes from its offset from the camera (`NearMap`).
const MAX_TRACKED_ZOOM: u8 = 20;
pub(super) const MAP_ZOOM: u8 = MAX_TRACKED_ZOOM;
/// The finest level f32 map coordinates hold: an eighth of a texel at 13.
pub(super) const GLOBAL_MAX_LEVEL: u8 = 13;
/// Texels across the map at `MAX_TRACKED_ZOOM` (256-pixel tiles).
const NEAR_SIDE: f64 = 256.0 * (1u64 << MAX_TRACKED_ZOOM) as f64;
const TILE_TABLE_SIZE: usize = 2048;
/// Slots of the set of tiles a frame sampled or missed (`need` in the shader): a few
/// thousand at most even in a wide view; more fall back to the CPU.
const NEEDS_SET_SIZE: usize = 16_384;
/// The zoom the needs' origin is counted at (`need_key`).
const ORIGIN_ZOOM: u32 = MAX_TRACKED_ZOOM as u32;
/// The inside air's entries (`atmosphere::InsideAir`).
const INSIDE_ENTRIES: usize = crate::atmosphere::SCATTERING_VIEWS
    * crate::atmosphere::SCATTERING_PHASES
    + crate::atmosphere::TRANSMITTANCE_ANGLES;
/// Where each part of the air buffer starts (`Overview::air`), in entries.
pub(super) const AIR_SCATTERING_AT: usize =
    crate::atmosphere::TRANSMITTANCE_HEIGHTS * crate::atmosphere::TRANSMITTANCE_ANGLES;
pub(super) const AIR_INSIDE_AT: usize = AIR_SCATTERING_AT
    + crate::atmosphere::SCATTERING_HEIGHTS
        * crate::atmosphere::SCATTERING_VIEWS
        * crate::atmosphere::SCATTERING_SUNS
        * crate::atmosphere::SCATTERING_PHASES;
pub(super) const AERIAL_AT: usize = AIR_INSIDE_AT + INSIDE_ENTRIES;
const AIR_ENTRIES: usize = AERIAL_AT
    + 2 * crate::atmosphere::aerial::VIEWS
        * crate::atmosphere::aerial::AZIMUTHS
        * crate::atmosphere::aerial::DISTANCES;
const MAX_RASTER_PIXELS: usize = 2000 * 2000;
/// How long a frame may take on the GPU before the device counts as failed. Frames
/// take milliseconds; the slowest software adapter measured took 0.15 s. Tests wait
/// longer: the relief walk at 1920×1200 takes the software adapter half a minute.
pub(super) const FRAME_WAIT_LIMIT: std::time::Duration =
    std::time::Duration::from_secs(if cfg!(test) { 120 } else { 5 });
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
    /// Night lights: shown, their finest level, unused, tracked level.
    lights: [u32; 4],
    /// The camera's tile coordinates at `ORIGIN_ZOOM` (`need_key` in the shader).
    origin: [u32; 4],
    /// The camera inside the air (`atmosphere::Frame::inside_air`): its distance from
    /// the centre (km), the sun's cosine there, √(radius² − ground²), and 1 (0: the
    /// camera is above the air).
    air_frame: [f32; 4],
    /// The camera's vertical, and how much of the air low down comes from the
    /// integrated table (`AERIAL_FADE_KM`; 0 without it).
    air_up: [f32; 4],
    /// The eye's adaptation to twilight on the air's light.
    air_adapt: [f32; 4],
    /// The integrated table: its horizon's sine, reach (km) and sun cosine.
    aerial: [f32; 4],
    /// The map near the camera (`relief::NearMap` in `MAX_TRACKED_ZOOM`'s texels): its
    /// texel, the fraction, the range squared and the camera's |c|² − 1 in the
    /// surface's metric (from f64), and the derivatives.
    near: [i32; 4],
    near_frac: [f32; 4],
    near_derivatives: [[f32; 4]; 6],
    near_cubic: [[f32; 4]; 5],
}
pub(super) struct Context<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub atlas: &'a wgpu::TextureView,
    /// The atlas tiles' mean colours (`GpuRenderer::means`).
    pub means: &'a wgpu::Buffer,
}
pub(super) struct Frame {
    pub view: RenderedView,
    pub sampled: Vec<TileKey>,
    /// The pixels the shader left to the CPU (sky, horizon, seams, polar caps, levels
    /// past the near map).
    #[cfg(test)]
    pub repaired: usize,
}
pub(super) struct Overview {
    pipeline: wgpu::ComputePipeline,
    uniform: wgpu::Buffer,
    table: wgpu::Buffer,
    output: wgpu::Buffer,
    needs: wgpu::Buffer,
    /// The air (`air_tables` in the shader): its tables, written once; the air
    /// with the camera inside it (`atmosphere::InsideAir`), written each such frame;
    /// and the aerial table (`atmosphere::aerial`), written when it changes, the table
    /// it holds in `aerial_for`. One buffer keeps the shader within the eight storage
    /// buffers every adapter offers.
    air: wgpu::Buffer,
    /// The water map for the sun's glint (bytes packed four to a word), and the map
    /// it holds.
    water: wgpu::Buffer,
    water_for: Option<std::sync::Arc<image::GrayImage>>,
    /// The aurora forecast (`aurora::glow`), packed as the water map, and the forecast
    /// it holds.
    aurora: wgpu::Buffer,
    aurora_for: Option<std::sync::Arc<globecraft_data::aurora::AuroraGrid>>,
    aerial_for: Option<std::sync::Arc<crate::atmosphere::aerial::Aerial>>,
    staging: wgpu::Buffer,
    pixels: usize,
    dependency_count: usize,
    empty: Vec<u32>,
}
pub(super) fn buffer(
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
/// A read-only table for the shader.
/// The air buffer (`Overview::air`), its tables in place.
fn air(device: &wgpu::Device) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    let tables = &crate::atmosphere::TABLES;
    let mut entries: Vec<[f32; 4]> = tables
        .transmittance_table()
        .iter()
        .chain(tables.scattering_table())
        .map(|entry| entry.to_array())
        .collect();
    assert_eq!(entries.len(), AIR_INSIDE_AT, "the air tables' sizes");
    entries.resize(AIR_ENTRIES, [0.0; 4]);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("GlobeCraft air"),
        contents: bytemuck::cast_slice(&entries),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
            air: air(device),
            water: buffer(device, "GlobeCraft water", 4, wgpu::BufferUsages::STORAGE),
            water_for: None,
            aurora: buffer(device, "GlobeCraft aurora", 4, wgpu::BufferUsages::STORAGE),
            aurora_for: None,
            aerial_for: None,
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
            means,
        } = context;
        let Some(max_zoom) = supported_zoom(request) else {
            return Err(("View requires the precise CPU renderer (close altitude, exact grid, or raster limits)".into(),pixels,false));
        };
        let count = request.width * request.height;
        // The needs' set: keys, their distances, the overflow flag after them.
        let lights_zoom = Provider::NightLights.max_zoom().min(max_zoom);
        let dependencies = 2 * NEEDS_SET_SIZE;
        self.reserve(device, count, dependencies);
        let origin = camera_tile(request.basis.c, request.basis.model.geometry());
        // The water map for the sun's glint, uploaded when it changes. The source is
        // twice as wide as high; the shader counts on it.
        let water = request
            .cfg
            .glint
            .as_ref()
            .filter(|water| request.cfg.sun.is_some() && water.width() == 2 * water.height());
        if let Some(water) = water
            && !self
                .water_for
                .as_ref()
                .is_some_and(|held| std::sync::Arc::ptr_eq(held, water))
        {
            use wgpu::util::DeviceExt;
            let mut bytes = water.as_raw().clone();
            bytes.resize(bytes.len().next_multiple_of(4), 0);
            self.water = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("GlobeCraft water"),
                contents: &bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
            self.water_for = Some(water.clone());
        }
        let aurora = request.cfg.aurora.as_ref();
        if let Some(aurora) = aurora
            && !self
                .aurora_for
                .as_ref()
                .is_some_and(|held| std::sync::Arc::ptr_eq(held, &aurora.grid))
        {
            use wgpu::util::DeviceExt;
            let mut bytes = aurora.grid.values().to_vec();
            bytes.resize(bytes.len().next_multiple_of(4), 0);
            self.aurora = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("GlobeCraft aurora"),
                contents: &bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
            self.aurora_for = Some(aurora.grid.clone());
        }
        // The air with the camera inside it, as the CPU's frame has it.
        let rays = crate::rays::RayCaster::new(request.width, request.height, &request.basis);
        let air = crate::frame_air(&rays, &request.cfg);
        let inside = air.as_ref().and_then(crate::atmosphere::Frame::inside_air);
        if let Some(inside) = &inside {
            let entries: Vec<[f32; 4]> = inside
                .scattering
                .iter()
                .chain(inside.transmittance)
                .map(|entry| entry.to_array())
                .collect();
            debug_assert_eq!(entries.len(), INSIDE_ENTRIES);
            queue.write_buffer(
                &self.air,
                (AIR_INSIDE_AT * 16) as u64,
                bytemuck::cast_slice(&entries),
            );
            if let Some(aerial) = inside.aerial
                && !self
                    .aerial_for
                    .as_ref()
                    .is_some_and(|held| std::sync::Arc::ptr_eq(held, aerial))
            {
                let cells: Vec<[f32; 4]> = aerial
                    .parts()
                    .3
                    .iter()
                    .flat_map(|(light, crossed)| [light.to_array(), crossed.to_array()])
                    .collect();
                assert_eq!(
                    cells.len(),
                    AIR_ENTRIES - AERIAL_AT,
                    "the aerial table's size"
                );
                queue.write_buffer(
                    &self.air,
                    (AERIAL_AT * 16) as u64,
                    bytemuck::cast_slice(&cells),
                );
                self.aerial_for = Some(aerial.clone());
            }
        }
        let rotation = request.basis.rotation_from_view();
        let v4 = |vector: glam::DVec3, w| [vector.x as f32, vector.y as f32, vector.z as f32, w];
        let (shape, figure) = figure(request.basis.model.geometry(), request.basis.c);
        let q = request.basis.model.geometry().inverse_axes_squared();
        let near = super::relief::NearMap::at(request.basis.c, q, NEAR_SIDE, near_range(request));
        let view = View {
            camera: v4(request.basis.c, request.basis.aspect_ratio() as f32),
            right: v4(rotation.x_axis, 0.0),
            up: v4(rotation.y_axis, 0.0),
            forward: v4(rotation.z_axis, 0.0),
            sun: v4(
                request.cfg.sun.unwrap_or_default(),
                if water.is_some() { 1.0 } else { 0.0 },
            ),
            shape,
            // The aurora's opacity rides in the figure's last place (0: off).
            figure: [
                figure[0],
                figure[1],
                crate::atmosphere::eye_weight(
                    ((request.basis.c.length() - 1.0) * globecraft_core::EARTH_RADIUS_KM) as f32,
                ),
                aurora.map_or(0.0, |aurora| aurora.opacity.max(f32::MIN_POSITIVE)),
            ],
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
                // Esri's imagery changes its source between levels 11 and 12.
                u32::from(matches!(
                    request.cfg.provider,
                    Provider::Satellite | Provider::Wayback
                )),
            ],
            limits: [
                max_zoom as u32,
                NEEDS_SET_SIZE as u32,
                (TILE_TABLE_SIZE - 1) as u32,
                water.map_or(0, |water| water.width()),
            ],
            origin: [origin.0, origin.1, 0, 0],
            air_frame: inside.as_ref().map_or([0.0; 4], |inside| {
                [inside.radius_km, inside.sun_cosine, inside.rho, 1.0]
            }),
            air_up: inside.as_ref().map_or([0.0; 4], |inside| {
                let share = inside.aerial.map_or(0.0, |_| {
                    let [from, to] = crate::atmosphere::AERIAL_FADE_KM;
                    let x = ((inside.radius_km - crate::atmosphere::GROUND_KM - from)
                        / (to - from))
                        .clamp(0.0, 1.0);
                    1.0 - x * x * (3.0 - 2.0 * x)
                });
                v4(inside.up, share)
            }),
            air_adapt: inside
                .as_ref()
                .map_or([1.0; 4], |inside| inside.adapt.to_array()),
            aerial: inside
                .as_ref()
                .and_then(|inside| inside.aerial)
                .map_or([0.0; 4], |aerial| {
                    let (horizon, reach, sun_cosine, _) = aerial.parts();
                    [horizon, reach, sun_cosine, 1.0]
                }),
            lights: [
                u32::from(request.cfg.night_lights.is_some()),
                u32::from(Provider::NightLights.max_zoom()),
                0,
                u32::from(lights_zoom),
            ],
            near: [near.whole[0] as i32, near.whole[1] as i32, 0, 0],
            near_frac: [
                near.fraction[0],
                near.fraction[1],
                (near.range * near.range) as f32,
                (request.basis.c.dot(request.basis.c * q) - 1.0) as f32,
            ],
            near_derivatives: near.derivatives,
            near_cubic: near.cubic,
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
            let mut index = tile_hash(level, x, y, TILE_TABLE_SIZE);
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
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.air.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: self.water.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: self.aurora.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: means.as_entire_binding(),
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
            for (&key, &distance) in needs[..NEEDS_SET_SIZE]
                .iter()
                .zip(&needs[NEEDS_SET_SIZE..2 * NEEDS_SET_SIZE])
            {
                if key == u32::MAX {
                    continue;
                }
                let (provider, release) = match request.cfg.night_lights {
                    Some(edition) if key >> 31 == 1 => (Provider::NightLights, edition),
                    _ => (request.cfg.provider, request.cfg.rel),
                };
                let tile = needed_tile(key, origin);
                required.push((
                    make_key(provider, tile.0, tile.1, tile.2, release),
                    f32::from_bits(distance),
                ));
            }
        }
        self.staging.unmap();
        let sampled = required.iter().map(|entry| entry.0).collect();
        let mut required_map: HashMap<_, _> = required
            .into_iter()
            .filter(|(key, _)| !slots.contains_key(key))
            .collect();
        #[cfg(test)]
        let repaired = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 255)
            .count();
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
            #[cfg(test)]
            repaired,
        })
    }
}
/// The camera's tile coordinates at `ORIGIN_ZOOM` (on the map below it, as the
/// shader's Mercator): the origin the needs' keys count from.
fn camera_tile(camera: glam::DVec3, geometry: SurfaceGeometry) -> (u32, u32) {
    use std::f64::consts::PI;
    let n = (camera * geometry.inverse_axes_squared()).normalize();
    let limit = tuning::WEB_MERCATOR_MAX_LATITUDE_DEG.to_radians().sin();
    let s = n.z.clamp(-limit, limit);
    let side = f64::from(1u32 << ORIGIN_ZOOM);
    let u = (n.y.atan2(n.x) / (2.0 * PI) + 0.5).rem_euclid(1.0);
    let v = 0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI);
    let tile = |c: f64| ((c * side).floor() as u32).min((1 << ORIGIN_ZOOM) - 1);
    (tile(u), tile(v))
}

/// The tile (zoom, x, y) a key of the needs' set stands for (`need_key` in the shader),
/// from the camera's tile at `ORIGIN_ZOOM`.
fn needed_tile(key: u32, origin: (u32, u32)) -> (u8, u32, u32) {
    let z = (key >> 26) & 31;
    let signed = |bits: u32| ((bits << 19) as i32) >> 19;
    let (dx, dy) = (signed((key >> 13) & 0x1fff), signed(key & 0x1fff));
    let shift = ORIGIN_ZOOM - z;
    let n = 1i64 << z;
    let x = (i64::from(origin.0 >> shift) + i64::from(dx)).rem_euclid(n);
    let y = i64::from(origin.1 >> shift) + i64::from(dy);
    (z as u8, x as u32, y as u32)
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
/// How far from the camera (world units at the equator) the view may sample a level
/// past `GLOBAL_MAX_LEVEL`, or blend toward one (`lod` in the shader): the shader's
/// level reaches `GLOBAL_MAX_LEVEL` no farther, with a twentieth to spare. Farther
/// pixels of those levels are the CPU's.
fn near_range(request: &RenderRequest) -> f64 {
    near_range_for(lod_height(request), request.basis.model.geometry())
}
fn near_range_for(lod_height: u32, geometry: SurfaceGeometry) -> f64 {
    let pa = 2.0 * VERTICAL_FOV_HALF_TAN / f64::from(lod_height);
    // The largest prime vertical radius, at the poles (a²/b; world units).
    let axes = geometry.axes();
    let widest = axes.max_element() * axes.max_element() / axes.min_element();
    1.05 * 2.0
        * std::f64::consts::PI
        * widest
        * 2f64.powf(tuning::LOD_SHARPNESS_BIAS - f64::from(GLOBAL_MAX_LEVEL))
        / (tuning::TILE_SIZE_PX * pa)
}
pub(super) fn supported_zoom(request: &RenderRequest) -> Option<u8> {
    let count = request.width.checked_mul(request.height)?;
    // The eye's height, as the CPU's air measures it (in the surface's own measure):
    // `cam.alt` is the distance to the point looked at, which a tilt puts far below
    // the eye's height (a view 130 km out at 60° has its eye 65 km up, in the air).
    let c = request.basis.c;
    let eye_km = ((c.dot(c * request.basis.model.geometry().inverse_axes_squared())).sqrt() - 1.0)
        * globecraft_core::EARTH_RADIUS_KM;
    if request.width == 0
        || request.height == 0
        || count > MAX_RASTER_PIXELS
        || !eye_km.is_finite()
        || eye_km <= 0.0
        || request.cfg.grid
    {
        return None;
    }
    // No ground lies nearer than the eye's height.
    let pa = 2.0 * VERTICAL_FOV_HALF_TAN / f64::from(lod_height(request));
    let max_lod = ((2.0 * std::f64::consts::PI * EARTH_RADIUS_M)
        / (tuning::TILE_SIZE_PX * eye_km * globecraft_core::METERS_PER_KM * pa))
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
/// Where tile (z, x, y) starts its search in a table of `size` (a power of two), as the
/// shaders' `tile_hash`.
pub(super) fn tile_hash(z: u32, x: u32, y: u32, size: usize) -> usize {
    (z.wrapping_mul(73856093) ^ x.wrapping_mul(19349663) ^ y.wrapping_mul(83492791)) as usize
        & (size - 1)
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
        // Every adapter offers eight storage buffers to a compute stage (wgpu's
        // default limit, which the device asks for).
        let storage = module
            .global_variables
            .iter()
            .filter(|(_, global)| matches!(global.space, naga::AddressSpace::Storage { .. }))
            .count();
        assert!(storage <= 8, "{storage} storage buffers");
        assert_eq!(std::mem::size_of::<View>(), 464);
        assert_eq!(std::mem::offset_of!(View, lights), 160);
        assert_eq!(std::mem::offset_of!(View, origin), 176);
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

    /// The shader's `need_key`, for the tile `(z, x, y)` seen from the camera's tile
    /// `origin` at `ORIGIN_ZOOM`.
    fn need_key(source: u32, z: u32, (x, y): (u32, u32), origin: (u32, u32)) -> Option<u32> {
        let n = 1u32 << z;
        let camera = (origin.0 >> (ORIGIN_ZOOM - z), origin.1 >> (ORIGIN_ZOOM - z));
        let mut dx = (x.wrapping_sub(camera.0) & (n - 1)) as i32;
        if dx >= (n / 2) as i32 {
            dx -= n as i32;
        }
        let dy = y as i32 - camera.1 as i32;
        (dx.abs() < 4096 && dy.abs() < 4096).then_some(
            (source << 31) | (z << 26) | ((dx as u32 & 0x1fff) << 13) | (dy as u32 & 0x1fff),
        )
    }

    /// The near map's place for a point within its range (as the shader works it out
    /// from the f32 derivatives) stays within a fiftieth of a texel of the exact one
    /// at the finest level a pixel there can take (or blend toward), for the tallest
    /// views, on both Earth models, also across the antimeridian and far north.
    #[test]
    fn the_near_map_holds_the_finest_levels_round_the_camera() {
        use super::super::relief::{NearMap, near_texels};
        use globecraft_core::earth_model::EarthModel;
        for model in [EarthModel::Wgs84, EarthModel::PerfectSphere] {
            let geometry = model.geometry();
            let q = geometry.inverse_axes_squared();
            for lod_height in [400, 1200, 2000] {
                let range = near_range_for(lod_height, geometry);
                let pa = 2.0 * VERTICAL_FOV_HALF_TAN / f64::from(lod_height);
                for (lat, lon, height_km) in [
                    (30.3, 79.8, 5.0),
                    (46.5, 179.99, 0.3),
                    (80.0, -20.0, 9.0),
                    (0.0, 0.0, 40.0),
                ] {
                    let (lat, lon): (f64, f64) = (f64::to_radians(lat), f64::to_radians(lon));
                    let up =
                        glam::DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
                    let surface = up / up.dot(up * q).sqrt();
                    let camera = surface + up * (height_km * 1000.0 / EARTH_RADIUS_M);
                    let near = NearMap::at(camera, q, NEAR_SIDE, range);
                    let lon0 = (camera * q).y.atan2((camera * q).x);
                    let [gx, gy, hx, hx2, hy, hy2] = near.derivatives.map(|v| v.map(f64::from));
                    for i in 0..256 {
                        let a = f64::from(i) * 2.399;
                        let share = (f64::from(i % 16) + 1.0) / 16.0;
                        let d = glam::DVec3::new(a.cos(), a.sin(), (a * 0.37).sin() * 0.5)
                            .normalize()
                            * near.range
                            * share
                            * share;
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
                        let c: Vec<f64> =
                            near.cubic.iter().flatten().map(|&v| f64::from(v)).collect();
                        let monomials = [
                            d.x * d.x * d.x,
                            d.y * d.y * d.y,
                            d.z * d.z * d.z,
                            d.x * d.x * d.y,
                            d.x * d.x * d.z,
                            d.y * d.y * d.x,
                            d.y * d.y * d.z,
                            d.z * d.z * d.x,
                            d.z * d.z * d.y,
                            d.x * d.y * d.z,
                        ];
                        let cubic =
                            |at: usize| (0..10).map(|t| c[at + t] * monomials[t]).sum::<f64>();
                        let shader = shader + glam::DVec2::new(cubic(0), cubic(10));
                        let mut off = shader - near_texels(camera + d, q, lon0, NEAR_SIDE);
                        off.x = (off.x + NEAR_SIDE / 2.0).rem_euclid(NEAR_SIDE) - NEAR_SIDE / 2.0;
                        // The finest level a pixel `d` away takes or blends toward: no
                        // finer than its texel at one pixel (the shader's `lod`).
                        let n = (camera + d) * q;
                        let cos = n
                            .normalize()
                            .truncate()
                            .length()
                            .max(tuning::MIN_LOD_LATITUDE_COSINE);
                        let lod = (2.0 * std::f64::consts::PI * 1.01 * cos
                            / (tuning::TILE_SIZE_PX * d.length() * pa))
                            .log2()
                            + tuning::LOD_SHARPNESS_BIAS;
                        let level = (lod.floor() + 1.0).clamp(0.0, f64::from(MAX_TRACKED_ZOOM));
                        let texel = 2f64.powf(f64::from(MAX_TRACKED_ZOOM) - level);
                        assert!(
                            off.abs().max_element() < 0.02 * texel,
                            "{model:?} {lod_height} {lat} {lon} {height_km} {share}: {off} at level {level}"
                        );
                    }
                }
            }
        }
    }

    /// Every tile within reach of the camera's comes back from its key, round the
    /// antimeridian too, at every zoom; tiles too far off do not fit a key.
    #[test]
    fn the_needs_keys_give_their_tiles_back() {
        for origin in [
            (0, 0),
            (1 << 18, 1 << 18),
            ((1 << ORIGIN_ZOOM) - 1, 1000),
            (5, (1 << ORIGIN_ZOOM) - 1),
        ] {
            for z in 0..=ORIGIN_ZOOM {
                let n = 1u32 << z;
                let camera = (origin.0 >> (ORIGIN_ZOOM - z), origin.1 >> (ORIGIN_ZOOM - z));
                for (ox, oy) in [(0i64, 0i64), (1, -1), (-3, 7), (4000, -4000), (-4095, 4095)] {
                    let x = (i64::from(camera.0) + ox).rem_euclid(i64::from(n)) as u32;
                    let y = i64::from(camera.1) + oy;
                    if !(0..i64::from(n)).contains(&y) {
                        continue;
                    }
                    let y = y as u32;
                    let wraps = i64::from(n) <= 8192;
                    match need_key(1, z, (x, y), origin) {
                        Some(key) => {
                            assert_ne!(key, u32::MAX);
                            assert_eq!(needed_tile(key, origin), (z as u8, x, y), "{z} {ox} {oy}");
                            assert_eq!(key >> 31, 1);
                        }
                        None => assert!(!wraps || oy.abs() >= 4096, "{z} {ox} {oy}"),
                    }
                }
                // Too far from the camera's tile at a fine zoom: no key.
                if z >= 14 {
                    let far = (camera.0.wrapping_add(5000) & (n - 1), camera.1);
                    assert_eq!(need_key(0, z, far, origin), None);
                }
            }
        }
    }
}
