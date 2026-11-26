use super::*;
use globecraft_core::{camera::Cam, math::ll_to_v3};
use globecraft_data::tiles::{Lookup, Provider, make_key};

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored --nocapture"]
fn gpu_overview_matches_cpu() {
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
    // An aurora oval of 80 % from 60° to 75° of latitude, north and south.
    let rows: Vec<serde_json::Value> = (0..360)
        .flat_map(|lon| (-90..=90).map(move |lat: i32| (lon, lat)))
        .map(|(lon, lat)| {
            serde_json::json!([
                lon,
                lat,
                if (60..=75).contains(&lat.abs()) {
                    80
                } else {
                    0
                }
            ])
        })
        .collect();
    let aurora = crate::aurora::AuroraCfg {
        grid: Arc::new(
            globecraft_data::aurora::AuroraGrid::parse(&serde_json::json!({
                "Observation Time": "2026-11-04T04:00:00Z",
                "Forecast Time": "2026-11-04T04:00:00Z",
                "coordinates": rows,
            }))
            .unwrap(),
        ),
        opacity: 1.0,
    };
    // Water in stripes of 10° of longitude, shining more toward the east of each.
    let water = Arc::new(image::GrayImage::from_fn(360, 180, |x, _| {
        image::Luma([((x % 20) * 12) as u8])
    }));
    let sky_image = Arc::new(
        globecraft_data::sky::SkyImage::decode(include_bytes!(
            "../../../../assets/sky/milkyway-2020-4k-sdr.jpg"
        ))
        .unwrap(),
    );
    let image = image::RgbaImage::from_fn(256, 256, |x, y| {
        image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let tile = Arc::new(Tile::decode(bytes.get_ref()).unwrap());
    // The night lights' tile: another pattern, so a mixed-up lookup shows.
    let image = image::RgbaImage::from_fn(256, 256, |x, y| {
        image::Rgba([(x / 2 + y / 2) as u8, 255 - y as u8, 255 - x as u8, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let lights_tile = Arc::new(Tile::decode(bytes.get_ref()).unwrap());
    const LIGHTS_EDITION: u16 = 1;
    let lights_key = make_key(Provider::NightLights, 0, 0, 0, LIGHTS_EDITION);
    let (w, h) = (65, 49);
    // The GPU matches the CPU within one level per channel almost everywhere: its f32
    // trigonometry moves a few texel lookups on the test pattern's sharp edges. The
    // error statistics are reported and bounded (`PARITY_SHARE_OVER_ONE`). A software
    // rasterizer (accepted only with GLOBECRAFT_GPU_ALLOW_SOFTWARE, for CI without a
    // GPU) may hand a frame back to the CPU, as the app does; a GPU must not.
    let software = gpu.software;
    let (mut channels, mut over_one, mut total_error, mut fallbacks) =
        (0usize, 0usize, 0u64, 0usize);
    use globecraft_core::earth_model::EarthModel;
    for (provider, model) in
        Provider::basemaps().flat_map(|provider| EarthModel::ALL.map(|model| (provider, model)))
    {
        for (lat, lon, alt) in [
            (-15.0, -50.0, 14000.0),
            (89.9, 179.9, 1000.0),
            (-89.9, -179.9, 14000.0),
            (0.0, 180.0, 30000.0),
            // The polar caps' edge and a mid-latitude view, where the ellipsoid's
            // normal leans furthest from the sphere's.
            (78.0, 15.0, 5000.0),
            (45.0, 9.0, 2500.0),
            // Under the sun: its glint off the water.
            (20.0, 45.0, 8000.0),
        ] {
            for (enabled, lights) in [(false, false), (true, false), (false, true), (true, true)] {
                let cam = Cam {
                    lat,
                    lon,
                    alt,
                    heading: 25.0,
                    tilt: 0.0,
                    look_up: 0.0,
                };
                let request = RenderRequest {
                    width: w,
                    height: h,
                    cam,
                    basis: cam.basis_for_model(w as f64 / h as f64, model),
                    cfg: crate::RenderCfg {
                        sky: enabled.then(|| {
                            crate::celestial::SkyBackdrop::new(
                                sky_image.clone(),
                                1_760_000_000.0,
                                cam,
                                ll_to_v3(20.0, 45.0),
                                false,
                            )
                            .unwrap()
                        }),
                        provider,
                        terrain: None,
                        rel: 7,
                        grid: false,
                        grid_step: 10.0,
                        atmo: enabled,
                        sun: enabled.then(|| ll_to_v3(20.0, 45.0)),
                        lod_scale: 1.0,
                        level_blend: true,
                        fixed_level: None,
                        night_lights: lights.then_some(LIGHTS_EDITION),
                        stars: None,
                        sky_behind: false,
                        clouds: None,
                        glint: enabled.then(|| water.clone()),
                        aurora: lights.then(|| aurora.clone()),
                    },
                    overlays: None,
                    aircraft: None,
                    live_layers: None,
                    compare: None,
                    presentation: 0,
                };
                for (loaded, pending) in [(false, false), (false, true), (true, false)] {
                    let key = make_key(provider, 0, 0, 0, 7);
                    let slots = gpu
                        .upload(&if loaded {
                            vec![(key, tile.clone()), (lights_key, lights_tile.clone())]
                        } else {
                            Vec::new()
                        })
                        .unwrap();
                    let lookup = |k| {
                        if loaded && k == key {
                            Lookup::Ready(tile.clone())
                        } else if loaded && k == lights_key {
                            Lookup::Ready(lights_tile.clone())
                        } else if pending && (k == key || k == lights_key) {
                            Lookup::Pending
                        } else {
                            Lookup::Absent
                        }
                    };
                    let (cpu, cpu_misses, cpu_waiting) =
                        crate::render_pixels_tracked(w, h, &request.basis, &request.cfg, &lookup);
                    let rendered = gpu.overview.render(
                        overview::Context {
                            device: &gpu.device,
                            queue: &gpu.queue,
                            atlas: &gpu.view,
                            means: &gpu.means,
                            walk: None,
                        },
                        &request,
                        &slots,
                        &lookup,
                        Vec::new(),
                    );
                    let frame = match rendered {
                        Ok(frame) => frame.view,
                        Err((error, _, _)) if software => {
                            println!(
                                "FALLBACK {provider:?} {model:?} ({lat},{lon}) alt={alt}: {error}"
                            );
                            fallbacks += 1;
                            continue;
                        }
                        Err((error, _, _)) => panic!("{error}"),
                    };
                    channels += cpu.len();
                    total_error += frame
                        .pixels
                        .iter()
                        .zip(&cpu)
                        .map(|(a, e)| u64::from(a.abs_diff(*e)))
                        .sum::<u64>();
                    let diffs: Vec<(usize, u8)> = frame
                        .pixels
                        .iter()
                        .zip(&cpu)
                        .enumerate()
                        .map(|(i, (a, e))| (i, a.abs_diff(*e)))
                        .filter(|(_, d)| *d > 1)
                        .collect();
                    over_one += diffs.len();
                    if !diffs.is_empty() {
                        let max = diffs.iter().map(|d| d.1).max().unwrap();
                        let pixels: Vec<(usize, usize)> = diffs
                            .iter()
                            .take(12)
                            .map(|(i, _)| ((i / 4) % w, (i / 4) / w))
                            .collect();
                        println!(
                            "DIFF {provider:?} {model:?} ({lat},{lon}) alt={alt} enabled={enabled} lights={lights} loaded={loaded} pending={pending}: {} channels >1, max {max}, at {pixels:?}",
                            diffs.len()
                        );
                    }
                    assert_eq!(
                        frame
                            .waiting
                            .iter()
                            .copied()
                            .collect::<std::collections::HashSet<_>>(),
                        cpu_waiting.into_iter().collect(),
                        "pending and missing dependencies must match CPU"
                    );
                    let actual: std::collections::HashSet<_> = frame.requests.into_iter().collect();
                    let expected = cpu_misses.into_iter().map(|entry| entry.0).collect();
                    assert_eq!(actual, expected, "requested tile coverage must match CPU");
                }
            }
        }
    }
    let mean = total_error as f64 / channels.max(1) as f64;
    let share = over_one as f64 / channels.max(1) as f64;
    println!(
        "PARITY {}: {channels} channels, mean error {mean:.4}, {:.3}% over 1, {fallbacks} frames handed to the CPU",
        if software { "software adapter" } else { "GPU" },
        share * 100.0
    );
    assert!(
        mean < 0.01 && share < PARITY_SHARE_OVER_ONE,
        "parity too loose: mean error {mean}, {share} of channels over one level"
    );
    assert!(
        software || fallbacks == 0,
        "a GPU must render every overview frame"
    );
    assert!(
        gpu.upload(&[(
            0,
            Arc::new(Tile::decode(include_bytes!("../../tests/fixtures/tile.png")).unwrap())
        )])
        .is_err()
    );
    assert!(
        gpu.upload(
            &(0..MAX_ATLAS_LAYERS as u64 + 1)
                .map(|key| (key, tile.clone()))
                .collect::<Vec<_>>()
        )
        .is_err()
    );
    gpu.device.destroy();
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(
        gpu.failure.lock().unwrap().is_some(),
        "device loss must be reported for CPU fallback"
    );
}

#[test]
fn unsupported_views_choose_precision_instead_of_truncating_gpu_work() {
    let cam = Cam {
        lat: -15.0,
        lon: -50.0,
        heading: 25.0,
        alt: 14000.0,
        tilt: 0.0,
        look_up: 0.0,
    };
    let mut request = RenderRequest {
        width: 2000,
        height: 1488,
        cam,
        basis: cam.basis(2000.0 / 1488.0),
        cfg: crate::RenderCfg {
            sky: None,
            provider: Provider::Satellite,
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
    assert!(GpuRenderer::supports(&request));
    // The default Earth model and a view of a pole are the GPU's too.
    request.cam.lat = 80.0;
    request.basis = request.cam.basis_for_model(
        request.width as f64 / request.height as f64,
        globecraft_core::earth_model::EarthModel::Wgs84,
    );
    assert!(GpuRenderer::supports(&request));
    let at = |request: &mut RenderRequest, alt: f64, tilt: f64| {
        request.cam.alt = alt;
        request.cam.tilt = tilt;
        request.basis = request.cam.basis_for_model(
            request.width as f64 / request.height as f64,
            globecraft_core::earth_model::EarthModel::Wgs84,
        );
    };
    // Close views are the GPU's too (roadmap D8): from inside the air (G2), with
    // levels to 20 from the map near the camera (G3).
    for (alt, tilt) in [(0.15, 0.0), (0.15, 80.0), (130.0, 60.0), (260.0, 60.0)] {
        at(&mut request, alt, tilt);
        assert!(GpuRenderer::supports(&request), "{alt} km at {tilt}°");
    }
    at(&mut request, 14000.0, 0.0);
    request.cfg.grid = true;
    assert!(!GpuRenderer::supports(&request));
    request.cfg.grid = false;
    request.width = usize::MAX;
    assert!(!GpuRenderer::supports(&request));
    request.width = 0;
    assert!(!GpuRenderer::supports(&request));
}

#[test]
fn motion_and_idle_frames_choose_the_same_level() {
    let cam = Cam {
        lat: 40.0,
        lon: 71.0,
        heading: 0.0,
        alt: 2000.0,
        tilt: 0.0,
        look_up: 0.0,
    };
    let request = |width: usize, height: usize, lod_scale: f32| RenderRequest {
        width,
        height,
        cam,
        basis: cam.basis(width as f64 / height as f64),
        cfg: crate::RenderCfg {
            sky: None,
            provider: Provider::Satellite,
            terrain: None,
            rel: 0,
            grid: false,
            grid_step: 10.0,
            atmo: true,
            sun: None,
            lod_scale,
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
    // The motion raster is 0.4 of the idle one; both pick the idle raster's level.
    let idle = super::overview::supported_zoom(&request(2000, 1250, 1.0));
    let motion = super::overview::supported_zoom(&request(800, 500, 0.4));
    assert!(idle.is_some());
    assert_eq!(idle, motion);
}

#[test]
fn the_self_check_accepts_rounding_and_refuses_wrong_pictures() {
    let cpu = vec![100u8; 4000];
    let mut gpu = cpu.clone();
    gpu[..2000].fill(101);
    assert_eq!(parity(&gpu, &cpu), Ok(()), "one level is rounding");
    // A few channels two levels off are imagery contours; many are a wrong picture.
    gpu[..39].fill(102);
    assert_eq!(parity(&gpu, &cpu), Ok(()));
    gpu[..40].fill(102);
    assert_eq!(parity(&gpu, &cpu), Err((40, 2)));
    let mut gpu = cpu.clone();
    gpu[7] = 100 + IMAGERY_MAX_DIFFERENCE + 1;
    assert_eq!(parity(&gpu, &cpu), Err((1, IMAGERY_MAX_DIFFERENCE + 1)));
    assert!(parity(&cpu[..8], &cpu).is_err(), "a short picture");
}

/// The relief march (`relief`) on synthetic Himalayan relief (`terrain_views`' views):
/// with its starts the CPU draws the same picture, and marches fewer steps.
#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored --nocapture"]
fn gpu_relief_march_keeps_the_cpu_picture() {
    relief_variant();
    use std::sync::Mutex;
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
    let tiles: Mutex<HashMap<TileKey, Arc<Tile>>> = Mutex::new(HashMap::new());
    let lookup = |key: TileKey| {
        let (provider, z, x, y, _) = globecraft_data::tiles::unkey(key);
        if provider == Provider::Satellite && z > 14 {
            return Lookup::Unavailable;
        }
        let mut tiles = tiles.lock().unwrap();
        Lookup::Ready(Arc::clone(tiles.entry(key).or_insert_with(|| {
            if provider == Provider::TerrainDem {
                crate::tests::probe_dem_tile(z, x, y)
            } else {
                crate::tests::probe_image_tile(z, x, y)
            }
        })))
    };
    // `GLOBECRAFT_RELIEF_PROBE_SIZE=1920x1200` times a full window on a real GPU.
    let (width, height) = std::env::var("GLOBECRAFT_RELIEF_PROBE_SIZE")
        .ok()
        .and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        // Not whole workgroups (8 pixels) across or down: the edges' partial ones count.
        .unwrap_or((324, 203));
    let mut failures = Vec::new();
    // `GLOBECRAFT_RELIEF_PROBE_VIEWS=tilt-16km,top-16km` runs only those.
    let only = std::env::var("GLOBECRAFT_RELIEF_PROBE_VIEWS").ok();
    for (name, alt, tilt, heading) in [
        ("top-16km", 16.0, 0.0, 0.0),
        ("tilt-16km", 16.0, 70.0, 0.0),
        ("tilt-52km", 52.0, 75.0, 0.0),
        ("tilt-6km", 6.0, 72.0, 30.0),
        ("tilt85-3km", 3.0, 85.0, 30.0),
    ] {
        if only
            .as_ref()
            .is_some_and(|only| !only.split(',').any(|view| view == name))
        {
            continue;
        }
        let cam = Cam {
            lat: 30.3,
            lon: 79.8,
            heading,
            alt,
            tilt,
            look_up: 0.0,
        };
        let mut cfg = crate::tests::batch_reference_config(Provider::Satellite);
        cfg.grid = false;
        cfg.terrain = Some(crate::TerrainCfg::new(1.0, 1.0));
        // A low sun in the west: long shadows.
        cfg.sun = Some(globecraft_core::math::ll_to_v3(15.0, -10.0));
        let request = RenderRequest {
            width,
            height,
            cam,
            basis: cam.basis_for_model(
                width as f64 / height as f64,
                globecraft_core::earth_model::EarthModel::Wgs84,
            ),
            cfg,
            overlays: None,
            aircraft: None,
            live_layers: None,
            presentation: 0,
            compare: None,
        };
        // Counted on one thread.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let render = |starts: Option<&[f32]>| {
            pool.install(|| {
                crate::terrain::COUNTS.with(|c| c.set((0, 0)));
                let frame = crate::render_pixels_with(
                    width,
                    height,
                    0..height,
                    &request.basis,
                    &request.cfg,
                    &lookup,
                    Vec::new(),
                    starts,
                );
                (frame, crate::terrain::COUNTS.with(|c| c.get().0))
            })
        };
        let (frame, cpu_steps) = render(None);
        let (expected, held) = (frame.pixels, frame.relief_tiles);
        let march = gpu
            .relief
            .get_or_insert_with(|| Some(relief::ReliefMarch::new(&gpu.device, 128)))
            .as_mut()
            .unwrap();
        // Frames one after another, as the backend draws them: each walk holds the
        // tiles the CPU's last frame held and the ones the last walk read (up to 64
        // more uploaded each frame). From the second frame on, every frame takes as
        // many of the GPU's grounds (they did not come and go, 2026-11-23; with six
        // uploads a frame a camera low among the relief waited 7 frames).
        let count = width * height;
        let taken = |pixels: &[f32]| {
            pixels[3 * count..4 * count]
                .iter()
                .filter(|&&t| t > 0.5)
                .count()
        };
        let mut tiles = held;
        let mut taken_by_frame = Vec::new();
        let mut uploads_by_frame = Vec::new();
        let mut starts = Vec::new();
        for _ in 0..16 {
            let grown = relief::grown_tiles(&tiles, lookup, std::time::Instant::now());
            let walked = march
                .starts(&gpu.device, &gpu.queue, &request, &grown)
                .expect("relief march");
            taken_by_frame.push(taken(&walked.pixels));
            uploads_by_frame.push(walked.uploads);
            let frame = render(Some(&walked.pixels)).0;
            tiles = relief::next_tiles(frame.relief_tiles, &walked.read);
            starts = walked.pixels;
        }
        let most = *taken_by_frame.iter().max().unwrap();
        println!(
            "{name}: grounds taken frame by frame {taken_by_frame:?}, tiles uploaded {uploads_by_frame:?}"
        );
        assert!(
            taken_by_frame[1..]
                .iter()
                .all(|&taken| taken * 10 >= most * 9),
            "{name}: {taken_by_frame:?}"
        );
        let walked = std::time::Instant::now();
        let grown = relief::grown_tiles(&tiles, lookup, std::time::Instant::now());
        let timed = march
            .starts(&gpu.device, &gpu.queue, &request, &grown)
            .expect("relief march");
        let (uploads, took) = (timed.uploads, timed.took);
        let walk_time = walked.elapsed();
        println!(
            "{name}: the GPU's walk {walk_time:?} ({uploads} tiles uploaded first): on the device {:?} (the cones, the view, the sun's quarter, its other rows, its odd columns), waited {:?}, copied {:?}",
            took.0, took.1, took.2
        );
        if !relief::helps(&starts) {
            println!("{name}: the walk gave up on most rays; the CPU's cones start them");
            continue;
        }
        // The same walk without the sun's share: what the walk toward the sun costs.
        let sunless = RenderRequest {
            cfg: crate::RenderCfg {
                sun: None,
                ..request.cfg.clone()
            },
            basis: cam.basis_for_model(
                width as f64 / height as f64,
                globecraft_core::earth_model::EarthModel::Wgs84,
            ),
            overlays: None,
            aircraft: None,
            live_layers: None,
            compare: None,
            ..request
        };
        let walked = std::time::Instant::now();
        let without_sun = march
            .starts(&gpu.device, &gpu.queue, &sunless, &grown)
            .expect("relief march")
            .pixels;
        let sunless_time = walked.elapsed();
        let (frame, gpu_steps) = render(Some(&starts));
        let pixels = frame.pixels;
        let result = parity(&pixels, &expected);
        // The march started from the camera: how much the start alone moves the
        // picture (the CPU's linear steps between exact measurements).
        let mut zeros = vec![0.0; width * height];
        zeros.resize(4 * width * height, -1.0);
        let from_camera = render(Some(&zeros)).0.pixels;
        let hits = crate::relief_hits(
            width,
            height,
            &request.basis,
            &request.cfg,
            &lookup,
            crate::HitMarch::Frame,
        );
        // The GPU's grounds are measured exactly: so is the march they are checked against.
        let exact_hits = crate::relief_hits(
            width,
            height,
            &request.basis,
            &request.cfg,
            &lookup,
            crate::HitMarch::Exact,
        );
        let (mut past, mut share, mut counted, mut most) = (0, 0.0, 0, 0.0f64);
        let known = starts[2 * count..3 * count]
            .iter()
            .filter(|&&seen| seen >= 0.0)
            .count();
        let met = starts[count..2 * count]
            .iter()
            .filter(|&&hit| hit >= 0.0)
            .count();
        let trusted = starts[3 * count..4 * count]
            .iter()
            .filter(|&&trusted| trusted > 0.5)
            .count();
        println!(
            "{name}: the GPU met the ground for {met} pixels ({trusted} taken as they are), knew the sun's share for {known}"
        );
        // The sun's share where both met the ground at the same place.
        let (mut compared, mut off, mut total) = (0, 0, 0.0);
        for (i, &(hit, seen)) in hits.iter().enumerate() {
            let (gpu_hit, gpu_seen) = (f64::from(starts[count + i]), starts[2 * count + i]);
            if hit.is_finite() && gpu_seen >= 0.0 && (gpu_hit - hit).abs() <= hit * 1e-3 {
                compared += 1;
                total += f64::from((gpu_seen - seen).abs());
                off += usize::from((gpu_seen - seen).abs() > 0.25);
            }
        }
        println!(
            "{name}: sun's share compared at {compared} pixels: mean difference {:.3}, {off} off by more than a quarter",
            total / compared.max(1) as f64
        );
        for (&start, &(hit, _)) in starts[..count].iter().zip(&hits) {
            if hit.is_finite() {
                past += usize::from(f64::from(start) > hit);
                most = most.max(f64::from(start) / hit - 1.0);
                share += f64::from(start) / hit;
                counted += 1;
            }
        }
        println!(
            "{name}: {past} starts past the ground (at most {:.4} % of the way); on average {:.0} % of the way",
            most * 100.0,
            share / counted.max(1) as f64 * 100.0
        );
        let differ = |a: &[u8], b: &[u8]| {
            a.iter()
                .zip(b)
                .filter(|(a, b)| a.abs_diff(**b) > 16)
                .count()
        };
        println!(
            "{name}: channels off by more than 16: GPU's walk {}, cones {} (against the march from the camera)",
            differ(&pixels, &from_camera),
            differ(&expected, &from_camera)
        );
        if let Ok(dir) = std::env::var("GLOBECRAFT_PROBE_DIR") {
            let save = |pixels: &[u8], what: &str| {
                image::RgbaImage::from_raw(width as u32, height as u32, pixels.to_vec())
                    .unwrap()
                    .save(format!("{dir}/{name}-{what}.png"))
                    .unwrap();
            };
            save(&expected, "cpu");
            save(&pixels, "gpu");
            let shade: Vec<u8> = starts[2 * width * height..]
                .iter()
                .flat_map(|&t| {
                    let level = (t * 255.0).clamp(0.0, 255.0) as u8;
                    [level, level, level, 255]
                })
                .collect();
            save(&shade, "starts");
        }
        println!(
            "{name}: {:.1} CPU march steps per pixel after the GPU's walk ({:.1} after the cones), {result:?}",
            gpu_steps as f64 / (width * height) as f64,
            cpu_steps as f64 / (width * height) as f64,
        );
        let _ = result;
        let timed = |starts: Option<&[f32]>| {
            (0..3)
                .map(|_| {
                    let started = std::time::Instant::now();
                    crate::render_pixels_with(
                        width,
                        height,
                        0..height,
                        &request.basis,
                        &request.cfg,
                        &lookup,
                        Vec::new(),
                        starts,
                    );
                    started.elapsed()
                })
                .min()
                .unwrap()
        };
        let mut flat = request.cfg.clone();
        flat.terrain = None;
        let flat_time = (0..3)
            .map(|_| {
                let started = std::time::Instant::now();
                crate::render_pixels_with(
                    width,
                    height,
                    0..height,
                    &request.basis,
                    &flat,
                    &lookup,
                    Vec::new(),
                    None,
                );
                started.elapsed()
            })
            .min()
            .unwrap();
        println!(
            "{name}: the CPU's part {:?} after the GPU's walk ({walk_time:?}), {:?} after the walk without the sun ({sunless_time:?}), {:?} with the cones, {flat_time:?} without relief",
            timed(Some(&starts)),
            timed(Some(&without_sun)),
            timed(None)
        );
        // Every view runs before the checks fail, so one run on a real GPU tells all.
        let past = relief::past_the_ground(&starts[..count], &hits);
        for (i, (&start, &(hit, _))) in starts[..count].iter().zip(&hits).enumerate() {
            if f64::from(start) > hit * (1.0 + relief::PAST_THE_GROUND_SHARE) {
                println!(
                    "{name}: past the ground at ({}, {}): start {start:e}, the CPU's ground {hit:e}, the GPU's {:e} (trusted {})",
                    i % width,
                    i / width,
                    starts[count + i],
                    starts[3 * count + i]
                );
            }
        }
        let wrong = relief::trusted_past_the_ground(&starts, &exact_hits);
        println!("{name}: {wrong} grounds taken from the GPU past the CPU's");
        for (i, &(hit, _)) in exact_hits.iter().enumerate() {
            let gpu = f64::from(starts[count + i]);
            if starts[3 * count + i] > 0.5 && gpu > hit * (1.0 + relief::PAST_THE_GROUND_SHARE) {
                println!(
                    "{name}: taken past the ground at ({}, {}): the GPU's {gpu:e}, the CPU's {hit:e}",
                    i % width,
                    i / width
                );
            }
        }
        if past > 0 || gpu_steps > cpu_steps || wrong * relief::TRUSTED_PAST_PER > count {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
    let march = gpu.relief.take().flatten().unwrap();
    drop(march);
    let mut checked = relief::ReliefMarch::new(&gpu.device, 128);
    checked
        .self_check(&gpu.device, &gpu.queue)
        .expect("the self-check passes");
}

/// Close cameras (roadmap D8): from 1,000 km down into the air (G1b, G2), the GPU's
/// picture matches the CPU's within the bounds real imagery is held to: levels to 13,
/// Esri's levels 11 and 12 matched in colour, the air seen from above it and from
/// inside it (its tables narrowed to the camera, the integrated table low down) with
/// the sun high, low and set, with and without the night lights. Views whose eye is low take a coarser level of detail,
/// so they stay within level 13 until the levels past it are the GPU's (G3).
#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored --nocapture"]
fn gpu_close_overview_matches_cpu() {
    overview_variant();
    use std::sync::Mutex;
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
    let tiles: Mutex<HashMap<TileKey, Arc<Tile>>> = Mutex::new(HashMap::new());
    let asked: Mutex<std::collections::HashSet<TileKey>> = Mutex::new(Default::default());
    let lookup = |key: TileKey| {
        asked.lock().unwrap().insert(key);
        let (provider, z, x, y, _) = globecraft_data::tiles::unkey(key);
        let mut tiles = tiles.lock().unwrap();
        Lookup::Ready(Arc::clone(
            tiles
                .entry(key)
                .or_insert_with(|| smooth_tile(provider, z, x, y)),
        ))
    };
    let (w, h) = (160usize, 100usize);
    let mut failures = Vec::new();
    for provider in [Provider::Satellite, Provider::Streets] {
        for (lat, lon, alt, tilt, heading, sun_east, fine) in [
            (45.0, 9.0, 900.0, 0.0, 0.0, 30.0, false),
            (45.0, 9.0, 500.0, 30.0, 40.0, 30.0, false),
            (30.3, 79.8, 200.0, 0.0, 0.0, 30.0, false),
            (30.3, 79.8, 150.0, 45.0, 10.0, 30.0, false),
            (-33.9, 151.2, 125.0, 20.0, 200.0, 30.0, false),
            (64.0, -150.0, 260.0, 60.0, 90.0, 30.0, false),
            (20.0, -150.0, 2000.0, 80.0, 90.0, 30.0, false),
            // Inside the air: from its top down to the integrated table's heights,
            // the sun high, low over the horizon and just set.
            (45.0, 9.0, 90.0, 0.0, 0.0, 30.0, false),
            (64.0, -150.0, 130.0, 60.0, 90.0, 30.0, false),
            (30.3, 79.8, 40.0, 30.0, 120.0, 80.0, false),
            (-33.9, 151.2, 25.0, 50.0, 300.0, 86.0, false),
            (45.0, 9.0, 12.0, 70.0, 90.0, 30.0, false),
            (45.0, 9.0, 8.0, 0.0, 0.0, 95.0, false),
            (10.0, -60.0, 3.0, 75.0, 270.0, 88.0, false),
            // Close (G3): the levels of a 1,200-pixel-high view, to 20 near the
            // camera, from the map near it; across the antimeridian and far north.
            (45.0, 9.0, 8.0, 0.0, 0.0, 30.0, true),
            (30.3, 79.8, 3.0, 45.0, 10.0, 30.0, true),
            (64.0, -150.0, 1.0, 70.0, 90.0, 80.0, true),
            (-33.9, 151.2, 0.3, 0.0, 0.0, 30.0, true),
            (10.0, -60.0, 0.15, 80.0, 270.0, 30.0, true),
            (46.5, 179.99, 2.0, 30.0, 90.0, 30.0, true),
            (78.0, 15.0, 5.0, 20.0, 180.0, 30.0, true),
        ] {
            for (lit, lights) in [(false, false), (true, false), (false, true), (true, true)] {
                let cam = Cam {
                    lat,
                    lon,
                    alt,
                    heading,
                    tilt,
                    look_up: 0.0,
                };
                let request = RenderRequest {
                    width: w,
                    height: h,
                    cam,
                    basis: cam.basis_for_model(
                        w as f64 / h as f64,
                        globecraft_core::earth_model::EarthModel::Wgs84,
                    ),
                    cfg: crate::RenderCfg {
                        provider,
                        rel: 7,
                        atmo: lit,
                        sun: lit.then(|| {
                            ll_to_v3(lat - 20.0 * (sun_east / 30.0_f64).min(1.0), lon + sun_east)
                        }),
                        grid: false,
                        lod_scale: 1.0,
                        level_blend: true,
                        night_lights: lights.then_some(1),
                        // The sky behind the frame, as the app draws it (the GPU's,
                        // G5), but for one case: the sky drawn into the frame (the
                        // CPU's).
                        sky_behind: lit || lights,
                        ..crate::tests::batch_reference_config(provider)
                    },
                    overlays: None,
                    aircraft: None,
                    live_layers: None,
                    compare: None,
                    presentation: 0,
                };
                // The levels of a view up to 2,000 pixels high, no finer than 13, or
                // those of a 1,200-pixel view.
                let mut request = request;
                let c = request.basis.c;
                let eye_km = (c
                    .dot(c * request.basis.model.geometry().inverse_axes_squared())
                    .sqrt()
                    - 1.0)
                    * globecraft_core::EARTH_RADIUS_KM;
                let lod_height = if fine {
                    1200.0
                } else {
                    (eye_km * 21.0).clamp(16.0, 2000.0)
                };
                request.cfg.lod_scale = (h as f64 / lod_height) as f32;
                let name = format!(
                    "{provider:?} ({lat},{lon}) {alt} km tilt {tilt} (eye {eye_km:.0} km) sun east {sun_east} lit {lit} lights {lights}"
                );
                assert!(GpuRenderer::supports(&request), "{name}");
                asked.lock().unwrap().clear();
                let (cpu, _, _) =
                    crate::render_pixels_tracked(w, h, &request.basis, &request.cfg, &lookup);
                let used: Vec<_> = {
                    let asked = asked.lock().unwrap();
                    let tiles = tiles.lock().unwrap();
                    asked.iter().map(|key| (*key, tiles[key].clone())).collect()
                };
                let deepest = used
                    .iter()
                    .map(|(key, _)| globecraft_data::tiles::unkey(*key).1)
                    .max()
                    .unwrap();
                let slots = gpu.upload(&used).expect("the view's tiles fit the atlas");
                let frame = gpu
                    .overview
                    .render(
                        overview::Context {
                            device: &gpu.device,
                            queue: &gpu.queue,
                            atlas: &gpu.view,
                            means: &gpu.means,
                            walk: None,
                        },
                        &request,
                        &slots,
                        &lookup,
                        Vec::new(),
                    )
                    .unwrap_or_else(|(error, _, _)| panic!("{name}: {error}"));
                let result = parity(&frame.view.pixels, &cpu);
                println!(
                    "{name}: {} tiles to level {deepest}, parity {result:?}, waiting {}, CPU pixels {}",
                    used.len(),
                    frame.view.waiting.len(),
                    frame.repaired
                );
                if result.is_err() {
                    failures.push(name);
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// Relief views drawn by the GPU from its walk (roadmap D8, G4): the ground the walk
/// met and the CPU would take as its own, its slope, the sun's share there and the
/// sun rising earlier over high ground, as the CPU draws the same frame from the same
/// walk; the CPU draws the rest (its part printed). With the sun low and high, without
/// it (hill shading), with more exaggeration and the relief half faded in.
#[test]
#[ignore = "needs a native GPU adapter; run explicitly to verify relief parity"]
fn gpu_relief_view_matches_cpu() {
    overview_variant();
    relief_variant();
    use std::sync::Mutex;
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
    let tiles: Mutex<HashMap<TileKey, Arc<Tile>>> = Mutex::new(HashMap::new());
    let asked: Mutex<std::collections::HashSet<TileKey>> = Mutex::new(Default::default());
    let lookup = |key: TileKey| {
        let (provider, z, x, y, _) = globecraft_data::tiles::unkey(key);
        if provider == Provider::Satellite && z > 14 {
            return Lookup::Unavailable;
        }
        if provider != Provider::TerrainDem {
            asked.lock().unwrap().insert(key);
        }
        let mut tiles = tiles.lock().unwrap();
        Lookup::Ready(Arc::clone(tiles.entry(key).or_insert_with(|| {
            if provider == Provider::TerrainDem {
                crate::tests::probe_dem_tile(z, x, y)
            } else {
                smooth_tile(provider, z, x, y)
            }
        })))
    };
    let (width, height) = std::env::var("GLOBECRAFT_RELIEF_PROBE_SIZE")
        .ok()
        .and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((320, 200));
    let mut failures = Vec::new();
    for (name, alt, tilt, heading) in [
        ("top-16km", 16.0, 0.0, 0.0),
        ("tilt-16km", 16.0, 70.0, 0.0),
        ("tilt-52km", 52.0, 75.0, 0.0),
        ("tilt-6km", 6.0, 72.0, 30.0),
    ] {
        for (light, sun, exaggeration, fade) in [
            (
                "low sun",
                Some(globecraft_core::math::ll_to_v3(15.0, -10.0)),
                1.0,
                1.0,
            ),
            (
                "high sun",
                Some(globecraft_core::math::ll_to_v3(25.0, 70.0)),
                1.0,
                1.0,
            ),
            ("no sun", None, 1.0, 1.0),
            (
                "low sun, 2× and half faded",
                Some(globecraft_core::math::ll_to_v3(15.0, -10.0)),
                2.0,
                0.5,
            ),
        ] {
            let cam = Cam {
                lat: 30.3,
                lon: 79.8,
                heading,
                alt,
                tilt,
                look_up: 0.0,
            };
            let mut cfg = crate::tests::batch_reference_config(Provider::Satellite);
            cfg.grid = false;
            cfg.terrain = Some(crate::TerrainCfg::new(exaggeration, fade));
            cfg.sun = sun;
            // The sky behind the frame, as the app draws it (the GPU's, G5).
            cfg.sky_behind = true;
            let request = RenderRequest {
                width,
                height,
                cam,
                basis: cam.basis_for_model(
                    width as f64 / height as f64,
                    globecraft_core::earth_model::EarthModel::Wgs84,
                ),
                cfg,
                overlays: None,
                aircraft: None,
                live_layers: None,
                presentation: 0,
                compare: None,
            };
            let name = format!("{name}, {light}");
            assert!(GpuRenderer::supports(&request), "{name}");
            let render = |starts: Option<&[f32]>| {
                crate::render_pixels_with(
                    width,
                    height,
                    0..height,
                    &request.basis,
                    &request.cfg,
                    &lookup,
                    Vec::new(),
                    starts,
                )
            };
            // Frames one after another, as the backend draws them, until the walk
            // holds what the CPU's frames held.
            let mut held = render(None).relief_tiles;
            let march = gpu
                .relief
                .get_or_insert_with(|| Some(relief::ReliefMarch::new(&gpu.device, 128)))
                .as_mut()
                .unwrap();
            let mut planes = Vec::new();
            for _ in 0..4 {
                let grown = relief::grown_tiles(&held, lookup, std::time::Instant::now());
                let walked = march
                    .starts(&gpu.device, &gpu.queue, &request, &grown)
                    .expect("relief march");
                held = relief::next_tiles(render(Some(&walked.pixels)).relief_tiles, &walked.read);
                planes = walked.pixels;
            }
            // The planes left on the device read back row by row as they were whole;
            // rows not asked for stay zero.
            let rows: Vec<bool> = (0..height).map(|y| y % 3 == 0).collect();
            let some = march
                .read_rows(&gpu.device, &gpu.queue, &rows)
                .expect("rows read back");
            for (i, (&read, &whole)) in some.iter().zip(&planes).enumerate() {
                let y = i % (width * height) / width;
                assert_eq!(
                    read.to_bits(),
                    if rows[y] { whole } else { 0.0 }.to_bits(),
                    "{name}"
                );
            }
            asked.lock().unwrap().clear();
            let cpu = render(Some(&planes)).pixels;
            let used: Vec<_> = {
                let asked = asked.lock().unwrap();
                let tiles = tiles.lock().unwrap();
                asked.iter().map(|key| (*key, tiles[key].clone())).collect()
            };
            let slots = gpu.upload(&used).expect("the view's tiles fit the atlas");
            let march = gpu.relief.as_ref().unwrap().as_ref().unwrap();
            let drawn = std::time::Instant::now();
            let frame = gpu
                .overview
                .render(
                    overview::Context {
                        device: &gpu.device,
                        queue: &gpu.queue,
                        atlas: &gpu.view,
                        means: &gpu.means,
                        // As the app's frames: the CPU reads the rows it draws.
                        walk: Some((march.output(), overview::Planes::OnDevice(march))),
                    },
                    &request,
                    &slots,
                    &lookup,
                    Vec::new(),
                )
                .unwrap_or_else(|(error, _, _)| panic!("{name}: {error}"));
            let drawn = drawn.elapsed();
            let result = parity(&frame.view.pixels, &cpu);
            println!(
                "{name}: parity {result:?}, drawn in {drawn:?} after the walk ({:?} on the device with the readback, {:?} the CPU's part), the CPU's part {:.1} % ({} pixels), waiting {}",
                frame.took.0,
                frame.took.1,
                100.0 * frame.repaired as f64 / (width * height) as f64,
                frame.repaired,
                frame.view.waiting.len()
            );
            if result.is_err() {
                failures.push(name);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// Smooth colour over the globe at every level with a faint pattern in each tile, a
/// sharp texel pattern past level 13 (so a place a fraction of a texel off shows);
/// Esri's imagery a third darker to level 11 (its mosaics) than from 12
/// (photographs), which colour matching evens out.
fn smooth_tile(provider: Provider, z: u8, x: u32, y: u32) -> Arc<Tile> {
    // Smooth colour over the globe at every level with a faint pattern in each
    // tile; Esri's imagery a third darker to level 11 (its mosaics) than from
    // 12 (photographs), which colour matching evens out.
    let gain = if provider == Provider::Satellite && z <= 11 {
        0.7
    } else {
        1.0
    };
    let image = image::RgbaImage::from_fn(256, 256, |px, py| {
        let u = (f64::from(x) + f64::from(px) / 256.0) / f64::from(1u32 << z);
        let v = (f64::from(y) + f64::from(py) / 256.0) / f64::from(1u32 << z);
        let wave = |f: f64| (0.5 + 0.5 * (f * std::f64::consts::TAU).sin()) as f32;
        // Past level 13 a sharp texel pattern, so a place a fraction of a
        // texel off shows.
        let detail = if z > 13 {
            if (px / 2 + py / 2) % 2 == 0 { 0.9 } else { 1.0 }
        } else if (px / 16 + py / 16) % 2 == 0 {
            0.98
        } else {
            1.0
        };
        let channel = |value: f32| (value * gain * detail * 255.0) as u8;
        image::Rgba([
            channel(wave(u * 37.0)),
            channel(wave(v * 53.0 + 0.3)),
            channel(wave((u + v) * 29.0 + 0.6)),
            255,
        ])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Arc::new(Tile::decode(bytes.get_ref()).unwrap())
}

/// The relief walk's passes timed on the device, alone (`GLOBECRAFT_RELIEF_PROBE_SIZE`
/// for the size): the fastest of five walks of each view once its tiles are held, and
/// what the walk found (to see a faster walk still finds the same grounds).
#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored --nocapture"]
fn gpu_relief_walk_timing() {
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
    let tiles: std::sync::Mutex<HashMap<TileKey, Arc<Tile>>> = Default::default();
    let lookup = |key: TileKey| {
        let (provider, z, x, y, _) = globecraft_data::tiles::unkey(key);
        if provider == Provider::Satellite && z > 14 {
            return Lookup::Unavailable;
        }
        let mut tiles = tiles.lock().unwrap();
        Lookup::Ready(Arc::clone(tiles.entry(key).or_insert_with(|| {
            if provider == Provider::TerrainDem {
                crate::tests::probe_dem_tile(z, x, y)
            } else {
                crate::tests::probe_image_tile(z, x, y)
            }
        })))
    };
    let (width, height) = std::env::var("GLOBECRAFT_RELIEF_PROBE_SIZE")
        .ok()
        .and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((320, 200));
    // `GLOBECRAFT_RELIEF_ABLATION=<name>`: the walk with one of its parts done twice
    // (the copy adds nothing to the results), so the time it adds is what that part
    // costs on this GPU (no shader profiler on an M1 for compute work).
    if let Ok(ablation) = std::env::var("GLOBECRAFT_RELIEF_ABLATION") {
        *relief::SHADER_PATCH.lock().unwrap() = walk_ablation(&ablation);
        println!("ABLATION {ablation}");
    }
    let march = gpu
        .relief
        .get_or_insert_with(|| Some(relief::ReliefMarch::new(&gpu.device, 128)))
        .as_mut()
        .unwrap();
    for (name, alt, tilt, heading) in [
        ("top-16km", 16.0, 0.0, 0.0),
        ("tilt-16km", 16.0, 70.0, 0.0),
        ("tilt-52km", 52.0, 75.0, 0.0),
        ("tilt-6km", 6.0, 72.0, 30.0),
    ] {
        // `GLOBECRAFT_RELIEF_PROBE_VIEWS=tilt-16km` walks only those (a GPU capture).
        if std::env::var("GLOBECRAFT_RELIEF_PROBE_VIEWS")
            .is_ok_and(|only| !only.split(',').any(|view| view == name))
        {
            continue;
        }
        let cam = Cam {
            lat: 30.3,
            lon: 79.8,
            heading,
            alt,
            tilt,
            look_up: 0.0,
        };
        let mut cfg = crate::tests::batch_reference_config(Provider::Satellite);
        cfg.grid = false;
        cfg.terrain = Some(crate::TerrainCfg::new(1.0, 1.0));
        cfg.sun = Some(globecraft_core::math::ll_to_v3(15.0, -10.0));
        let request = RenderRequest {
            width,
            height,
            cam,
            basis: cam.basis_for_model(
                width as f64 / height as f64,
                globecraft_core::earth_model::EarthModel::Wgs84,
            ),
            cfg,
            overlays: None,
            aircraft: None,
            live_layers: None,
            presentation: 0,
            compare: None,
        };
        let held = crate::render_pixels_with(
            width,
            height,
            0..height,
            &request.basis,
            &request.cfg,
            &lookup,
            Vec::new(),
            None,
        )
        .relief_tiles;
        let grown = relief::grown_tiles(&held, lookup, std::time::Instant::now());
        let mut best: Option<[std::time::Duration; relief::TIMED_PASSES]> = None;
        let mut planes = Vec::new();
        // `GLOBECRAFT_RELIEF_PROBE_RUNS`: walks of each view (a GPU capture: 2).
        let runs = std::env::var("GLOBECRAFT_RELIEF_PROBE_RUNS")
            .ok()
            .and_then(|runs| runs.parse().ok())
            .unwrap_or(8);
        for run in 0..runs {
            let walked = march
                .starts(&gpu.device, &gpu.queue, &request, &grown)
                .expect("relief march");
            // The first runs upload the tiles.
            if let (Some(took), true) = (walked.took.0, run >= 3) {
                best =
                    Some(best.map_or(took, |best| std::array::from_fn(|i| best[i].min(took[i]))));
            }
            planes = walked.pixels;
        }
        // The whole walk as a frame takes it, alternately with its planes read back and
        // left on the device (the GPU's frames): the fastest of each.
        let (mut read_back, mut on_device) = (std::time::Duration::MAX, std::time::Duration::MAX);
        for _ in 0..runs.max(2) {
            let started = std::time::Instant::now();
            march
                .starts(&gpu.device, &gpu.queue, &request, &grown)
                .expect("relief march");
            read_back = read_back.min(started.elapsed());
            let started = std::time::Instant::now();
            march
                .walk_on_device(&gpu.device, &gpu.queue, &request, &grown)
                .expect("relief march");
            on_device = on_device.min(started.elapsed());
        }
        println!(
            "WALK {name}: the whole walk {:.2} ms with its planes read back, {:.2} ms with them left on the device",
            read_back.as_secs_f64() * 1000.0,
            on_device.as_secs_f64() * 1000.0
        );
        let count = width * height;
        let hits: Vec<f64> = planes[count..2 * count]
            .iter()
            .filter(|&&hit| hit >= 0.0)
            .map(|&hit| f64::from(hit))
            .collect();
        let trusted = planes[3 * count..4 * count]
            .iter()
            .filter(|&&t| t > 0.5)
            .count();
        let seen: f64 = planes[2 * count..3 * count]
            .iter()
            .filter(|&&s| s >= 0.0)
            .map(|&s| f64::from(s))
            .sum();
        // A pass the device refused walks nothing, and says so only here.
        let failure = gpu
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        assert!(failure.is_none(), "{name}: {failure:?}");
        assert!(
            hits.iter().any(|&hit| hit > 0.0),
            "{name}: the walk met no ground"
        );
        println!(
            "WALK {name}: on the device {:?} (the cones, the view, the sun's quarter, its other rows, its odd columns); {} grounds (mean distance {:.6e}), {trusted} trusted, sun's shares summing to {seen:.1}",
            best.map(|best| best.map(|d| d.as_secs_f64() * 1000.0)),
            hits.len(),
            hits.iter().sum::<f64>() / hits.len().max(1) as f64,
        );
    }
}

/// A camera that moved into tiles the GPU does not hold yet (the finest levels round
/// it): the walk stands in coarser held levels over the open air, so it still answers
/// most rays, and none of its starts passes the ground.
#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored --nocapture"]
fn a_level_not_held_yet_is_stood_in_for_over_the_open_air() {
    // The finest level the walk is given in this view.
    const HELD_LEVELS: u8 = 12;
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
    let tiles = std::sync::Mutex::new(HashMap::<TileKey, Arc<Tile>>::new());
    let lookup = |key: TileKey| {
        let (provider, z, x, y, _) = globecraft_data::tiles::unkey(key);
        if provider == Provider::Satellite && z > 14 {
            return Lookup::Unavailable;
        }
        let mut tiles = tiles.lock().unwrap();
        Lookup::Ready(Arc::clone(tiles.entry(key).or_insert_with(|| {
            if provider == Provider::TerrainDem {
                crate::tests::probe_dem_tile(z, x, y)
            } else {
                crate::tests::probe_image_tile(z, x, y)
            }
        })))
    };
    let (width, height) = (324, 203);
    let cam = Cam {
        lat: 30.3,
        lon: 79.8,
        heading: 0.0,
        alt: 16.0,
        tilt: 70.0,
        look_up: 0.0,
    };
    let mut cfg = crate::tests::batch_reference_config(Provider::Satellite);
    cfg.grid = false;
    cfg.terrain = Some(crate::TerrainCfg::new(1.0, 1.0));
    cfg.sun = Some(ll_to_v3(15.0, -10.0));
    let request = RenderRequest {
        width,
        height,
        cam,
        basis: cam.basis_for_model(
            width as f64 / height as f64,
            globecraft_core::earth_model::EarthModel::Wgs84,
        ),
        cfg,
        overlays: None,
        aircraft: None,
        live_layers: None,
        presentation: 0,
        compare: None,
    };
    let held = crate::render_pixels_with(
        width,
        height,
        0..height,
        &request.basis,
        &request.cfg,
        &lookup,
        Vec::new(),
        None,
    )
    .relief_tiles;
    let finest = held
        .iter()
        .map(|&key| globecraft_data::tiles::unkey(key).1)
        .max()
        .unwrap();
    assert!(finest > HELD_LEVELS + 1, "the view needs level {finest}");
    let coarse: Vec<TileKey> = held
        .into_iter()
        .filter(|&key| globecraft_data::tiles::unkey(key).1 <= HELD_LEVELS)
        .collect();
    let march = gpu
        .relief
        .get_or_insert_with(|| Some(relief::ReliefMarch::new(&gpu.device, 128)))
        .as_mut()
        .unwrap();
    let grown = relief::grown_tiles(&coarse, lookup, std::time::Instant::now());
    let starts = march
        .starts(&gpu.device, &gpu.queue, &request, &grown)
        .expect("relief march")
        .pixels;
    let count = width * height;
    let gave_up = starts[3 * count..]
        .iter()
        .filter(|&&trusted| trusted < 0.0)
        .count();
    println!(
        "levels to {HELD_LEVELS} of {finest} held: the walk gave up on {gave_up} of {count} rays"
    );
    assert!(relief::helps(&starts), "gave up on {gave_up} of {count}");
    let hits = crate::relief_hits(
        width,
        height,
        &request.basis,
        &request.cfg,
        &lookup,
        crate::HitMarch::Exact,
    );
    let past = starts[..count]
        .iter()
        .zip(&hits)
        .filter(|&(&start, &(hit, _))| hit.is_finite() && f64::from(start) > hit * (1.0 + 1e-4))
        .count();
    assert_eq!(past, 0, "starts past the ground");
}

/// The shader patches of a walk ablation (`gpu_relief_walk_timing`): each does one part
/// of a step a second time and adds it times a zero the compiler cannot see.
fn walk_ablation(name: &str) -> Vec<(String, String)> {
    let zero = (
        "fn walk(".to_string(),
        "fn dbg_zero() -> f32 { return f32(relief.raster.w >> 31u); }\nfn walk(".to_string(),
    );
    let patch = |from: &str, to: &str| (from.to_string(), to.to_string());
    let mut patches = vec![zero];
    patches.extend(match name {
        "none" => vec![],
        // The point's place on the map (atan2, log, normalize).
        "spot" => vec![patch(
            "        let at = spot(p, dir);\n",
            "        let at = spot(p, dir);\n        let at_dup = spot(p * (1.0 + 1e-7 * dbg_zero()) + dir * 1e-9, dir);\n        t += dbg_zero() * (at_dup.rate.x + at_dup.uv.y + at_dup.climb);\n",
        )],
        // The level from the distance.
        "zoom" => vec![patch(
            "        let wanted = clamp(",
            "        t += dbg_zero() * log2(2.0 * PI * EARTH_RADIUS_M * max(at.latitude_cosine, MIN_LATITUDE_COSINE) / (DEM_TEXELS * max(t * 1.0001 * EARTH_RADIUS_M * angle, 0.01) * PIXELS_PER_TEXEL));\n        let wanted = clamp(",
        )],
        // A tile looked up in the hash table every step.
        "hash" => vec![patch(
            "        var level = wanted;\n",
            "        var level = wanted;\n        t += dbg_zero() * f32(layer_of(u32(wanted), u32(at.uv.x * 4096.0), u32(at.uv.y * 4096.0)));\n",
        )],
        // A second block maximum, another mip, at every mip tried.
        "mip" => vec![patch(
            "            var top = block_top(own, mip);\n",
            "            var top = block_top(own, mip);\n            top = max(top, block_top(own, max(mip - 1, 0)) - 1e30 * (1.0 - dbg_zero()));\n",
        )],
        // A second texel maximum (the next texel) at every texel step.
        "peaks" => vec![patch(
            "        var top = texel_top(own);\n",
            "        var top = texel_top(own);\n        top = max(top, texel_top(Place(own.layer, own.at + vec2<f32>(1.0, 1.0))) - 1e30 * (1.0 - dbg_zero()));\n",
        )],
        // The cell's heights along the ray twice, at the next cell.
        "cell" => vec![patch(
            "            along = bilinear_along(own, rate);\n",
            "            along = bilinear_along(own, rate);\n            along += dbg_zero() * bilinear_along(Place(own.layer, own.at + vec2<f32>(1.0, 1.0)), rate);\n",
        )],
        // Not a doubling: the walk with one tile cache for the levels on either side
        // and the stand-ins (fewer registers: 576 threads a group on an M1, not 512).
        "caches" => vec![
            patch(
                "    var fine_looked = own_looked;\n    var fallback_looked = own_looked;\n",
                "",
            ),
            patch("&fallback_looked)", "&coarse_looked)"),
            patch(
                "fine_at = locate(level + 1, d, at.uv, &fine_looked)",
                "fine_at = locate(level + 1, d, at.uv, &coarse_looked)",
            ),
        ],
        other => panic!("unknown ablation {other}"),
    });
    patches
}

/// `GLOBECRAFT_RELIEF_ABLATION=<name>` in the parity tests: the walk's variant must
/// keep the CPU's picture before it is timed.
fn relief_variant() {
    if let Ok(name) = std::env::var("GLOBECRAFT_RELIEF_ABLATION") {
        *relief::SHADER_PATCH.lock().unwrap() = walk_ablation(&name);
        println!("ABLATION {name}");
    }
}

/// `GLOBECRAFT_OVERVIEW_VARIANT=<name>`: the overview shader with a variant, to time it
/// against the shader as it is (the parity tests print the device's time).
fn overview_variant() {
    let Ok(name) = std::env::var("GLOBECRAFT_OVERVIEW_VARIANT") else {
        return;
    };
    // A variant is a list of (from, to) replacements in the shader's source; none is
    // kept now (the air tables' loops and the area average's order were timed,
    // 2026-11-26).
    *super::shader::SHADER_PATCH.lock().unwrap() = match name.as_str() {
        "none" => vec![],
        other => panic!("unknown overview variant {other}"),
    };
    println!("OVERVIEW VARIANT {name}");
}
