//! Still frames rendered until every tile is at its target level: the high-quality
//! screenshots (Plus, Pro, Pro Max). A render lists the tiles it lacks; they are
//! reserved (a new view's priorities do not retire them) and requested, and once they
//! have arrived the frame renders again, until nothing is missing or time runs out.
use super::*;
use std::time::{Duration, Instant};

/// How often the waiting loop re-admits requests (the cache admits a bounded number).
const READMIT_INTERVAL: Duration = Duration::from_millis(400);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A finished still: its pixels and how many tiles were still missing at the deadline.
pub struct Still {
    pub pixels: Vec<u8>,
    pub missing: usize,
}

/// Renders `width`×`height` from `basis` until every tile it samples has loaded or
/// `deadline` passes. `progress` hears how many tiles are still missing.
pub fn render_still(
    width: usize,
    height: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    cache: &TileCache,
    deadline: Duration,
    progress: impl Fn(usize),
) -> Still {
    let started = Instant::now();
    loop {
        let view = render_view(width, height, basis, cfg, cache);
        let required = view.required_tiles();
        progress(required.len());
        if required.is_empty() || started.elapsed() >= deadline {
            cache.release_reserved();
            return Still {
                pixels: view.pixels,
                missing: required.len(),
            };
        }
        cache.reserve(&required);
        let mut admitted = Instant::now() - READMIT_INTERVAL;
        loop {
            if admitted.elapsed() >= READMIT_INTERVAL {
                cache.prioritize_view(&required);
                admitted = Instant::now();
            }
            let missing = required
                .iter()
                .filter(|&&key| matches!(cache.lookup(key), Lookup::Pending | Lookup::Absent))
                .count();
            progress(missing);
            if missing == 0 || started.elapsed() >= deadline {
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

    #[test]
    fn a_still_returns_at_the_deadline_when_tiles_never_arrive() {
        let runtime = globecraft_data::runtime::ServiceRuntime::new().unwrap();
        let directory =
            std::env::temp_dir().join(format!("globecraft-still-{}", std::process::id()));
        // Nothing listens on the discard port: every download fails at once.
        let cache = TileCache::with_fixture_source(
            || {},
            runtime.handle(),
            directory.clone(),
            "http://127.0.0.1:9",
        );
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
        };
        let started = Instant::now();
        let still = render_still(
            64,
            48,
            &basis,
            &cfg,
            &cache,
            Duration::from_millis(300),
            |_| {},
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(still.pixels.len(), 64 * 48 * RGBA_CHANNELS);
        assert!(still.missing > 0);
        let _ = std::fs::remove_dir_all(directory);
    }
}
