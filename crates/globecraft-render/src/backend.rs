//! Backend selection owned by the background frame coordinator.
use crate::{RenderedView, worker::RenderRequest};
use globecraft_data::tiles::TileCache;
// Complete 512x384 GPU/CPU frames were tied on the baseline adapter; avoid
// GPU synchronization overhead for small rasters while retaining forced tests.
#[cfg(feature = "gpu")]
const MIN_AUTO_GPU_PIXELS: usize = 256 * 1024;

// Optional GPU backend failures must not unwind the frame coordinator. A panic
// loses its in-progress buffer; CPU fallback allocates another only on failure.
#[cfg(feature = "gpu")]
fn gpu_call<T>(operation: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).map_err(|payload| {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic payload");
        format!("GPU backend panicked: {message}")
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendPreference {
    Auto,
    Cpu,
    Gpu,
}
/// Whether the GPU renderer draws the globe unless told otherwise: wherever it is
/// built in. Each adapter proves itself before its first frame (the self-check
/// against the CPU renderer), a start that never finished is not repeated (the
/// start marker), and a device that fails hands its frames to the CPU. Measured on
/// an M1 (Metal, 2026-11-01): 2000×1488 frames in 14 ms against 40 ms on the CPU.
pub const GPU_BY_DEFAULT: bool = cfg!(feature = "gpu");

/// What draws the globe: for diagnostics and Settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The GPU adapter in use (name and backend) while the GPU renderer runs.
    pub adapter: Option<String>,
    /// Why the GPU renderer does not run although it was asked for, in English.
    pub note: Option<String>,
    /// Frames drawn by each renderer since the start. Close views, the grid, relief
    /// and small frames are the CPU's even while the GPU renderer runs.
    pub gpu_frames: u64,
    pub cpu_frames: u64,
}
static STATUS: std::sync::Mutex<Status> = std::sync::Mutex::new(Status {
    adapter: None,
    note: None,
    gpu_frames: 0,
    cpu_frames: 0,
});
/// What draws the globe now.
pub fn status() -> Status {
    STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}
fn update_status(update: impl FnOnce(&mut Status)) {
    update(
        &mut STATUS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
}

/// GPU frames logged with their times from the start, and the time from which any is.
#[cfg(feature = "gpu")]
const LOGGED_GPU_FRAMES: u64 = 20;
#[cfg(feature = "gpu")]
const SLOW_GPU_FRAME: std::time::Duration = std::time::Duration::from_secs(1);

/// A failed GPU device (lost, an error, a frame that never finished) is replaced
/// after this long, so a driver restart or a laptop waking up has settled…
#[cfg(feature = "gpu")]
const GPU_RESTART_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// …at most this many times per session; then the CPU draws until GlobeCraft restarts.
#[cfg(feature = "gpu")]
const MAX_GPU_RESTARTS: u8 = 3;
/// What the start marker holds: a start that crashed is tried again by the next version.
#[cfg(feature = "gpu")]
const MARKER_VERSION: &str = env!("CARGO_PKG_VERSION");

impl BackendPreference {
    /// `GLOBECRAFT_RENDERER` when set, else the platform default unless the user
    /// turned the GPU off (`allow_gpu`, the `gpu_globe` setting).
    pub fn from_environment(allow_gpu: bool) -> Self {
        Self::resolve(
            std::env::var("GLOBECRAFT_RENDERER").ok().as_deref(),
            allow_gpu,
            GPU_BY_DEFAULT,
        )
    }
    fn resolve(setting: Option<&str>, allow_gpu: bool, gpu_by_default: bool) -> Self {
        match setting {
            None if allow_gpu && gpu_by_default => Self::Auto,
            None => Self::Cpu,
            setting => Self::from_setting(setting),
        }
    }
    fn from_setting(setting: Option<&str>) -> Self {
        match setting {
            Some("gpu") => Self::Gpu,
            Some("auto") => Self::Auto,
            Some("cpu") | None => Self::Cpu,
            Some(value) => {
                globecraft_data::log_line!(
                    "Unknown GLOBECRAFT_RENDERER value {value:?}; using CPU rendering"
                );
                Self::Cpu
            }
        }
    }
}

/// Created cheaply on the UI thread; adapter/device initialization is deferred
/// until `render` runs on the frame coordinator. A device that fails to start is not
/// retried; one that fails later is replaced a bounded number of times.
pub struct FrameRenderer {
    preference: BackendPreference,
    initialized: bool,
    #[cfg(feature = "gpu")]
    gpu: Option<crate::gpu::GpuRenderer>,
    #[cfg(feature = "gpu")]
    capacity_warning: bool,
    /// When a failed device may be replaced, and how many replacements remain.
    #[cfg(feature = "gpu")]
    restart_at: Option<std::time::Instant>,
    #[cfg(feature = "gpu")]
    restarts_left: u8,
    /// See `with_start_marker`.
    #[cfg(feature = "gpu")]
    start_marker: Option<std::path::PathBuf>,
    #[cfg(feature = "gpu")]
    relief_timing: ReliefTiming,
    /// The last relief frame's drawn ground (`take_depth`).
    #[cfg(feature = "gpu")]
    depth: Option<std::sync::Arc<crate::overlays::DepthMap>>,
}

/// Whether the GPU's relief walk (`gpu::relief`) makes relief frames faster here:
/// it costs the GPU what it saves the CPU, and only the hardware and the view say
/// which is more. The walk's frames cost about the same whatever the view, the CPU's
/// change with it: on a PowerVR phone at 270×601 the walk's frames took 35–60 ms
/// everywhere, the CPU's 11–22 ms over flat land and 73–224 ms over tilted mountains
/// (2026-11-26; a rule that turned the walk off for good after flat checks left the
/// mountains at 5–9 frames a second). So the last frame with the walk stands for its
/// cost at that size, and the CPU marches alone only while it beats that: every
/// `RELIEF_CHECK_EVERY`th frame tries the CPU (the third relief frame first), and once
/// it is faster the CPU draws until one of its frames is slower than the walk's, after
/// which the next walks; every `RELIEF_CHECK_EVERY`th it also tries the walk, unless
/// that took `RELIEF_DECISIVE_WIN` times the CPU's time (a software adapter, ten
/// times the CPU's whole frame, 2026-11-23, stays with the CPU). A walk that ran but
/// did not help counts with the CPU's frame after it: that is what trying it cost. The first walked frame is never timed: it
/// carries the walk's setup (its pipelines, Metal compiling them after a fresh build,
/// the self-check, the first tiles; 1238 against 580 ms per megapixel on an M1 once
/// turned the walk off for a session, 2026-11-24). Frames are only set against frames
/// of their size (a motion frame and a sharp one): a frame's fixed costs weigh more
/// per pixel in a small one. Where the walk wins `RELIEF_SETTLED_WINS` checks in a row
/// clearly (the CPU taking `RELIEF_CLEAR_WIN` times as long or longer), or one by
/// `RELIEF_DECISIVE_WIN`, the checks stop: on an M1 a check frame took 4–6 times as
/// long (about 0.6–1 s at 1920×1200 against 0.14–0.18 s), a hitch every 20 relief
/// frames (2026-11-24).
#[cfg(feature = "gpu")]
#[derive(Default)]
struct ReliefTiming {
    frames: u32,
    /// Frames drawn with the walk.
    walked: u32,
    /// The last frame with the walk at each recent size: its pixels, and seconds per
    /// megapixel (at most `TIMED_SIZES`).
    walk_costs: Vec<(usize, f64)>,
    wins: u32,
    /// The walk won clearly often enough: no more frames without it.
    settled: bool,
    /// The CPU marches alone for now.
    cpu_for_now: bool,
    /// While `cpu_for_now`: its last frame's pixels and seconds per megapixel.
    last_cpu: Option<(usize, f64)>,
}
/// Frame sizes whose walk costs are kept: the motion and the sharp frame's, and a
/// window's last few sizes.
#[cfg(feature = "gpu")]
const TIMED_SIZES: usize = 4;
#[cfg(feature = "gpu")]
const RELIEF_CHECK_EVERY: u32 = 20;

/// Relief frames logged as with `GLOBECRAFT_FRAME_TIMES` (`log_frame_times`).
#[cfg(feature = "gpu")]
static FRAME_TIMES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Logs every relief frame's time split, as `GLOBECRAFT_FRAME_TIMES` does: for hosts
/// that set no environment (the mobile apps).
pub fn log_frame_times() {
    #[cfg(feature = "gpu")]
    FRAME_TIMES.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// With `GLOBECRAFT_FRAME_TIMES` set: one log line per relief frame, its size, who
/// drew it, the walk's time (and the elevation tiles it uploaded first) and the whole
/// frame's, to find where a slow frame's time goes; for the GPU's, also its colour
/// passes' time on the device with the readback, and the CPU's repair after them.
#[cfg(feature = "gpu")]
fn log_relief_frame(
    request: &RenderRequest,
    drawn_by: &str,
    walk: std::time::Duration,
    total: std::time::Duration,
    uploads: usize,
    draw: Option<(
        (usize, std::time::Duration, std::time::Duration, usize),
        f32,
    )>,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if FRAME_TIMES.load(std::sync::atomic::Ordering::Relaxed)
        || *ON.get_or_init(|| std::env::var_os("GLOBECRAFT_FRAME_TIMES").is_some())
    {
        let ms = |time: std::time::Duration| time.as_secs_f64() * 1000.0;
        let pixels = (request.width * request.height).max(1);
        let draw = draw.map_or_else(
            String::new,
            |((passes, device, repair, repaired), sun_unknown)| {
                format!(
                    " (colour: {passes} pass(es), {:.1} ms on the device with the readback, {:.1} ms CPU repair of {repaired} pixels, {:.1} %; the walk's sun unknown at {:.0} % of its grounds)",
                    ms(device),
                    ms(repair),
                    100.0 * repaired as f64 / pixels as f64,
                    100.0 * sun_unknown
                )
            },
        );
        globecraft_data::log_line!(
            "RELIEF FRAME {}x{} by the {drawn_by}: walk {:.1} ms ({uploads} tiles uploaded), whole {:.1} ms{draw}",
            request.width,
            request.height,
            ms(walk),
            ms(total)
        );
    }
}
#[cfg(feature = "gpu")]
const RELIEF_SETTLED_WINS: u32 = 3;
#[cfg(feature = "gpu")]
const RELIEF_CLEAR_WIN: f64 = 1.5;
/// A win this clear (the frame without the walk taking this many times as long or
/// longer) settles it at once: each check is a frame drawn without the walk, on an M1
/// 106–124 ms at 616×386 against 23–46 with it, and its overlays without the drawn
/// ground 53–68 ms more (2026-11-26).
#[cfg(feature = "gpu")]
const RELIEF_DECISIVE_WIN: f64 = 3.0;

#[cfg(feature = "gpu")]
impl ReliefTiming {
    fn walk_cost(&self, pixels: usize) -> Option<f64> {
        self.walk_costs
            .iter()
            .find(|(size, _)| *size == pixels)
            .map(|(_, cost)| *cost)
    }
    /// Whether the next relief frame, of `pixels`, takes the walk.
    fn walks(&self, pixels: usize) -> bool {
        if self.settled {
            return true;
        }
        if self.cpu_for_now {
            let Some(walk) = self.walk_cost(pixels) else {
                return true;
            };
            let check = (self.frames + 1).is_multiple_of(RELIEF_CHECK_EVERY);
            return self
                .last_cpu
                .filter(|(size, _)| *size == pixels)
                .is_some_and(|(_, cpu)| cpu > walk || (check && walk < cpu * RELIEF_DECISIVE_WIN));
        }
        !(self.frames == 2 || (self.frames + 1).is_multiple_of(RELIEF_CHECK_EVERY))
    }
    fn note(&mut self, walked: bool, seconds_per_megapixel: f64, pixels: usize) {
        self.frames += 1;
        if walked {
            self.walked += 1;
            if self.walked > 1 {
                self.walk_costs.retain(|(size, _)| *size != pixels);
                self.walk_costs.insert(0, (pixels, seconds_per_megapixel));
                self.walk_costs.truncate(TIMED_SIZES);
            }
            let beats_cpu = self
                .last_cpu
                .is_some_and(|(size, cpu)| size == pixels && seconds_per_megapixel < cpu);
            if self.cpu_for_now && beats_cpu {
                (self.cpu_for_now, self.last_cpu) = (false, None);
                globecraft_data::log_line!(
                    "The GPU's relief walk is faster again here ({:.0} ms per megapixel); it walks",
                    seconds_per_megapixel * 1000.0
                );
            }
            return;
        }
        let Some(walk) = self.walk_cost(pixels) else {
            return;
        };
        if self.cpu_for_now {
            self.last_cpu = Some((pixels, seconds_per_megapixel));
            return;
        }
        self.wins = if seconds_per_megapixel >= walk * RELIEF_CLEAR_WIN {
            self.wins + 1
        } else {
            0
        };
        self.settled =
            self.wins >= RELIEF_SETTLED_WINS || seconds_per_megapixel >= walk * RELIEF_DECISIVE_WIN;
        if seconds_per_megapixel < walk {
            (self.cpu_for_now, self.last_cpu) = (true, Some((pixels, seconds_per_megapixel)));
            globecraft_data::log_line!(
                "The CPU alone made relief frames faster here ({:.0} against {:.0} ms per megapixel with the walk); it marches alone while it stays faster",
                seconds_per_megapixel * 1000.0,
                walk * 1000.0
            );
        }
    }
}

impl FrameRenderer {
    pub fn new(preference: BackendPreference) -> Self {
        Self {
            preference,
            initialized: false,
            #[cfg(feature = "gpu")]
            gpu: None,
            #[cfg(feature = "gpu")]
            capacity_warning: false,
            #[cfg(feature = "gpu")]
            restart_at: None,
            #[cfg(feature = "gpu")]
            restarts_left: MAX_GPU_RESTARTS,
            #[cfg(feature = "gpu")]
            start_marker: None,
            #[cfg(feature = "gpu")]
            relief_timing: ReliefTiming::default(),
            #[cfg(feature = "gpu")]
            depth: None,
        }
    }
    /// The renderer the settings ask for (`BackendPreference::from_environment`). When
    /// `GLOBECRAFT_RENDERER` does not say otherwise, the GPU's start is guarded by
    /// `start_marker` (see `with_start_marker`).
    pub fn from_environment(allow_gpu: bool, start_marker: std::path::PathBuf) -> Self {
        let renderer = Self::new(BackendPreference::from_environment(allow_gpu));
        if std::env::var_os("GLOBECRAFT_RENDERER").is_some() {
            return renderer;
        }
        renderer.with_start_marker(start_marker)
    }
    /// Guards the GPU's start with a file: written before the device is created,
    /// removed after its first frame (or a start that failed cleanly). A crash or a
    /// forced quit in between leaves it, and the next start then keeps to the CPU
    /// instead of crashing again. Deleting the file allows another try.
    #[cfg_attr(not(feature = "gpu"), allow(unused_mut, unused_variables))]
    pub fn with_start_marker(mut self, path: std::path::PathBuf) -> Self {
        #[cfg(feature = "gpu")]
        {
            self.start_marker = Some(path);
        }
        self
    }
    #[cfg(feature = "gpu")]
    fn clear_start_marker(&mut self) {
        if let Some(path) = self.start_marker.take()
            && let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            globecraft_data::log_line!(
                "Unable to remove the GPU start marker {}: {error}",
                path.display()
            );
        }
    }
    #[cfg(feature = "gpu")]
    fn initialize_with(
        &mut self,
        initialize: impl FnOnce() -> Result<crate::gpu::GpuRenderer, String>,
    ) {
        let restart = self
            .restart_at
            .is_some_and(|at| std::time::Instant::now() >= at);
        if self.initialized && !restart {
            return;
        }
        let first = !self.initialized;
        self.initialized = true;
        self.restart_at = None;
        if self.preference == BackendPreference::Cpu {
            return;
        }
        if first && let Some(marker) = &self.start_marker {
            if std::fs::read_to_string(marker).is_ok_and(|version| version == MARKER_VERSION) {
                let note = "The GPU renderer did not finish starting last time, so the CPU draws the globe. Turn \"Draw the globe on the GPU\" off and on in Settings to try again.";
                globecraft_data::log_line!("{note}");
                update_status(|status| status.note = Some(note.into()));
                // The marker stays until the user asks for another try.
                self.start_marker = None;
                return;
            }
            if let Err(error) = marker
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(marker, MARKER_VERSION))
            {
                globecraft_data::log_line!(
                    "Unable to write the GPU start marker {}: {error}",
                    marker.display()
                );
            }
        }
        match gpu_call(initialize).and_then(|result| result) {
            Ok(gpu) => {
                update_status(|status| {
                    status.adapter = Some(gpu.adapter().into());
                    status.note = None;
                });
                self.gpu = Some(gpu);
            }
            Err(error) => {
                globecraft_data::log_line!("GPU renderer unavailable; using CPU: {error}");
                update_status(|status| status.note = Some(error));
                // A start that failed cleanly is not a crash.
                self.clear_start_marker();
            }
        }
    }
    /// After a device failed: the CPU draws, and another device is tried later while
    /// replacements remain.
    #[cfg(feature = "gpu")]
    fn device_failed(&mut self, error: &str) {
        self.gpu = None;
        let outlook = if self.restarts_left > 0 {
            self.restarts_left -= 1;
            self.restart_at = Some(std::time::Instant::now() + GPU_RESTART_DELAY);
            "starting the GPU renderer again"
        } else {
            "the CPU draws the globe until GlobeCraft restarts"
        };
        globecraft_data::log_line!("GPU renderer failed: {error}; {outlook}");
        update_status(|status| {
            status.adapter = None;
            status.note = Some(format!("{error}; {outlook}"));
        });
    }
    /// Draws `request`'s frame, a motion frame sharpened (`sharpen::motion_frame`).
    /// The drawn ground of the frame `render` just drew, when it was a relief frame
    /// the GPU's walk went through (see `overlays::DepthMap`).
    #[cfg(feature = "gpu")]
    pub fn take_depth(&mut self) -> Option<std::sync::Arc<crate::overlays::DepthMap>> {
        self.depth.take()
    }
    pub fn render(
        &mut self,
        request: &RenderRequest,
        cache: &TileCache,
        buffer: Vec<u8>,
    ) -> RenderedView {
        #[cfg(feature = "gpu")]
        {
            self.depth = None;
        }
        let mut frame = self.draw(request, cache, buffer);
        crate::sharpen::motion_frame(
            &mut frame.pixels,
            request.width,
            request.height,
            request.cfg.lod_scale,
        );
        frame
    }

    fn draw(
        &mut self,
        request: &RenderRequest,
        cache: &TileCache,
        buffer: Vec<u8>,
    ) -> RenderedView {
        let cpu = |buffer| {
            update_status(|status| status.cpu_frames += 1);
            crate::render_view_into(
                request.width,
                request.height,
                &request.basis,
                &request.cfg,
                cache,
                buffer,
            )
        };
        // Relief: the CPU draws it, from where the GPU's walk through the open air
        // ended (roadmap D1).
        #[cfg(feature = "gpu")]
        if request.cfg.terrain.is_some() && self.preference != BackendPreference::Cpu {
            self.initialize_with(crate::gpu::GpuRenderer::new);
            let started = std::time::Instant::now();
            let walks = self.relief_timing.walks(request.width * request.height);
            let starts =
                match self.gpu.as_mut().filter(|_| walks).map(|gpu| {
                    gpu_call(|| gpu.relief_starts(request, cache)).and_then(|result| result)
                }) {
                    Some(Ok(starts)) => starts,
                    Some(Err(error)) => {
                        self.device_failed(&error);
                        None
                    }
                    None => None,
                };
            let walked = starts.is_some();
            let walk_took = started.elapsed();
            // The walk's planes are also the frame's drawn ground, for the overlays.
            let starts = starts.map(std::sync::Arc::new);
            let depth = |planes: Option<std::sync::Arc<Vec<f32>>>| {
                planes
                    .and_then(|planes| {
                        crate::overlays::DepthMap::new(request.width, request.height, planes)
                    })
                    .map(std::sync::Arc::new)
            };
            // The GPU draws the frame from its walk where it can (roadmap D8, G4), the
            // CPU what it leaves. Also below `MIN_AUTO_GPU_PIXELS`, which is for flat
            // frames: relief frames while moving are that small (0.28 of the window),
            // and at 616×360 on an M1 the GPU drew them in 3–8 ms after the walk, the
            // CPU in 18–23 ms (2026-11-24).
            let mut buffer = buffer;
            if let (Some(planes), Some(gpu)) = (starts.as_deref(), self.gpu.as_mut())
                && crate::gpu::GpuRenderer::supports(request)
            {
                match gpu_call(|| gpu.render_relief(request, cache, buffer, planes))
                    .unwrap_or_else(|error| Err((error, Vec::new(), true)))
                {
                    Ok((frame, held)) => {
                        gpu.relief_frame_held(held);
                        let pixels = request.width * request.height;
                        let megapixels = pixels as f64 / 1e6;
                        self.relief_timing.note(
                            true,
                            started.elapsed().as_secs_f64() / megapixels.max(1e-6),
                            pixels,
                        );
                        update_status(|status| status.gpu_frames += 1);
                        log_relief_frame(
                            request,
                            "GPU",
                            walk_took,
                            started.elapsed(),
                            gpu.relief_uploads,
                            Some((gpu.last_draw, gpu.relief_sun_unknown)),
                        );
                        self.depth = depth(starts);
                        return frame;
                    }
                    Err((error, returned, permanent)) => {
                        buffer = returned;
                        if permanent {
                            self.device_failed(&error);
                        }
                    }
                }
            }
            update_status(|status| status.cpu_frames += 1);
            let (frame, held) = crate::render_relief_view_into(
                request.width,
                request.height,
                &request.basis,
                &request.cfg,
                cache,
                buffer,
                starts.as_deref().map(Vec::as_slice),
            );
            self.depth = depth(starts);
            let gave_up = self
                .gpu
                .as_ref()
                .filter(|gpu| walks && !walked && gpu.relief_ran)
                .map(|gpu| {
                    format!(
                        "CPU after a walk that did not help ({:.0} % of its rays without their tiles)",
                        100.0 * gpu.relief_untrusted
                    )
                });
            log_relief_frame(
                request,
                gave_up
                    .as_deref()
                    .unwrap_or(if walked { "CPU from the walk" } else { "CPU" }),
                walk_took,
                started.elapsed(),
                self.gpu.as_ref().map_or(0, |gpu| gpu.relief_uploads),
                None,
            );
            if let Some(gpu) = &mut self.gpu {
                gpu.relief_frame_held(held);
                // A walk that ran but did not help (more than half its rays without
                // the tiles they needed) is what trying the walk cost here: the CPU
                // drew the frame after it. On a PowerVR phone over close mountains
                // with tiles still arriving, that was most frames, 10–60 ms of walk
                // before 60–110 ms of CPU, and uncounted, the walk was tried every
                // frame (2026-11-26). Frames before the walk had tiles say nothing.
                let tried = walked || (walks && gpu.relief_ran);
                if tried || !walks {
                    let pixels = request.width * request.height;
                    let megapixels = pixels as f64 / 1e6;
                    self.relief_timing.note(
                        tried,
                        started.elapsed().as_secs_f64() / megapixels.max(1e-6),
                        pixels,
                    );
                }
            }
            return frame;
        }
        #[cfg(feature = "gpu")]
        if self.preference == BackendPreference::Cpu
            || (self.preference == BackendPreference::Auto
                && request.width.saturating_mul(request.height) < MIN_AUTO_GPU_PIXELS)
            || !crate::gpu::GpuRenderer::supports(request)
        {
            return cpu(buffer);
        }
        #[cfg(feature = "gpu")]
        self.initialize_with(crate::gpu::GpuRenderer::new);
        #[cfg(not(feature = "gpu"))]
        {
            if !self.initialized && self.preference != BackendPreference::Cpu {
                globecraft_data::log_line!(
                    "GPU support is disabled in this build; using CPU renderer"
                );
            }
            self.initialized = true;
        }
        #[cfg(feature = "gpu")]
        if let Some(gpu) = &mut self.gpu {
            let started = std::time::Instant::now();
            let result = gpu_call(|| gpu.render(request, cache, buffer))
                .unwrap_or_else(|error| Err((error, Vec::new(), true)));
            let took = started.elapsed();
            match result {
                Ok(frame) => {
                    self.capacity_warning = false;
                    self.clear_start_marker();
                    let frames = {
                        let mut frames = 0;
                        update_status(|status| {
                            status.gpu_frames += 1;
                            frames = status.gpu_frames;
                        });
                        frames
                    };
                    // The first frames, and any slow one, for bug reports.
                    if frames <= LOGGED_GPU_FRAMES || took >= SLOW_GPU_FRAME {
                        globecraft_data::log_line!(
                            "GPU frame {frames}: {}×{} in {:.1} ms",
                            request.width,
                            request.height,
                            took.as_secs_f64() * 1000.0
                        );
                    }
                    return frame;
                }
                Err((error, buffer, permanent)) => {
                    globecraft_data::log_line!(
                        "GPU frame {}×{} refused after {:.1} ms",
                        request.width,
                        request.height,
                        took.as_secs_f64() * 1000.0
                    );
                    if permanent {
                        self.device_failed(&error);
                    } else if !self.capacity_warning {
                        globecraft_data::log_line!("GPU renderer using CPU fallback: {error}");
                    }
                    self.capacity_warning = true;
                    return cpu(buffer);
                }
            }
        }
        cpu(buffer)
    }
}

#[cfg(all(test, feature = "gpu"))]
mod relief_timing_tests {
    use super::*;

    const PIXELS: usize = 1920 * 1200;

    /// Draws `frames` relief frames of `pixels` the way `timing` says, each taking
    /// `with` or `without` seconds per megapixel.
    fn drive(timing: &mut ReliefTiming, with: f64, without: f64, frames: u32, pixels: usize) {
        for _ in 0..frames {
            let walks = timing.walks(pixels);
            timing.note(walks, if walks { with } else { without }, pixels);
        }
    }

    /// The CPU marches alone while it beats the walk's last frame, and the frame after
    /// one of its frames is slower walks again (a PowerVR phone, 2026-11-26: the walk's
    /// frames 245 ms per megapixel everywhere, the CPU's 75 over flat land, 450–1000
    /// over mountains).
    #[test]
    fn the_cheaper_way_draws_as_the_view_changes() {
        let mut timing = ReliefTiming::default();
        drive(&mut timing, 0.245, 0.075, 3, PIXELS);
        assert!(timing.cpu_for_now && !timing.settled);
        let walked = timing.walked;
        drive(&mut timing, 0.245, 0.075, 10 * RELIEF_CHECK_EVERY, PIXELS);
        assert_eq!(
            timing.walked, walked,
            "no walk three times the CPU's is tried"
        );
        // Mountains: one CPU frame, then the walk.
        drive(&mut timing, 0.245, 0.7, 1, PIXELS);
        assert!(timing.cpu_for_now && timing.walks(PIXELS));
        drive(&mut timing, 0.245, 0.7, 1, PIXELS);
        assert!(!timing.cpu_for_now);
        // Each check over mountains costs one CPU frame; three clear ones settle it.
        let walked = timing.walked;
        drive(&mut timing, 0.245, 0.7, 3 * RELIEF_CHECK_EVERY, PIXELS);
        assert!(timing.settled && timing.walked - walked >= 3 * RELIEF_CHECK_EVERY - 3);
        drive(&mut timing, 0.245, 0.075, 10 * RELIEF_CHECK_EVERY, PIXELS);
        assert!(!timing.cpu_for_now);
    }

    /// Close mountains with tiles arriving (a PowerVR phone, 2026-11-26): trying the
    /// walk cost more than the CPU alone, so the CPU draws; every `RELIEF_CHECK_EVERY`th
    /// frame tries the walk again, and once it got cheaper than the CPU, it walks.
    #[test]
    fn a_walk_that_got_cheaper_is_found() {
        let mut timing = ReliefTiming::default();
        drive(&mut timing, 0.45, 0.35, 3, PIXELS);
        assert!(timing.cpu_for_now);
        let walked = timing.walked;
        drive(&mut timing, 0.45, 0.35, 5 * RELIEF_CHECK_EVERY, PIXELS);
        assert!(timing.cpu_for_now);
        assert_eq!(timing.walked - walked, 5, "one tried walk in each check");
        drive(&mut timing, 0.2, 0.35, RELIEF_CHECK_EVERY, PIXELS);
        assert!(!timing.cpu_for_now && timing.walks(PIXELS));
    }

    /// Where the walk pays everywhere (an M1), one decisive check settles it; a narrow
    /// win keeps the checks going; a software adapter's walk is tried twice, then the
    /// CPU keeps the frames.
    #[test]
    fn the_relief_walk_stays_where_it_pays() {
        let mut decisive = ReliefTiming::default();
        drive(&mut decisive, 0.1, 0.35, 3, PIXELS);
        assert!(decisive.settled && decisive.walks(PIXELS));
        let mut narrow = ReliefTiming::default();
        drive(&mut narrow, 0.1, 0.12, 5 * RELIEF_CHECK_EVERY, PIXELS);
        assert!(!narrow.settled && !narrow.cpu_for_now && narrow.wins == 0);
        let mut software = ReliefTiming::default();
        drive(&mut software, 13.0, 0.3, 10 * RELIEF_CHECK_EVERY, PIXELS);
        assert!(software.cpu_for_now && software.walked == 2);
    }

    /// The first walked frame (the walk's setup) is not timed, nor is a frame set
    /// against the walk at another size; a size without a timed walk walks to learn
    /// it, without leaving the CPU.
    #[test]
    fn the_walks_setup_and_other_sizes_are_not_compared() {
        // M1, 2026-11-24: the first walked frame 1238 ms per megapixel with the setup,
        // the frame without the walk 580; the walk itself 4–6 times faster after it.
        let mut fresh = ReliefTiming::default();
        fresh.note(true, 1.238, PIXELS);
        assert!(fresh.walks(PIXELS) && fresh.walk_cost(PIXELS).is_none());
        fresh.note(true, 0.12, PIXELS);
        assert!(!fresh.walks(PIXELS));
        fresh.note(false, 0.58, PIXELS);
        assert!(!fresh.cpu_for_now && fresh.wins == 1);
        let small = 616 * 386;
        let mut sizes = ReliefTiming::default();
        sizes.note(true, 0.2, PIXELS);
        sizes.note(true, 0.2, PIXELS);
        sizes.note(false, 0.05, small);
        assert!(!sizes.cpu_for_now && sizes.wins == 0);
        sizes.note(false, 0.05, PIXELS);
        assert!(sizes.cpu_for_now && sizes.walks(small));
        sizes.note(true, 0.3, small);
        assert!(sizes.cpu_for_now && !sizes.walks(PIXELS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_gpu_is_the_default_unless_turned_off_or_overridden() {
        assert_eq!(
            BackendPreference::resolve(None, true, true),
            BackendPreference::Auto
        );
        assert_eq!(
            BackendPreference::resolve(None, false, true),
            BackendPreference::Cpu
        );
        assert_eq!(
            BackendPreference::resolve(None, true, false),
            BackendPreference::Cpu
        );
        assert_eq!(
            BackendPreference::resolve(Some("cpu"), true, true),
            BackendPreference::Cpu
        );
        assert_eq!(
            BackendPreference::resolve(Some("gpu"), false, false),
            BackendPreference::Gpu
        );
    }
    #[test]
    fn gpu_rendering_requires_explicit_opt_in() {
        assert_eq!(
            BackendPreference::from_setting(None),
            BackendPreference::Cpu
        );
        assert_eq!(
            BackendPreference::from_setting(Some("cpu")),
            BackendPreference::Cpu
        );
        assert_eq!(
            BackendPreference::from_setting(Some("invalid")),
            BackendPreference::Cpu
        );
        assert_eq!(
            BackendPreference::from_setting(Some("auto")),
            BackendPreference::Auto
        );
        assert_eq!(
            BackendPreference::from_setting(Some("gpu")),
            BackendPreference::Gpu
        );
    }
    #[test]
    fn forced_cpu_never_initializes_a_device() {
        let renderer = FrameRenderer::new(BackendPreference::Cpu);
        #[cfg(feature = "gpu")]
        let mut renderer = renderer;
        #[cfg(feature = "gpu")]
        renderer.initialize_with(|| panic!("forced CPU must skip initialization"));
        assert_eq!(renderer.preference, BackendPreference::Cpu);
        #[cfg(feature = "gpu")]
        assert!(renderer.initialized);
        #[cfg(feature = "gpu")]
        assert!(renderer.gpu.is_none());
    }
}

#[cfg(all(test, feature = "gpu"))]
mod fallback_tests {
    use super::*;
    #[test]
    fn gpu_calls_preserve_results_and_report_render_panics() {
        assert_eq!(gpu_call(|| 42), Ok(42));
        assert_eq!(
            gpu_call(|| panic!("simulated driver render panic")),
            Err::<(), _>("GPU backend panicked: simulated driver render panic".into())
        );
    }
    #[test]
    fn initialization_panic_disables_gpu_without_killing_the_frame_worker() {
        let mut renderer = FrameRenderer::new(BackendPreference::Gpu);
        renderer.initialize_with(|| panic!("simulated driver initialization panic"));
        assert!(renderer.initialized);
        assert!(renderer.gpu.is_none());
        renderer.initialize_with(|| panic!("failed initialization must not repeat"));
    }
    fn marker(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("globecraft-{name}-{}", std::process::id()))
    }
    #[test]
    fn a_start_that_never_finished_is_not_repeated() {
        let path = marker("gpu-start-left");
        std::fs::write(&path, MARKER_VERSION).unwrap();
        let mut renderer =
            FrameRenderer::new(BackendPreference::Auto).with_start_marker(path.clone());
        renderer.initialize_with(|| panic!("a start that crashed last time must not run"));
        assert!(renderer.gpu.is_none());
        assert!(path.exists(), "the marker stays until the user asks again");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn a_marker_from_another_version_or_a_clean_failure_allows_the_next_start() {
        let path = marker("gpu-start-old");
        std::fs::write(&path, "0.0.1-older").unwrap();
        let mut renderer =
            FrameRenderer::new(BackendPreference::Auto).with_start_marker(path.clone());
        let mut started = false;
        renderer.initialize_with(|| {
            started = true;
            // Written before the device is created, so a crash here leaves it.
            assert_eq!(std::fs::read_to_string(&path).unwrap(), MARKER_VERSION);
            Err("test adapter is unavailable".into())
        });
        assert!(started);
        assert!(
            !path.exists(),
            "a start that failed cleanly leaves no marker"
        );
    }
    #[test]
    fn a_failed_device_is_replaced_a_bounded_number_of_times() {
        let mut renderer = FrameRenderer::new(BackendPreference::Gpu);
        renderer.initialized = true;
        for _ in 0..MAX_GPU_RESTARTS {
            renderer.device_failed("GPU device lost");
            assert!(renderer.restart_at.is_some());
            renderer.restart_at = None;
        }
        renderer.device_failed("GPU device lost");
        assert!(renderer.restart_at.is_none(), "no more replacements");
    }
    #[test]
    fn initialization_failure_disables_gpu_without_retrying_each_frame() {
        let mut renderer = FrameRenderer::new(BackendPreference::Auto);
        renderer.initialize_with(|| Err("test adapter is unavailable".into()));
        assert!(renderer.initialized);
        assert!(renderer.gpu.is_none());
        renderer.initialize_with(|| panic!("failed initialization must not repeat"));
    }
}

#[cfg(all(test, feature = "gpu"))]
mod native_fallback_tests {
    use super::*;
    #[test]
    #[ignore = "requires native GPU; run explicitly and alone with --test-threads=1"]
    fn gpu_device_loss_publishes_a_cpu_frame() {
        struct Fixture {
            root: std::path::PathBuf,
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.root).unwrap();
            }
        }
        let root = std::env::temp_dir().join(format!(
            "globecraft-gpu-fallback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture = Fixture { root };
        let runtime = globecraft_data::runtime::ServiceRuntime::new().unwrap();
        let cache = TileCache::with_cache_dir(
            || {},
            runtime.handle(),
            fixture.root.join("globecraft_tiles"),
        );
        let gpu = crate::gpu::GpuRenderer::new().unwrap();
        gpu.destroy_for_test();
        let mut renderer = FrameRenderer::new(BackendPreference::Gpu);
        renderer.initialized = true;
        renderer.gpu = Some(gpu);
        let cam = globecraft_core::camera::Cam {
            lat: -15.0,
            lon: -50.0,
            heading: 25.0,
            alt: 14000.0,
            tilt: 0.0,
            look_up: 0.0,
        };
        let request = RenderRequest {
            width: 65,
            height: 49,
            cam,
            basis: cam.basis(65.0 / 49.0),
            cfg: crate::RenderCfg {
                sky: None,
                provider: globecraft_data::tiles::Provider::Satellite,
                terrain: None,
                rel: 0,
                grid: false,
                grid_step: 10.0,
                atmo: true,
                sun: None,
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
        let fallback = renderer.render(&request, &cache, Vec::new());
        let expected = crate::render_view_into(
            request.width,
            request.height,
            &request.basis,
            &request.cfg,
            &cache,
            Vec::new(),
        );
        assert_eq!(fallback.pixels, expected.pixels);
        assert_eq!(
            fallback
                .required_tiles()
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            expected.required_tiles().into_iter().collect()
        );
        assert!(renderer.gpu.is_none(), "a failed device is not used again");
        assert_eq!(
            renderer.render(&request, &cache, Vec::new()).pixels,
            expected.pixels
        );
        // Once the delay has passed, a new device takes over.
        assert!(renderer.gpu.is_none(), "no new device before the delay");
        renderer.restart_at = Some(std::time::Instant::now());
        renderer.render(&request, &cache, Vec::new());
        assert!(renderer.gpu.is_some(), "a lost device is replaced");
        drop(renderer);
        drop(cache);
        drop(runtime);
        drop(fixture);
    }
}
