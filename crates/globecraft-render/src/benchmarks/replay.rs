//! Streaming replay benchmark (roadmap 1.1): recorded camera paths against a local
//! fixture tile server that adds latency and shares one bandwidth limit between its
//! connections, the way a real tile host behaves. Each path starts with an empty
//! cache and runs in real time, with the production request path: the frame loop
//! renders at the motion or idle resolution the app uses, admits the frame's misses
//! with `TileCache::prioritize_view`, and pumps the overview and zoom-out warmups
//! when the view rests, as `globe.rs` does.
//!
//! Reported per path:
//! - frame time while moving (render plus admission);
//! - hole pixels: no level of the imagery loaded at all (the dark fill);
//! - time to complete (no holes) and time to sharp (every visible tile at its
//!   target level), both counted from the moment the camera stops;
//! - tiles and bytes downloaded, and wasted downloads: tiles fetched but never
//!   sampled by any frame (prefetch that a later frame does not use counts here too).
//!
//! Run alone, in release:
//! `cargo test -p globecraft-render --release replay -- --ignored --nocapture --test-threads=1`
//! `GLOBECRAFT_REPLAY_LATENCY_MS` and `GLOBECRAFT_REPLAY_KBPS` change the link.
//! `GLOBECRAFT_REPLAY_TERRAIN`, `GLOBECRAFT_REPLAY_ATMOSPHERE` and
//! `GLOBECRAFT_REPLAY_WGS84` (any value) turn on relief (served from a fixture
//! Terrarium tile), the atmosphere and the ellipsoid, as the app can draw them.
//! The two tilted paths (2026-10-26) look toward the horizon down to street level.
use super::*;
use globecraft_core::camera::Cam;
use std::collections::HashSet;
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const VIEWPORT_POINTS: (f32, f32) = (960.0, 600.0);
const FRAME_INTERVAL: Duration = Duration::from_millis(33);
/// How long a path may take to settle after the camera stops.
const SETTLE_LIMIT: Duration = Duration::from_secs(25);
const DEFAULT_LATENCY_MS: u64 = 60;
/// 32 Mbit/s, shared by all connections.
const DEFAULT_KBPS: u64 = 4_000;
const CHUNK_BYTES: usize = 8 * 1024;
/// Same as `OVERVIEW_WARMUP_ALTITUDE_KM` in the app.
const OVERVIEW_WARMUP_ALTITUDE_KM: f64 = 14_000.0;

#[derive(Clone, Copy)]
struct Link {
    latency: Duration,
    bytes_per_sec: f64,
}
impl Link {
    fn from_env() -> Self {
        let number = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        Self {
            latency: Duration::from_millis(number(
                "GLOBECRAFT_REPLAY_LATENCY_MS",
                DEFAULT_LATENCY_MS,
            )),
            bytes_per_sec: number("GLOBECRAFT_REPLAY_KBPS", DEFAULT_KBPS) as f64 * 1000.0,
        }
    }
}

#[derive(Default)]
struct Served {
    keys: Mutex<Vec<TileKey>>,
    bytes: AtomicU64,
}

/// A satellite-like JPEG: smooth colour fields with fine noise, about 20 KB, so
/// transfer and decode cost resemble real imagery.
fn fixture_tile() -> Vec<u8> {
    let image = image::RgbImage::from_fn(256, 256, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let noise = (x.wrapping_mul(1_664_525) ^ y.wrapping_mul(1_013_904_223))
            .wrapping_mul(2_246_822_519)
            >> 27;
        let base = |a: f32, b: f32| 90.0 + 50.0 * (fx / a).sin() * (fy / b).cos();
        image::Rgb([
            (base(17.0, 23.0) + noise as f32) as u8,
            (base(29.0, 13.0) + noise as f32) as u8,
            (base(41.0, 37.0) * 0.8 + noise as f32) as u8,
        ])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&image)
        .unwrap();
    out
}

/// A 512 px Terrarium elevation tile: rolling hills from 300 to 1,500 m.
fn fixture_dem_tile() -> Vec<u8> {
    let image = image::RgbaImage::from_fn(512, 512, |x, y| {
        let metres = 900.0 + 600.0 * (x as f64 / 40.0).sin() * (y as f64 / 55.0).cos();
        let value = metres + 32_768.0;
        image::Rgba([
            (value / 256.0) as u8,
            (value % 256.0) as u8,
            ((value.fract()) * 256.0) as u8,
            255,
        ])
    });
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

async fn serve(
    listener: tokio::net::TcpListener,
    body: Arc<Vec<u8>>,
    dem: Arc<Vec<u8>>,
    link: Link,
    served: Arc<Served>,
) {
    // When the shared link is free again.
    let link_free = Arc::new(tokio::sync::Mutex::new(tokio::time::Instant::now()));
    loop {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        let (imagery, dem, served, link_free) =
            (body.clone(), dem.clone(), served.clone(), link_free.clone());
        tokio::spawn(async move {
            let mut pending = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let Ok(read) = socket.read(&mut buffer).await else {
                    return;
                };
                if read == 0 {
                    return;
                }
                pending.extend_from_slice(&buffer[..read]);
                while let Some(end) = pending.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head: Vec<u8> = pending.drain(..end + 4).collect();
                    let line = String::from_utf8_lossy(&head);
                    let path = line.split_whitespace().nth(1).unwrap_or("");
                    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
                    if let [provider, z, x, y, release] = parts[..]
                        && let (Some(provider), Ok(z), Ok(x), Ok(y), Ok(release)) = (
                            Provider::from_code(provider),
                            z.parse(),
                            x.parse(),
                            y.parse(),
                            release.parse(),
                        )
                    {
                        served
                            .keys
                            .lock()
                            .unwrap()
                            .push(make_key(provider, z, x, y, release));
                    }
                    let body = if path.starts_with("/terrain_dem/") {
                        &dem
                    } else {
                        &imagery
                    };
                    tokio::time::sleep(link.latency).await;
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    );
                    if socket.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in body.chunks(CHUNK_BYTES) {
                        let done = {
                            let mut free = link_free.lock().await;
                            let start = (*free).max(tokio::time::Instant::now());
                            *free = start
                                + Duration::from_secs_f64(chunk.len() as f64 / link.bytes_per_sec);
                            *free
                        };
                        tokio::time::sleep_until(done).await;
                        if socket.write_all(chunk).await.is_err() {
                            return;
                        }
                    }
                    served.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
                }
            }
        });
    }
}

struct Report {
    name: &'static str,
    frames: usize,
    motion_ms_median: f64,
    motion_ms_p95: f64,
    motion_holes_pct: f64,
    complete_after_s: Option<f64>,
    sharp_after_s: Option<f64>,
    tiles: usize,
    megabytes: f64,
    wasted: usize,
}

/// The fill a pixel gets when no level of its imagery is loaded.
fn hole_rgba(cfg: &RenderCfg) -> [u8; 4] {
    let basis = Cam {
        lat: 0.0,
        lon: 0.0,
        heading: 0.0,
        alt: 1_000.0,
        tilt: 0.0,
    }
    .basis(1.0);
    let (pixels, _, _) =
        render_pixels_into(8, 8, 0..8, &basis, cfg, &|_| Lookup::Absent, Vec::new());
    let centre = (4 * 8 + 4) * RGBA_CHANNELS;
    pixels[centre..centre + 4].try_into().unwrap()
}

fn replay(
    name: &'static str,
    duration_s: f64,
    // A fly-to: its destination is known from the start and loaded ahead, as the app
    // does (`zoom_out::arrival_tiles`).
    flight: bool,
    path: impl Fn(f64) -> Cam,
    link: Link,
    body: &Arc<Vec<u8>>,
    runtime: &globecraft_data::runtime::ServiceRuntime,
) -> Report {
    let directory = std::env::temp_dir().join(format!(
        "globecraft-replay-{}-{}",
        std::process::id(),
        name.replace(' ', "-")
    ));
    let _ = std::fs::remove_dir_all(&directory);
    let served = Arc::new(Served::default());
    let listener = runtime
        .handle()
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = runtime.handle().spawn(serve(
        listener,
        body.clone(),
        Arc::new(fixture_dem_tile()),
        link,
        served.clone(),
    ));
    let cache = TileCache::with_fixture_source(|| {}, runtime.handle(), directory.clone(), &base);
    let viewport = emath::vec2(VIEWPORT_POINTS.0, VIEWPORT_POINTS.1);
    let cam_alt = std::cell::Cell::new(f64::MAX);
    let config = |lod_scale| RenderCfg {
        sky: None,
        provider: Provider::Satellite,
        // GLOBECRAFT_REPLAY_TERRAIN=1 adds relief, as the app draws it below 150 km.
        terrain: std::env::var("GLOBECRAFT_REPLAY_TERRAIN")
            .is_ok()
            .then(|| crate::TerrainCfg::new(1.0))
            .filter(|_| cam_alt.get() <= crate::TERRAIN_MAX_ALTITUDE_KM),
        rel: 0,
        grid: false,
        grid_step: 10.0,
        atmo: std::env::var("GLOBECRAFT_REPLAY_ATMOSPHERE").is_ok(),
        sun: None,
        lod_scale,
        level_blend: true,
        fixed_level: None,
        night_lights: false,
        stars: true,
    };
    let hole = hole_rgba(&config(1.0));
    let used = Mutex::new(HashSet::<TileKey>::new());
    let mut overview = globecraft_data::tiles::TileWarmup::startup_satellite();
    let mut zoom_out: Option<globecraft_data::tiles::TileWarmup> = None;
    let mut arrival =
        (flight && std::env::var("GLOBECRAFT_REPLAY_NO_ARRIVAL").is_err()).then(|| {
            let (w, h) = view::raster_size(viewport, 1.0, true);
            globecraft_data::tiles::TileWarmup::of_keys(crate::zoom_out::arrival_tiles(
                path(duration_s),
                h,
                w as f64 / h as f64,
                Provider::Satellite,
                &[0],
            ))
        });
    let (mut motion_ms, mut motion_holes) = (Vec::new(), Vec::new());
    let (mut complete_after_s, mut sharp_after_s) = (None, None);
    let mut buffer = Vec::new();
    let mut frames = 0;
    let started = Instant::now();
    loop {
        let t = started.elapsed().as_secs_f64();
        let moving = t < duration_s;
        let cam = path(t.min(duration_s));
        cam_alt.set(cam.alt);
        let (w, h) = view::raster_size(viewport, 1.0, !moving);
        let cfg = config(view::render_scale(viewport, 1.0, !moving));
        let basis = if std::env::var("GLOBECRAFT_REPLAY_WGS84").is_ok() {
            cam.basis_for_model(
                w as f64 / h as f64,
                globecraft_core::earth_model::EarthModel::Wgs84,
            )
        } else {
            cam.basis(w as f64 / h as f64)
        };
        let frame_started = Instant::now();
        let lookup = |key| {
            let found = cache.lookup(key);
            if matches!(found, Lookup::Ready(_)) {
                used.lock().unwrap().insert(key);
            }
            found
        };
        let (pixels, mut misses, waiting) = render_pixels_into(
            w,
            h,
            0..h,
            &basis,
            &cfg,
            &lookup,
            std::mem::take(&mut buffer),
        );
        misses.sort_by(|a, b| unkey(a.0).1.cmp(&unkey(b.0).1).then(a.1.total_cmp(&b.1)));
        let frame = RenderedView {
            pixels,
            waiting,
            requests: misses.into_iter().map(|(key, _)| key).collect(),
        };
        let required = frame.required_tiles();
        cache.prioritize_view(&required);
        let frame_ms = frame_started.elapsed().as_secs_f64() * 1000.0;
        frames += 1;
        let holes = frame
            .pixels
            .chunks_exact(RGBA_CHANNELS)
            .filter(|pixel| *pixel == hole)
            .count();
        let holes_pct = 100.0 * holes as f64 / (w * h) as f64;
        buffer = frame.pixels;
        if let Some(arrival) = &mut arrival {
            arrival.pump(&cache);
        }
        if moving {
            motion_ms.push(frame_ms);
            motion_holes.push(holes_pct);
        } else {
            let after = t - duration_s;
            if holes == 0 && complete_after_s.is_none() {
                complete_after_s = Some(after);
            }
            if required.is_empty() {
                sharp_after_s = Some(after);
                break;
            }
            if after > SETTLE_LIMIT.as_secs_f64() {
                break;
            }
            // The idle warmups of `globe.rs`.
            if cam.alt >= OVERVIEW_WARMUP_ALTITUDE_KM {
                overview.pump(&cache);
            }
            zoom_out
                .get_or_insert_with(|| {
                    globecraft_data::tiles::TileWarmup::of_keys(crate::zoom_out::zoom_out_tiles(
                        cam,
                        h,
                        w as f64 / h as f64,
                        Provider::Satellite,
                        &[0],
                    ))
                })
                .pump(&cache);
        }
        if let Some(rest) = FRAME_INTERVAL.checked_sub(frame_started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    server.abort();
    let served_keys = served.keys.lock().unwrap().clone();
    let used = used.into_inner().unwrap();
    let wasted = served_keys.iter().filter(|key| !used.contains(key)).count();
    let _ = std::fs::remove_dir_all(&directory);
    let quantile = |values: &mut Vec<f64>, q: f64| {
        values.sort_by(f64::total_cmp);
        values
            .get(((values.len() as f64 - 1.0) * q).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    Report {
        name,
        frames,
        motion_ms_median: quantile(&mut motion_ms, 0.5),
        motion_ms_p95: quantile(&mut motion_ms, 0.95),
        motion_holes_pct: motion_holes.iter().sum::<f64>() / motion_holes.len().max(1) as f64,
        complete_after_s,
        sharp_after_s,
        tiles: served_keys.len(),
        megabytes: served.bytes.load(Ordering::Relaxed) as f64 / 1e6,
        wasted,
    }
}

fn smoothstep(s: f64) -> f64 {
    let s = s.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

#[test]
#[ignore = "manual streaming replay benchmark; run alone in release with --ignored --nocapture --test-threads=1"]
fn streaming_replay() {
    let link = Link::from_env();
    let body = Arc::new(fixture_tile());
    let runtime = globecraft_data::runtime::ServiceRuntime::new().unwrap();
    let cam = |lat, lon, heading, alt| Cam {
        lat,
        lon,
        heading,
        alt,
        tilt: 0.0,
    };
    let reports = [
        // Zoom into Munich from a whole-globe view, exponentially like a scroll zoom.
        replay(
            "zoom into a city",
            8.0,
            false,
            |t| {
                let s = smoothstep(t / 8.0);
                cam(48.14, 11.58, 0.0, 12_000.0 * (3.0_f64 / 12_000.0).powf(s))
            },
            link,
            &body,
            &runtime,
        ),
        // Lisbon to Istanbul at street-to-region scale, rising mid-flight like a fly-to.
        replay(
            "fly across",
            8.0,
            true,
            |t| {
                let s = smoothstep(t / 8.0);
                cam(
                    38.72 + (41.01 - 38.72) * s,
                    -9.14 + (28.98 + 9.14) * s,
                    0.0,
                    50.0 + 2_500.0 * (PI * s).sin(),
                )
            },
            link,
            &body,
            &runtime,
        ),
        // Drag across Rio de Janeiro at 30 km, half a view per second.
        replay(
            "drag-pan",
            6.0,
            false,
            |t| cam(-22.9, -43.4 + 0.2 * t, 0.0, 30.0),
            link,
            &body,
            &runtime,
        ),
        // A full turn over Paris at 8 km.
        replay(
            "orbit",
            8.0,
            false,
            |t| cam(48.86, 2.35, 45.0 * t, 8.0),
            link,
            &body,
            &runtime,
        ),
        // Tilted toward the horizon, down to street level over Zurich.
        replay(
            "tilted descent",
            8.0,
            false,
            |t| {
                let s = smoothstep(t / 8.0);
                Cam {
                    tilt: 70.0,
                    ..cam(47.37, 8.54, 30.0, 60.0 * (1.0_f64 / 60.0).powf(s))
                }
            },
            link,
            &body,
            &runtime,
        ),
        // Turning while tilted close to the ground.
        replay(
            "tilted orbit close",
            6.0,
            false,
            |t| Cam {
                tilt: 70.0,
                ..cam(47.37, 8.54, 30.0 * t, 1.5)
            },
            link,
            &body,
            &runtime,
        ),
    ];
    println!(
        "Streaming replay: {:.0} x {:.0} pt view, latency {} ms, {:.0} KB/s shared, {} B tiles, {} Rayon threads",
        VIEWPORT_POINTS.0,
        VIEWPORT_POINTS.1,
        link.latency.as_millis(),
        link.bytes_per_sec / 1000.0,
        body.len(),
        rayon::current_num_threads()
    );
    println!(
        "| Path | Frames | Motion frame ms (median / p95) | Holes while moving | Complete after stop | Sharp after stop | Tiles | MB | Wasted |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    let seconds = |value: Option<f64>| {
        value.map_or_else(|| "not within 25 s".to_string(), |s| format!("{s:.2} s"))
    };
    for r in &reports {
        println!(
            "| {} | {} | {:.1} / {:.1} | {:.2} % | {} | {} | {} | {:.2} | {} ({:.0} %) |",
            r.name,
            r.frames,
            r.motion_ms_median,
            r.motion_ms_p95,
            r.motion_holes_pct,
            seconds(r.complete_after_s),
            seconds(r.sharp_after_s),
            r.tiles,
            r.megabytes,
            r.wasted,
            100.0 * r.wasted as f64 / r.tiles.max(1) as f64
        );
    }
}
