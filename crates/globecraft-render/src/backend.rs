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
    pub fn render(
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
            let result = gpu_call(|| gpu.render(request, cache, buffer))
                .unwrap_or_else(|error| Err((error, Vec::new(), true)));
            match result {
                Ok(frame) => {
                    self.capacity_warning = false;
                    self.clear_start_marker();
                    update_status(|status| status.gpu_frames += 1);
                    return frame;
                }
                Err((error, buffer, permanent)) => {
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
