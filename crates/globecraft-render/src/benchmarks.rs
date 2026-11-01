//! Manual CPU-frame comparison using isolated disk-cache fixtures and production APIs.
use super::*;
mod kernels;
mod replay;
use globecraft_core::{camera::Cam, math::now_unix};
use std::{
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant},
};

const FRAME_WIDTH_PX: usize = 512;
const FRAME_HEIGHT_PX: usize = 384;
const BENCH_FRAMES: usize = 200;
const BENCH_ROUNDS: usize = 5;
const FIXTURE_TILE_SIZE_PX: u32 = 256;

struct FixtureCache {
    root: PathBuf,
}
impl FixtureCache {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "globecraft-render-bench-{}-{}",
            std::process::id(),
            now_unix()
        ));
        Self { root }
    }
    /// Writes satellite tiles (zooms 0–4), and Terrarium elevation tiles too when `terrain`
    /// (zooms 0–3: 85 tiles of 512 px, 85 MiB, so both sets fit the decoded-cache budget).
    fn prepare(&self, terrain: bool) -> Vec<TileKey> {
        let mut keys = Vec::new();
        if terrain {
            // Rolling hills up to ~2.5 km, encoded as Terrarium: (R * 256 + G + B / 256) - 32768.
            let dem = image::RgbaImage::from_fn(512, 512, |x, y| {
                let h = 1250.0 + 1250.0 * ((x as f64 / 23.0).sin() * (y as f64 / 31.0).cos());
                let v = h + 32768.0;
                let r = (v / 256.0).floor();
                let g = (v - r * 256.0).floor();
                image::Rgba([r as u8, g as u8, 0, 255])
            });
            let mut dem_png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(dem)
                .write_to(&mut dem_png, image::ImageFormat::Png)
                .unwrap();
            for z in 0..=3u8 {
                for x in 0..1u32 << z {
                    for y in 0..1u32 << z {
                        let path = self
                            .root
                            .join("globecraft_tiles/mapterhorn_dem")
                            .join(z.to_string())
                            .join(x.to_string())
                            .join(format!("{y}.tile"));
                        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                        std::fs::write(path, dem_png.get_ref()).unwrap();
                        keys.push(make_key(Provider::TerrainDem, z, x, y, 0));
                    }
                }
            }
        }
        let image =
            image::RgbaImage::from_fn(FIXTURE_TILE_SIZE_PX, FIXTURE_TILE_SIZE_PX, |x, y| {
                let value =
                    (x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)).wrapping_mul(2246822519);
                image::Rgba([value as u8, (value >> 8) as u8, (value >> 16) as u8, 255])
            });
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        for z in 0..=4 {
            for x in 0..1u32 << z {
                for y in 0..1u32 << z {
                    let path = self
                        .root
                        .join("globecraft_tiles/esri_sat")
                        .join(z.to_string())
                        .join(x.to_string())
                        .join(format!("{y}.tile"));
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, encoded.get_ref()).unwrap();
                    keys.push(make_key(Provider::Satellite, z, x, y, 0));
                }
            }
        }
        keys
    }
}
impl Drop for FixtureCache {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
#[ignore = "manual warm CPU-frame benchmark; run alone with --ignored --nocapture --test-threads=1"]
fn warm_cpu_frame_time() {
    warm_cpu_frame_for_model(
        globecraft_core::earth_model::EarthModel::PerfectSphere,
        false,
    );
}
#[test]
#[ignore = "manual 3D-terrain CPU-frame benchmark; run alone with --ignored --nocapture --test-threads=1"]
fn warm_terrain_cpu_frame_time() {
    warm_cpu_frame_for_model(globecraft_core::earth_model::EarthModel::Wgs84, true);
}
#[test]
#[ignore = "manual ellipsoid CPU-frame benchmark; run alone with --ignored --nocapture --test-threads=1"]
fn warm_ellipsoid_cpu_frame_time() {
    warm_cpu_frame_for_model(globecraft_core::earth_model::EarthModel::Wgs84, false);
}

fn warm_cpu_frame_for_model(model: globecraft_core::earth_model::EarthModel, terrain: bool) {
    let fixture = FixtureCache::new();
    let keys = fixture.prepare(terrain);
    let runtime = globecraft_data::runtime::ServiceRuntime::new().unwrap();
    let cache = TileCache::with_cache_dir(
        || {},
        runtime.handle(),
        fixture.root.join("globecraft_tiles"),
    );
    let started = Instant::now();
    let mut warmup = globecraft_data::tiles::TileWarmup::startup_satellite();
    while warmup.pump(&cache) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "fixture warmup must finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    while keys
        .iter()
        .any(|&key| !matches!(cache.lookup(key), Lookup::Ready(_)))
    {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "fixture tiles must load from isolated disk cache"
        );
        // Elevation tiles are not part of the overview warmup: ask for them.
        for &key in &keys {
            if matches!(cache.lookup(key), Lookup::Absent) {
                cache.request(key);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let basis = Cam {
        lat: -15.0,
        lon: -50.0,
        heading: 25.0,
        alt: 14000.0,
        tilt: 0.0,
    }
    .basis_for_model(FRAME_WIDTH_PX as f64 / FRAME_HEIGHT_PX as f64, model);
    let config = RenderCfg {
        sky: None,
        provider: Provider::Satellite,
        terrain: terrain.then(|| TerrainCfg::new(1.5)),
        rel: 0,
        grid: true,
        grid_step: 10.0,
        atmo: true,
        sun: Some(globecraft_core::math::ll_to_v3(20.0, 45.0)),
        lod_scale: 1.0,
        level_blend: true,
        fixed_level: None,
        night_lights: false,
        stars: true,
    };
    // Global overview warming must also cover parts of the globe that were
    // behind the camera at startup, including the dateline and poles.
    for lat in [-90.0, -15.0, 0.0, 90.0] {
        for lon in [-180.0, -90.0, 0.0, 90.0, 180.0] {
            let rotated = Cam {
                lat,
                lon,
                heading: 25.0,
                alt: 14_000.0,
                tilt: 0.0,
            }
            .basis_for_model(FRAME_WIDTH_PX as f64 / FRAME_HEIGHT_PX as f64, model);
            black_box(render_frame(
                FRAME_WIDTH_PX,
                FRAME_HEIGHT_PX,
                &rotated,
                &config,
                &cache,
            ));
            assert_eq!(
                cache.inflight(),
                0,
                "overview rotations must stay in memory"
            );
        }
    }
    black_box(render_frame(
        FRAME_WIDTH_PX,
        FRAME_HEIGHT_PX,
        &basis,
        &config,
        &cache,
    ));
    let mut samples = Vec::new();
    for _ in 0..BENCH_ROUNDS {
        let started = Instant::now();
        for _ in 0..BENCH_FRAMES {
            black_box(render_frame(
                FRAME_WIDTH_PX,
                FRAME_HEIGHT_PX,
                &basis,
                &config,
                &cache,
            ));
        }
        samples.push(started.elapsed().as_secs_f64() * 1000.0 / BENCH_FRAMES as f64);
    }
    assert_eq!(
        cache.inflight(),
        0,
        "warm frame must not request network tiles"
    );
    samples.sort_by(f64::total_cmp);
    println!(
        "CPU-frame samples (ms): {samples:?}; Rayon threads: {}",
        rayon::current_num_threads()
    );
    println!(
        "Warm 512x384 CPU frame{}: median {:.3} ms; 341 256x256 RGBA fixture tiles (85.25 MiB decoded), 5 rounds",
        if terrain { " with 3D terrain" } else { "" },
        samples[BENCH_ROUNDS / 2]
    );
}

#[cfg(feature = "gpu")]
#[test]
#[ignore = "manual native GPU/CPU comparison; run alone with --ignored --nocapture --test-threads=1"]
fn warm_gpu_frame_time() {
    let fixture = FixtureCache::new();
    let keys = fixture.prepare(false);
    let runtime = globecraft_data::runtime::ServiceRuntime::new().unwrap();
    let cache = TileCache::with_cache_dir(
        || {},
        runtime.handle(),
        fixture.root.join("globecraft_tiles"),
    );
    let started = Instant::now();
    let mut warmup = globecraft_data::tiles::TileWarmup::startup_satellite();
    while warmup.pump(&cache)
        || keys
            .iter()
            .any(|&key| !matches!(cache.lookup(key), Lookup::Ready(_)))
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut gpu = crate::gpu::GpuRenderer::new().unwrap();
    for (w, h) in [(512, 384), (2000, 1488)] {
        let cam = Cam {
            lat: -15.0,
            lon: -50.0,
            heading: 25.0,
            alt: 14000.0,
            tilt: 0.0,
        };
        let request = crate::worker::RenderRequest {
            width: w,
            height: h,
            cam,
            basis: cam.basis(w as f64 / h as f64),
            cfg: RenderCfg {
                sky: None,
                provider: Provider::Satellite,
                terrain: None,
                rel: 0,
                grid: false,
                grid_step: 10.0,
                atmo: true,
                sun: Some(globecraft_core::math::ll_to_v3(20.0, 45.0)),
                lod_scale: 1.0,
                level_blend: true,
                fixed_level: None,
                night_lights: false,
                stars: true,
            },
            overlays: None,
            aircraft: None,
            live_layers: None,
            presentation: 0,
            compare: None,
        };
        let mut cpu_times = Vec::new();
        let mut gpu_times = Vec::new();
        let mut cpu_buffer = Vec::new();
        let mut gpu_buffer = Vec::new();
        for round in 0..16 {
            let start = Instant::now();
            let cpu = render_view_into(w, h, &request.basis, &request.cfg, &cache, cpu_buffer);
            let cpu_time = start.elapsed();
            let start = Instant::now();
            let accelerated = gpu
                .render(&request, &cache, gpu_buffer)
                .unwrap_or_else(|(error, _, _)| panic!("{error}"));
            let gpu_time = start.elapsed();
            assert_eq!(
                cpu.waiting
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>(),
                accelerated
                    .waiting
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
            );
            if round == 0 || round == 15 {
                eprintln!("round {round}:");
                let mut histogram = [0usize; 256];
                for (a, b) in cpu.pixels.iter().zip(&accelerated.pixels) {
                    histogram[a.abs_diff(*b) as usize] += 1;
                }
                let maximum = histogram.iter().rposition(|count| *count > 0).unwrap();
                eprintln!(
                    "{w}x{h} channel difference max {maximum}; channels above one step {}",
                    histogram[2..].iter().sum::<usize>()
                );
                let worst = cpu
                    .pixels
                    .iter()
                    .zip(&accelerated.pixels)
                    .position(|(a, b)| a.abs_diff(*b) as usize == maximum)
                    .unwrap();
                // Where the larger differences are, by row band (tenths of the frame).
                let mut bands = [0usize; 10];
                for (i, (a, b)) in cpu.pixels.iter().zip(&accelerated.pixels).enumerate() {
                    if a.abs_diff(*b) > 1 {
                        bands[(i / 4 / w * 10 / h).min(9)] += 1;
                    }
                }
                eprintln!("differences over one level by row band: {bands:?}");
                if std::env::var_os("GLOBECRAFT_GPU_DEBUG").is_some() {
                    let rays = crate::rays::RayCaster::new(w, h, &request.basis);
                    let mut shown = 0;
                    for (i, (a, b)) in cpu.pixels.iter().zip(&accelerated.pixels).enumerate() {
                        if a.abs_diff(*b) <= 8 || i % 4 != 0 || shown >= 8 {
                            continue;
                        }
                        shown += 1;
                        let (x, y) = (i / 4 % w, i / 4 / w);
                        let ray = rays.cast_model::<false>(x, y)[0];
                        let surface = crate::pixel::surface::<false>(
                            x,
                            y,
                            w,
                            h,
                            &ray,
                            &request.cfg,
                            rays.model_geometry(),
                        );
                        let n = f64::from(1u32 << surface.z0);
                        let (tx, ty) = ((surface.u * n).floor(), (surface.v * n).floor());
                        let key = globecraft_data::tiles::make_key(
                            Provider::Satellite,
                            surface.z0 as u8,
                            tx as u32,
                            ty as u32,
                            0,
                        );
                        let samples = match cache.lookup(key) {
                            globecraft_data::tiles::Lookup::Ready(tile) => {
                                let exponent = 1.0 - f64::from(surface.frac);
                                let stride = exponent.exp2() as f32;
                                let (u, v) = (surface.u * n - tx, surface.v * n - ty);
                                format!(
                                    "tile {}x{} bilinear {:?} area {stride:.2} {:?}",
                                    tile.rgba().0,
                                    tile.rgba().1,
                                    tile.sample(u, v, 1.0) * 255.0,
                                    tile.sample(u, v, stride) * 255.0
                                )
                            }
                            _ => "tile not loaded".into(),
                        };
                        eprintln!(
                            "  ({x},{y}) cpu {:?} gpu {:?} z0 {} frac {:.4} {samples}",
                            &cpu.pixels[i..i + 3],
                            &accelerated.pixels[i..i + 3],
                            surface.z0,
                            surface.frac
                        );
                    }
                }
                eprintln!(
                    "worst pixel ({},{}), channel {}",
                    worst / 4 % w,
                    worst / 4 / w,
                    worst % 4
                );
            }
            // The GPU's f32 level of detail lands on the other side of the CPU's
            // filter and fade thresholds along thin contours of real imagery; the
            // picture must match to within quantization everywhere else.
            let differences: Vec<u8> = cpu
                .pixels
                .iter()
                .zip(&accelerated.pixels)
                .map(|(a, b)| a.abs_diff(*b))
                .filter(|d| *d > 1)
                .collect();
            assert!(
                std::env::var_os("GLOBECRAFT_GPU_DEBUG").is_some()
                    || (differences.len() as f64)
                        < cpu.pixels.len() as f64 * crate::gpu::IMAGERY_SHARE_OVER_ONE
                        && differences
                            .iter()
                            .all(|d| *d <= crate::gpu::IMAGERY_MAX_DIFFERENCE),
                "GPU pixels differ by more than one level in {} channels",
                differences.len()
            );
            cpu_buffer = cpu.pixels;
            gpu_buffer = accelerated.pixels;
            if round > 0 {
                cpu_times.push(cpu_time);
                gpu_times.push(gpu_time);
            }
        }
        cpu_times.sort();
        gpu_times.sort();
        eprintln!(
            "{w}x{h}: CPU median {:.3} ms; GPU including readback and sky repair median {:.3} ms (15 warm frames)",
            cpu_times[7].as_secs_f64() * 1000.0,
            gpu_times[7].as_secs_f64() * 1000.0
        );
    }
    // A decoded cache larger than the GPU atlas must not disable acceleration
    // when the actual view fits. Seed extra detail through the normal disk loader.
    let Lookup::Ready(root_tile) = cache.lookup(make_key(Provider::Satellite, 0, 0, 0, 0)) else {
        panic!("warm root tile");
    };
    let image = image::RgbaImage::from_raw(256, 256, root_tile.rgba().2.to_vec()).unwrap();
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let mut detail = Vec::new();
    for x in 0..16 {
        for y in 0..16 {
            let path = fixture
                .root
                .join("globecraft_tiles/esri_sat/5")
                .join(x.to_string())
                .join(format!("{y}.tile"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, encoded.get_ref()).unwrap();
            detail.push(make_key(Provider::Satellite, 5, x, y, 0));
        }
    }
    let started = Instant::now();
    while detail
        .iter()
        .any(|&key| !matches!(cache.lookup(key), Lookup::Ready(_)))
    {
        cache.prioritize_view(&detail);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "isolated detail loading"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(cache.ready_tiles(Provider::Satellite, 0).len() > 512);
    for (lat, lon) in [(-15.0, -50.0), (40.0, 80.0), (-30.0, 170.0)] {
        let cam = Cam {
            lat,
            lon,
            heading: 25.0,
            alt: 14000.0,
            tilt: 0.0,
        };
        let request = crate::worker::RenderRequest {
            width: 2000,
            height: 1488,
            cam,
            basis: cam.basis(2000.0 / 1488.0),
            cfg: RenderCfg {
                sky: None,
                provider: Provider::Satellite,
                terrain: None,
                rel: 0,
                grid: false,
                grid_step: 10.0,
                atmo: true,
                sun: Some(globecraft_core::math::ll_to_v3(20.0, 45.0)),
                lod_scale: 1.0,
                level_blend: true,
                fixed_level: None,
                night_lights: false,
                stars: true,
            },
            overlays: None,
            aircraft: None,
            live_layers: None,
            presentation: 0,
            compare: None,
        };
        let frame = gpu
            .render(&request, &cache, Vec::new())
            .unwrap_or_else(|(error, _, _)| {
                panic!("large decoded cache must remain GPU-renderable: {error}")
            });
        let cpu = render_view_into(
            request.width,
            request.height,
            &request.basis,
            &request.cfg,
            &cache,
            Vec::new(),
        );
        // Imagery: the same bounds as the warm views above (filter and fade
        // thresholds along thin contours).
        let differences: Vec<u8> = frame
            .pixels
            .iter()
            .zip(&cpu.pixels)
            .map(|(a, b)| a.abs_diff(*b))
            .filter(|d| *d > 1)
            .collect();
        eprintln!(
            "large cache ({lat},{lon}): {} channels over one level, max {}",
            differences.len(),
            differences.iter().max().copied().unwrap_or(0)
        );
        assert!(
            std::env::var_os("GLOBECRAFT_GPU_DEBUG").is_some()
                || (differences.len() as f64)
                    < cpu.pixels.len() as f64 * crate::gpu::IMAGERY_SHARE_OVER_ONE
                    && differences
                        .iter()
                        .all(|d| *d <= crate::gpu::IMAGERY_MAX_DIFFERENCE),
            "large-cache rotation pixel parity: {} channels over one level",
            differences.len()
        );
        assert_eq!(
            frame
                .waiting
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            cpu.waiting.into_iter().collect()
        );
    }
    eprintln!(
        "597 decoded tiles: GPU residency and pixel/dependency parity passed across three hemispheric views"
    );
    drop(cache);
    drop(runtime);
    drop(fixture);
}
