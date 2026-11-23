//! GPU sampling and shading of overview frames, without egui dependencies.
mod overview;
mod relief;
mod repair;
mod shader;
#[cfg(test)]
mod tests;
use crate::{RenderedView, worker::RenderRequest};
use globecraft_data::tiles::{Tile, TileCache, TileKey};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

const TILE_EDGE_PX: u32 = 256;
const GPU_TILE_BUDGET_BYTES: u32 = 128 * 1024 * 1024;
const TILE_BYTES: u32 = TILE_EDGE_PX * TILE_EDGE_PX * 4;
const MAX_ATLAS_LAYERS: u32 = GPU_TILE_BUDGET_BYTES / TILE_BYTES;
const GLOBAL_RESIDENT_ZOOM: u8 = 3;
const MAX_RESIDENCY_PASSES: usize = 2;
const WORKGROUP_SIZE: u32 = 256;

struct Slot {
    key: TileKey,
    source: Weak<Tile>,
}
pub(crate) struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    overview: overview::Overview,
    atlas: wgpu::Texture,
    view: wgpu::TextureView,
    slots: Vec<Option<Slot>>,
    preferred: Vec<TileKey>,
    failure: Arc<Mutex<Option<String>>>,
    /// Whether the adapter is a software rasterizer (allowed only on request).
    #[cfg_attr(not(test), allow(dead_code))]
    software: bool,
    /// The adapter's name and backend, for diagnostics.
    adapter: String,
    /// The relief march (`relief`), made with the first relief frame; `None` inside
    /// once it failed its check on this adapter.
    relief: Option<Option<relief::ReliefMarch>>,
    /// The elevation tiles for the next relief walk (`relief::next_tiles`), and the
    /// ones the last walk read.
    relief_tiles: Vec<TileKey>,
    relief_read: Vec<TileKey>,
}
/// The share of colour channels that may differ from the CPU renderer by more than one
/// level (of 255): the shader's f32 trigonometry moves a few texel lookups by a
/// fraction of a texel, which shows on sharp imagery edges and nowhere else.
#[cfg(test)]
pub(crate) const PARITY_SHARE_OVER_ONE: f64 = 0.001;
/// With real imagery (the frame benchmark) the f32 level of detail also lands on the
/// other side of the CPU's area-filter and fade thresholds along thin contours: up to
/// this share of channels, by at most this many levels.
/// The start-up self-check holds an adapter to the same bounds.
pub(crate) const IMAGERY_SHARE_OVER_ONE: f64 = 0.01;
pub(crate) const IMAGERY_MAX_DIFFERENCE: u8 = 16;

/// Set to accept a software GPU adapter (lavapipe, WARP): for checking parity where
/// there is no GPU, never faster than the CPU renderer.
const ALLOW_SOFTWARE_ENV: &str = "GLOBECRAFT_GPU_ALLOW_SOFTWARE";

impl GpuRenderer {
    #[cfg(test)]
    pub(crate) fn destroy_for_test(&self) {
        self.device.destroy();
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll destroyed device");
    }
    pub(crate) fn supports(request: &RenderRequest) -> bool {
        // Relief and the unblended screenshot levels are the CPU's. So is a surface the
        // shader has no kernel for (it has the unit sphere and ellipsoids).
        if request.cfg.terrain.is_some() || !request.cfg.level_blend {
            return false;
        }
        use globecraft_core::{EARTH_RADIUS_KM, earth_model::SurfaceGeometry};
        match request.basis.model.geometry() {
            SurfaceGeometry::Sphere { radius_km } if radius_km == EARTH_RADIUS_KM => {}
            SurfaceGeometry::Ellipsoid { .. } => {}
            SurfaceGeometry::Sphere { .. } => return false,
        }

        overview::supported_zoom(request).is_some()
    }
    /// A renderer whose pictures match the CPU's on this adapter (`self_check`).
    pub(crate) fn new() -> Result<Self, String> {
        let mut renderer = pollster::block_on(Self::initialize())?;
        renderer.self_check()?;
        Ok(renderer)
    }
    /// The adapter's name and backend.
    pub(crate) fn adapter(&self) -> &str {
        &self.adapter
    }
    async fn initialize() -> Result<Self, String> {
        // Vulkan, Metal or DX12 only: wgpu's OpenGL backend lacks what this compute
        // renderer needs, and starting it opens EGL, which can panic on a broken
        // driver (issue #1).
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|error| format!("No compatible GPU adapter: {error}"))?;
        // A software rasterizer is slower than the CPU renderer; it is used only when
        // asked for, so the parity tests can run on machines without a GPU (CI).
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu
            && std::env::var_os(ALLOW_SOFTWARE_ENV).is_none()
        {
            return Err("Only a software GPU adapter is available".into());
        }
        let available = adapter.limits();
        let layers = MAX_ATLAS_LAYERS.min(available.max_texture_array_layers);
        let limits = wgpu::Limits {
            max_texture_array_layers: layers,
            ..Default::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("GlobeCraft compute renderer"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?;
        let failure = Arc::new(Mutex::new(None));
        let errors = failure.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            *errors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
        }));
        let lost = failure.clone();
        device.set_device_lost_callback(move |reason, message| {
            *lost
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(format!("GPU device lost ({reason:?}): {message}"));
        });
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GlobeCraft bounded tile atlas"),
            size: wgpu::Extent3d {
                width: TILE_EDGE_PX,
                height: TILE_EDGE_PX,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let overview = overview::Overview::new(&device);
        if let Some(error) = scope.pop().await {
            return Err(error.to_string());
        }
        if let Some(error) = failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error);
        }
        globecraft_data::log_line!(
            "GPU globe rendering enabled: {} ({:?}), {} MiB tile atlas",
            adapter.get_info().name,
            adapter.get_info().backend,
            layers * TILE_BYTES / (1024 * 1024)
        );
        Ok(Self {
            device,
            queue,
            overview,
            atlas,
            view,
            slots: (0..layers).map(|_| None).collect(),
            preferred: Vec::new(),
            failure,
            software: adapter.get_info().device_type == wgpu::DeviceType::Cpu,
            adapter: format!(
                "{} ({:?})",
                adapter.get_info().name,
                adapter.get_info().backend
            ),
            relief: None,
            relief_tiles: Vec::new(),
            relief_read: Vec::new(),
        })
    }

    /// What the GPU's walk found for each pixel (`relief::Walked::pixels`); `None`
    /// until a relief frame told which elevation tiles it held (`relief_frame_held`),
    /// or once the walk failed its check or a frame on this adapter (only a lost
    /// device is an error: the GPU renderer goes with it).
    pub(crate) fn relief_starts(
        &mut self,
        request: &RenderRequest,
        cache: &TileCache,
    ) -> Result<Option<Vec<f32>>, String> {
        if let Some(error) = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error);
        }
        let (device, queue) = (&self.device, &self.queue);
        let layers = self.slots.len() as u32;
        let march = self.relief.get_or_insert_with(|| {
            let mut march = relief::ReliefMarch::new(device, layers);
            match march.self_check(device, queue) {
                Ok(()) => Some(march),
                Err(error) => {
                    globecraft_data::log_line!("The CPU marches the relief alone: {error}");
                    None
                }
            }
        });
        let Some(march) = march else {
            return Ok(None);
        };
        let tiles = relief::grown_tiles(
            &self.relief_tiles,
            |key| cache.lookup(key),
            std::time::Instant::now(),
        );
        if tiles.is_empty() {
            return Ok(None);
        }
        match march.starts(device, queue, request, &tiles) {
            Ok(walked) => {
                self.relief_read = walked.read;
                Ok(Some(walked.pixels))
            }
            Err(error) => {
                globecraft_data::log_line!("The CPU marches the relief alone: {error}");
                self.relief = Some(None);
                Ok(None)
            }
        }
    }
    /// The elevation tiles a relief frame held: with the ones the walk read, the
    /// walk's for the next.
    pub(crate) fn relief_frame_held(&mut self, tiles: Vec<TileKey>) {
        self.relief_tiles = relief::next_tiles(tiles, &self.relief_read);
    }

    /// Draws one small view of a made-up tile on this adapter and on the CPU, and
    /// refuses the adapter when the pictures differ by more than the parity tests
    /// allow: a driver that computes wrong pixels must not draw the globe. Run once
    /// per device, before its first frame.
    fn self_check(&mut self) -> Result<(), String> {
        const WIDTH: usize = 96;
        const HEIGHT: usize = 72;
        // A smooth pattern: f32 texel rounding then moves no channel by more than a level.
        let pattern = image::RgbaImage::from_fn(TILE_EDGE_PX, TILE_EDGE_PX, |x, y| {
            image::Rgba([x as u8, y as u8, ((x + y) / 2) as u8, 255])
        });
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pattern)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| format!("GPU self-check tile: {error}"))?;
        let tile = Arc::new(
            Tile::decode(png.get_ref()).map_err(|error| format!("GPU self-check tile: {error}"))?,
        );
        let provider = globecraft_data::tiles::Provider::Streets;
        let key = globecraft_data::tiles::make_key(provider, 0, 0, 0, 0);
        let cam = globecraft_core::camera::Cam {
            lat: 40.0,
            lon: 10.0,
            heading: 25.0,
            alt: 9000.0,
            tilt: 0.0,
            look_up: 0.0,
        };
        let request = RenderRequest {
            width: WIDTH,
            height: HEIGHT,
            cam,
            basis: cam.basis_for_model(WIDTH as f64 / HEIGHT as f64, Default::default()),
            cfg: crate::RenderCfg {
                sky: None,
                provider,
                terrain: None,
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
        let lookup = |wanted| {
            if wanted == key {
                globecraft_data::tiles::Lookup::Ready(tile.clone())
            } else {
                globecraft_data::tiles::Lookup::Unavailable
            }
        };
        let slots = self.upload(&[(key, tile.clone())])?;
        let frame = self
            .overview
            .render(
                overview::Context {
                    device: &self.device,
                    queue: &self.queue,
                    atlas: &self.view,
                },
                &request,
                &slots,
                &lookup,
                Vec::new(),
            )
            .map_err(|(error, _, _)| format!("GPU self-check: {error}"))?;
        let (expected, _, _) = crate::render_pixels_into(
            WIDTH,
            HEIGHT,
            0..HEIGHT,
            &request.basis,
            &request.cfg,
            &lookup,
            Vec::new(),
        );
        parity(&frame.view.pixels, &expected).map_err(|(over_one, largest)| {
            format!(
                "GPU self-check: {} draws {over_one} of {} colour channels more than one level from the CPU renderer (largest difference {largest})",
                self.adapter,
                expected.len()
            )
        })?;
        Ok(())
    }

    fn upload(&mut self, tiles: &[(TileKey, Arc<Tile>)]) -> Result<HashMap<TileKey, u32>, String> {
        if tiles.len() > self.slots.len() {
            return Err("Decoded provider working set exceeds the bounded GPU atlas".into());
        }
        if tiles.iter().any(|(_, tile)| {
            let (w, h, _) = tile.rgba();
            w != TILE_EDGE_PX as usize || h != TILE_EDGE_PX as usize
        }) {
            return Err("Tile dimensions require the general CPU sampler".into());
        }
        let wanted: std::collections::HashSet<_> = tiles.iter().map(|(key, _)| *key).collect();
        let mut map: HashMap<_, _> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|slot| (slot.key, i as u32)))
            .collect();
        for (key, tile) in tiles {
            let layer = if let Some(&layer) = map.get(key) {
                layer
            } else {
                let layer = self
                    .slots
                    .iter()
                    .position(Option::is_none)
                    .or_else(|| {
                        self.slots.iter().position(|slot| {
                            slot.as_ref()
                                .is_some_and(|slot| !wanted.contains(&slot.key))
                        })
                    })
                    .ok_or("Visible band exceeds GPU residency capacity")?;
                if let Some(old) = &self.slots[layer] {
                    map.remove(&old.key);
                }
                map.insert(*key, layer as u32);
                layer as u32
            };
            let slot = &mut self.slots[layer as usize];
            if slot
                .as_ref()
                .and_then(|slot| slot.source.upgrade())
                .is_some_and(|source| Arc::ptr_eq(&source, tile))
            {
                continue;
            }
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                tile.rgba().2,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE_EDGE_PX * 4),
                    rows_per_image: Some(TILE_EDGE_PX),
                },
                wgpu::Extent3d {
                    width: TILE_EDGE_PX,
                    height: TILE_EDGE_PX,
                    depth_or_array_layers: 1,
                },
            );
            *slot = Some(Slot {
                key: *key,
                source: Arc::downgrade(tile),
            });
        }
        map.retain(|key, _| wanted.contains(key));
        Ok(map)
    }
    pub(crate) fn render(
        &mut self,
        request: &RenderRequest,
        cache: &TileCache,
        buffer: Vec<u8>,
    ) -> Result<RenderedView, (String, Vec<u8>, bool)> {
        if let Some(error) = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err((error, buffer, true));
        }
        let Some(max_zoom) = overview::supported_zoom(request) else {
            return Err(("View requires precise CPU rendering".into(), buffer, false));
        };
        let lights = request.cfg.night_lights.map_or_else(Vec::new, |edition| {
            cache.ready_tiles(globecraft_data::tiles::Provider::NightLights, edition)
        });
        let tiles: Vec<_> = cache
            .ready_tiles(request.cfg.provider, request.cfg.rel)
            .into_iter()
            .chain(lights)
            .filter(|(key, _)| globecraft_data::tiles::unkey(*key).1 <= max_zoom)
            .collect();
        let ready: HashMap<_, _> = tiles.iter().map(|(key, _)| (*key, ())).collect();
        let mut selected: std::collections::HashSet<_> = self
            .preferred
            .iter()
            .copied()
            .filter(|key| ready.contains_key(key))
            .collect();
        let mut buffer = buffer;
        for pass in 0..MAX_RESIDENCY_PASSES {
            if selected.len() > self.slots.len() {
                return Err((
                    "Visible tile working set exceeds the bounded GPU atlas".into(),
                    buffer,
                    false,
                ));
            }
            // Coarse global imagery is cheap and avoids a cold opposite hemisphere.
            // Current-view tiles always take precedence over this optional set.
            for (key, _) in &tiles {
                if selected.len() >= self.slots.len() {
                    break;
                }
                if globecraft_data::tiles::unkey(*key).1 <= GLOBAL_RESIDENT_ZOOM {
                    selected.insert(*key);
                }
            }
            let referenced: Vec<_> = tiles
                .iter()
                .filter(|(key, _)| selected.contains(key))
                .cloned()
                .collect();
            let slots = match self.upload(&referenced) {
                Ok(slots) => slots,
                Err(error) => return Err((error, buffer, false)),
            };
            let frame = self.overview.render(
                overview::Context {
                    device: &self.device,
                    queue: &self.queue,
                    atlas: &self.view,
                },
                request,
                &slots,
                &|key| cache.lookup(key),
                buffer,
            )?;
            if let Some(error) = self
                .failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                return Err((error, frame.view.pixels, true));
            }
            let wanted: std::collections::HashSet<_> = frame
                .sampled
                .iter()
                .copied()
                .filter(|key| ready.contains_key(key))
                .collect();
            let missing_ready = wanted.iter().any(|key| !slots.contains_key(key));
            self.preferred = frame.sampled;
            if !missing_ready {
                return Ok(frame.view);
            }
            buffer = frame.view.pixels;
            if pass + 1 == MAX_RESIDENCY_PASSES {
                return Err((
                    "GPU residency could not resolve this view within its bounded passes".into(),
                    buffer,
                    false,
                ));
            }
            selected = wanted;
        }
        unreachable!("nonzero bounded GPU residency passes")
    }
}
/// Whether a GPU picture matches the CPU's within the bounds real imagery is held to;
/// else how many channels are more than one level off, and the largest difference.
fn parity(gpu: &[u8], cpu: &[u8]) -> Result<(), (usize, u8)> {
    let (mut over_one, mut largest) = (0usize, 0u8);
    for difference in gpu.iter().zip(cpu).map(|(gpu, cpu)| gpu.abs_diff(*cpu)) {
        over_one += usize::from(difference > 1);
        largest = largest.max(difference);
    }
    if gpu.len() != cpu.len()
        || over_one as f64 >= cpu.len() as f64 * IMAGERY_SHARE_OVER_ONE
        || largest > IMAGERY_MAX_DIFFERENCE
    {
        return Err((over_one, largest));
    }
    Ok(())
}
fn binding(buffer: &wgpu::Buffer, size: u64) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer,
        offset: 0,
        size: std::num::NonZeroU64::new(size),
    })
}
