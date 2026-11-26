//! The globe for the native Android and iPadOS apps (mobile plan, Phase 0).
//!
//! The host app (Kotlin or Swift) owns the window, the gestures and every control; this
//! crate owns the camera, the tiles and the renderer, and hands back finished RGBA
//! frames. The host calls [`Globe::tick`] once per display refresh while
//! [`Globe::busy`] says there is work, and shows [`Globe::frame`] when it changed.
//! Gestures are in points (the host's density-independent units).
//!
//! Android: `android.rs` (JNI, draws into a `Surface`). iOS: the C functions in `ffi.rs`
//! (header `include/globecraft_mobile.h`).

use emath::{Rect, pos2, vec2};
use globecraft_core::{
    camera::Cam,
    earth_model::EarthModel,
    math::{ll_to_v3, subsolar, wrap_lon},
};
use globecraft_data::{
    runtime::ServiceRuntime,
    tiles::{CacheBudgets, Provider, TileCache},
};
use globecraft_render::{
    RenderCfg, TerrainCfg,
    backend::{BackendPreference, FrameRenderer},
    view::{MOTION_STEPS, next_motion_step},
    worker::{RenderRequest, RenderWorker},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android;
#[allow(unsafe_code)]
mod ffi;

/// How long after the last gesture frames stay at motion resolution.
const SETTLE: Duration = Duration::from_millis(150);
/// Raster size while a gesture moves the globe, relative to a still frame's.
const MOTION_SCALE: f32 = 0.5;
/// Most pixels a still frame the CPU draws has; the host scales it to the screen.
/// A phone's 1080 × 2404 (2.6 million) took the CPU 150–1,600 ms a frame on a
/// PowerVR DXT-48-1536 phone, 540 × 1202 46–333 ms (2026-11-24).
const MAX_CPU_FRAME_PIXELS: f32 = 650_000.0;
/// The run log's last bytes the host can share ([`Globe::log_tail`]).
const LOG_TAIL_BYTES: u64 = 64 * 1024;
/// Phone budgets: decoded tiles in memory, and downloads kept on disk.
/// shortcut: fixed numbers, until Phase 1 sizes them from the device's memory.
const BUDGETS: CacheBudgets = CacheBudgets {
    decoded_bytes: 384 * 1024 * 1024,
    disk_bytes: 2 * 1024 * 1024 * 1024,
};

/// A finished frame: premultiplied RGBA, `width` × `height` pixels, rows packed.
#[derive(Default)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

pub struct Globe {
    // Declared first, dropped first: the worker and the cache stop before the runtime.
    worker: RenderWorker,
    cache: Arc<TileCache>,
    _runtime: ServiceRuntime,
    pub cam: Cam,
    /// The view in pixels, and pixels per point.
    width: usize,
    height: usize,
    scale: f32,
    pub relief: bool,
    /// Set by the tile cache when tiles arrive: the frame shown may now be sharper.
    tiles_arrived: Arc<AtomicBool>,
    /// The camera, the size or a setting changed since the last request.
    changed: bool,
    last_gesture: Option<Instant>,
    /// The last request was a motion frame, so a full one follows once gestures stop.
    motion_frame: bool,
    /// Which of `view::MOTION_STEPS` moving relief frames are drawn at.
    motion_step: usize,
    presentation: u64,
    frame: Frame,
    /// Microseconds the last frame took to render.
    render_us: Arc<AtomicU64>,
}

impl Globe {
    /// Starts the network runtime, the tile cache and the renderer. `app_dir` is the
    /// app's own writable folder (caches and settings go below it).
    pub fn new(app_dir: std::path::PathBuf) -> std::io::Result<Self> {
        globecraft_data::use_app_dir(app_dir);
        globecraft_data::run_log::init();
        // Phase 0 measures: every relief frame's time split goes to the shared log.
        globecraft_render::backend::log_frame_times();
        let runtime = ServiceRuntime::new()?;
        let tiles_arrived = Arc::new(AtomicBool::new(true));
        let arrived = tiles_arrived.clone();
        let cache = TileCache::with_budgets(
            move || arrived.store(true, Ordering::Release),
            runtime.handle(),
            BUDGETS,
        );
        let render_us = Arc::new(AtomicU64::new(0));
        let timing = render_us.clone();
        let render_cache = cache.clone();
        let admission_cache = cache.clone();
        // The GPU when it passes its self-check against the CPU renderer, else the CPU.
        let mut backend = FrameRenderer::new(BackendPreference::Auto)
            .with_start_marker(globecraft_data::gpu_start_marker());
        let worker = RenderWorker::new(
            move |request, buffer| {
                let started = Instant::now();
                let view = backend.render(request, &render_cache, buffer);
                timing.store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                view
            },
            // The host polls once per display refresh; nothing to wake.
            || {},
            move |view| admission_cache.prioritize_view(&view.required_tiles()),
        )?;
        Ok(Self {
            worker,
            cache,
            _runtime: runtime,
            cam: Cam {
                lat: 48.0,
                lon: 10.0,
                heading: 0.0,
                alt: 12_000.0,
                tilt: 0.0,
                look_up: 0.0,
            },
            width: 1,
            height: 1,
            scale: 1.0,
            relief: true,
            tiles_arrived,
            changed: true,
            last_gesture: None,
            motion_frame: false,
            motion_step: 0,
            presentation: 0,
            frame: Frame::default(),
            render_us,
        })
    }

    /// The view's size in pixels and the screen's pixels per point.
    pub fn set_size(&mut self, width: usize, height: usize, scale: f32) {
        (self.width, self.height) = (width.max(1), height.max(1));
        self.scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        self.presentation += 1;
        self.changed = true;
    }

    fn rect(&self) -> Rect {
        Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(self.width as f32, self.height as f32) / self.scale,
        )
    }

    fn aspect(&self) -> f64 {
        self.width as f64 / self.height as f64
    }

    fn moved(&mut self) {
        self.last_gesture = Some(Instant::now());
        self.changed = true;
    }

    /// A one-finger drag by (`dx`, `dy`) points: the ground follows the finger.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let height = self.rect().height();
        self.cam
            .pan_for_model(dx as f64, dy as f64, height, EarthModel::default());
        self.moved();
    }

    /// A pinch by `scale` (above 1: fingers apart, closer) around the point (`x`, `y`).
    pub fn pinch(&mut self, scale: f32, x: f32, y: f32) {
        if !(scale.is_finite() && scale > 0.0) {
            return;
        }
        let (rect, aspect) = (self.rect(), self.aspect());
        self.cam.zoom_at_for_model(
            1.0 / scale as f64,
            Some(pos2(x, y)),
            rect,
            aspect,
            EarthModel::default(),
        );
        self.moved();
    }

    /// A two-finger turn by `degrees`, clockwise on screen: the map turns with it.
    pub fn rotate(&mut self, degrees: f32) {
        self.cam.heading = wrap_lon(self.cam.heading - degrees as f64);
        self.moved();
    }

    /// A two-finger vertical drag: positive `degrees` lean the view toward the horizon.
    pub fn tilt(&mut self, degrees: f32) {
        let tilt = self.cam.view_tilt() + degrees as f64;
        self.cam.tilt_view(tilt);
        self.moved();
    }

    pub fn set_relief(&mut self, on: bool) {
        self.relief = on;
        self.changed = true;
    }

    fn moving(&self, now: Instant) -> bool {
        self.last_gesture
            .is_some_and(|at| now.duration_since(at) < SETTLE)
    }

    fn config(&self, width: usize, height: usize, lod_scale: f32, unix: f64) -> RenderCfg {
        let (lat, lon) = subsolar(unix);
        RenderCfg {
            sky: None,
            provider: Provider::Satellite,
            terrain: self
                .relief
                .then(|| TerrainCfg::new(1.0, globecraft_render::relief_fade(self.cam.alt, true))),
            rel: 0,
            grid: false,
            grid_step: globecraft_render::grid_step(self.cam.alt, width, height),
            atmo: true,
            sun: Some(ll_to_v3(lat, lon)),
            lod_scale,
            level_blend: true,
            fixed_level: None,
            night_lights: None,
            stars: None,
            // Sky pixels carry the air's glow over transparency, drawn over the hosts'
            // black (Android's surface, iOS's `backgroundColor`): the same picture,
            // but the GPU draws it. Without, the CPU drew every sky pixel after the
            // GPU's frame, 17–48 % of a tilted phone frame, 15–60 ms of a 135×301
            // one on a PowerVR phone (2026-11-26).
            sky_behind: true,
            clouds: None,
            glint: None,
            aurora: None,
        }
    }

    /// A still frame's size relative to the screen's pixels: all of them when the GPU
    /// draws, at most `MAX_CPU_FRAME_PIXELS` when the CPU does, and with relief (the
    /// GPU's walk costs per pixel too, and its cost on a phone is not measured yet).
    fn still_scale(&self) -> f32 {
        if globecraft_render::backend::status().adapter.is_some() && !self.relief {
            return 1.0;
        }
        let pixels = (self.width * self.height) as f32;
        (MAX_CPU_FRAME_PIXELS / pixels).sqrt().min(1.0)
    }

    fn request(&mut self, now: Instant) {
        let moving = self.moving(now);
        let motion = MOTION_SCALE
            * if self.relief {
                MOTION_STEPS[self.motion_step]
            } else {
                1.0
            };
        let lod_scale = self.still_scale() * if moving { motion } else { 1.0 };
        let size = |n: usize| ((n as f32 * lod_scale).round() as usize).max(1);
        let (width, height) = (size(self.width), size(self.height));
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64());
        let basis = self
            .cam
            .basis_for_model(self.aspect(), EarthModel::default());
        let request = RenderRequest {
            width,
            height,
            cam: self.cam,
            basis,
            cfg: self.config(width, height, lod_scale, unix),
            overlays: None,
            aircraft: None,
            live_layers: None,
            compare: None,
            presentation: self.presentation,
        };
        // The first moving frame after a still one does not wait for it: a still
        // relief frame took the phone 250–450 ms, and a gesture begun meanwhile caught
        // up only after it (Julien, 2026-11-26).
        if moving && !self.motion_frame {
            self.worker.submit_preempting(request);
        } else {
            self.worker.submit(request);
        }
        self.motion_frame = moving;
        self.changed = false;
    }

    /// Asks for a frame if anything changed and collects a finished one. Returns true
    /// when [`Globe::frame`] holds a new frame to show.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let settled = self.motion_frame && !self.moving(now);
        if self.changed || settled || self.tiles_arrived.swap(false, Ordering::AcqRel) {
            self.request(now);
        }
        let Some(done) = self.worker.take_completed(self.presentation) else {
            return false;
        };
        if self.relief && done.request.cfg.lod_scale < self.still_scale() * MOTION_SCALE * 1.01 {
            let took = Duration::from_micros(self.render_us.load(Ordering::Relaxed));
            self.motion_step = next_motion_step(self.motion_step, took);
        }
        let previous = std::mem::replace(
            &mut self.frame,
            Frame {
                width: done.request.width,
                height: done.request.height,
                pixels: done.pixels,
            },
        );
        self.worker.recycle(previous.pixels);
        true
    }

    /// Whether the host should keep calling [`Globe::tick`]: a frame is being drawn or
    /// wanted, or tiles are still downloading.
    pub fn busy(&self) -> bool {
        self.changed
            || self.motion_frame
            || !self.worker.is_idle()
            || self.tiles_arrived.load(Ordering::Acquire)
            || self.cache.inflight() > 0
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// The run log's last part (`globecraft.log`), for the host to share.
    pub fn log_tail() -> String {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(globecraft_data::run_log::path()) else {
            return "No run log yet.".to_owned();
        };
        let length = file.metadata().map_or(0, |meta| meta.len());
        let _ = file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL_BYTES)));
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// One line for the debug overlay: renderer, frame size and time, downloads.
    pub fn stats(&self) -> String {
        let status = globecraft_render::backend::status();
        // On the CPU, say why the GPU renderer is not drawing (if it was refused).
        let renderer = match (status.adapter, status.note) {
            (Some(adapter), _) => adapter,
            (None, Some(note)) => format!("CPU ({note})"),
            (None, None) => "CPU".to_owned(),
        };
        format!(
            "{renderer} · {}×{} in {:.1} ms · GPU {} / CPU {} frames · {} downloads · {:.0} km",
            self.frame.width,
            self.frame.height,
            self.render_us.load(Ordering::Relaxed) as f64 / 1000.0,
            status.gpu_frames,
            status.cpu_frames,
            self.cache.inflight(),
            self.cam.alt,
        )
    }
}

#[cfg(test)]
mod tests;
