//! Screenshots rendered until every tile is at its target level. The picture is made
//! in strips from top to bottom, each finished (its tiles downloaded) before the next,
//! and handed on as it is done: an 8× picture never needs to fit in memory, and the
//! tile cache only holds one strip's tiles at a time.
//!
//! Stills render on their own small thread pool, so the view on screen keeps the
//! shared pool: rendering a still on it froze the globe for as long as the still
//! took (2026-10-25).
use super::*;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// About this many pixels per strip (16 MB of RGBA).
const STRIP_PIXELS: usize = 4_000_000;
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How often missing tiles are requested again (the cache admits a bounded number).
const REQUEST_INTERVAL: Duration = Duration::from_millis(400);
/// Downloads a still may hold at once (as in `TileCache::request_in_background`).
const BACKGROUND_INFLIGHT: usize = globecraft_data::tiles::MAX_INFLIGHT * 3 / 4;

/// How far a still has come, for the status line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StillProgress {
    /// Fraction of the picture finished, 0 to 1.
    pub done: f32,
    /// Tiles the current strip still waits for.
    pub missing: usize,
}

/// How a still ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StillEnd {
    /// Every row was handed on; `missing` tiles never arrived (shown coarser).
    Finished {
        missing: usize,
    },
    Cancelled,
}

/// The pool stills render on: half the cores, so the view keeps the rest.
fn still_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads((cores / 2).max(1))
            .thread_name(|index| format!("globecraft-still-{index}"))
            .build()
            .ok()
    })
    .as_ref()
}

/// Rows per strip for a picture `width` wide: whole tile batches, at least one.
fn strip_rows(width: usize) -> usize {
    let rows = STRIP_PIXELS / width.max(1);
    (rows / ROWS_PER_TILE_BATCH).max(1) * ROWS_PER_TILE_BATCH
}

/// Renders `width`×`height` from `basis` strip by strip, waiting for each strip's
/// tiles, and hands every finished strip to `sink` (whole rows, top to bottom). A
/// strip whose tiles stop arriving for `stall` is handed on as it is. `cancel` stops
/// it between polls.
#[allow(clippy::too_many_arguments)]
pub fn render_still<E>(
    width: usize,
    height: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    cache: &TileCache,
    stall: Duration,
    cancel: &AtomicBool,
    mut progress: impl FnMut(StillProgress),
    mut sink: impl FnMut(&[u8]) -> Result<(), E>,
) -> Result<StillEnd, E> {
    let rows_per_strip = strip_rows(width);
    let mut missing_total = 0;
    let mut first_row = 0;
    while first_row < height {
        let rows = first_row..(first_row + rows_per_strip).min(height);
        let strip = render_strip(
            width,
            height,
            rows.clone(),
            basis,
            cfg,
            cache,
            stall,
            cancel,
            &mut |missing| {
                progress(StillProgress {
                    done: first_row as f32 / height as f32,
                    missing,
                })
            },
        );
        cache.release_reserved();
        let Some((pixels, missing)) = strip else {
            return Ok(StillEnd::Cancelled);
        };
        sink(&pixels)?;
        missing_total += missing;
        first_row = rows.end;
    }
    progress(StillProgress {
        done: 1.0,
        missing: 0,
    });
    Ok(StillEnd::Finished {
        missing: missing_total,
    })
}

/// One strip, rendered again until its tiles have arrived (or stalled). None when
/// cancelled; otherwise the pixels and how many tiles never came.
#[allow(clippy::too_many_arguments)]
fn render_strip(
    width: usize,
    height: usize,
    rows: std::ops::Range<usize>,
    basis: &Basis,
    cfg: &RenderCfg,
    cache: &TileCache,
    stall: Duration,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize),
) -> Option<(Vec<u8>, usize)> {
    let render = || render_rows(width, height, rows.clone(), basis, cfg, cache);
    let mut last_arrival = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let view = match still_pool() {
            Some(pool) => pool.install(render),
            None => render(),
        };
        let required = view.required_tiles();
        progress(required.len());
        let failed = required.iter().filter(|&&key| cache.failed(key)).count();
        if required.len() == failed || last_arrival.elapsed() >= stall {
            return Some((view.pixels, required.len()));
        }
        cache.reserve(&required);
        let mut requested: Option<Instant> = None;
        let mut missing = required.len();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if requested.is_none_or(|at| at.elapsed() >= REQUEST_INTERVAL) {
                for &key in &required {
                    if matches!(cache.lookup(key), Lookup::Absent)
                        && !cache.request_in_background(key)
                        && cache.inflight() >= BACKGROUND_INFLIGHT
                    {
                        break;
                    }
                }
                requested = Some(Instant::now());
            }
            // A tile whose download failed is not waited for: the strip shows a
            // coarser level there rather than wait out the stall.
            let now_missing = required
                .iter()
                .filter(|&&key| {
                    matches!(cache.lookup(key), Lookup::Pending | Lookup::Absent)
                        && !cache.failed(key)
                })
                .count();
            if now_missing < missing {
                last_arrival = Instant::now();
                progress(now_missing);
            }
            missing = now_missing;
            if missing == 0 || last_arrival.elapsed() >= stall {
                break;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;

    fn offline_cache(
        name: &str,
    ) -> (
        globecraft_data::runtime::ServiceRuntime,
        Arc<TileCache>,
        std::path::PathBuf,
    ) {
        let runtime = globecraft_data::runtime::ServiceRuntime::new().unwrap();
        let directory =
            std::env::temp_dir().join(format!("globecraft-still-{name}-{}", std::process::id()));
        // Nothing listens on the discard port: every download fails at once.
        let cache = TileCache::with_fixture_source(
            || {},
            runtime.handle(),
            directory.clone(),
            "http://127.0.0.1:9",
        );
        (runtime, cache, directory)
    }

    fn view() -> (Basis, RenderCfg) {
        let basis = Cam {
            lat: 10.0,
            lon: 20.0,
            heading: 0.0,
            alt: 5_000.0,
            tilt: 0.0,
        }
        .basis(4.0 / 3.0);
        let cfg = RenderCfg {
            sky: None,
            provider: Provider::Satellite,
            terrain: None,
            rel: 0,
            grid: false,
            grid_step: 10.0,
            atmo: true,
            sun: None,
            lod_scale: 1.0,
            level_blend: false,
        };
        (basis, cfg)
    }

    #[test]
    fn a_still_hands_on_every_row_in_strips_when_tiles_never_arrive() {
        let (_runtime, cache, directory) = offline_cache("stall");
        let (basis, cfg) = view();
        // Narrow enough that the picture takes several strips.
        let (width, height) = (STRIP_PIXELS / 40, 100);
        let mut rows = 0;
        let mut strips = 0;
        let started = Instant::now();
        let end = render_still(
            width,
            height,
            &basis,
            &cfg,
            &cache,
            Duration::from_millis(200),
            &AtomicBool::new(false),
            |_| {},
            |pixels: &[u8]| -> Result<(), ()> {
                assert_eq!(pixels.len() % (width * RGBA_CHANNELS), 0);
                rows += pixels.len() / (width * RGBA_CHANNELS);
                strips += 1;
                Ok(())
            },
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert_eq!(rows, height);
        assert!(strips > 1);
        assert!(matches!(end, StillEnd::Finished { missing } if missing > 0));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_cancelled_still_stops_without_handing_on_rows() {
        let (_runtime, cache, directory) = offline_cache("cancel");
        let (basis, cfg) = view();
        let end = render_still(
            64,
            48,
            &basis,
            &cfg,
            &cache,
            Duration::from_secs(60),
            &AtomicBool::new(true),
            |_| {},
            |_: &[u8]| -> Result<(), ()> { panic!("a cancelled still wrote rows") },
        )
        .unwrap();
        assert_eq!(end, StillEnd::Cancelled);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn strips_are_whole_tile_batches() {
        assert_eq!(strip_rows(16_000) % ROWS_PER_TILE_BATCH, 0);
        assert!(strip_rows(16_000) * 16_000 <= STRIP_PIXELS);
        assert_eq!(strip_rows(10_000_000), ROWS_PER_TILE_BATCH);
    }
}
