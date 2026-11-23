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
    request.cam.alt = 0.15;
    assert!(!GpuRenderer::supports(&request));
    request.cam.alt = 14000.0;
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
        .unwrap_or((320, 200));
    for (name, alt, tilt, heading) in [
        ("top-16km", 16.0, 0.0, 0.0),
        ("tilt-16km", 16.0, 70.0, 0.0),
        ("tilt-52km", 52.0, 75.0, 0.0),
        ("tilt-6km", 6.0, 72.0, 30.0),
        ("tilt85-3km", 3.0, 85.0, 30.0),
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
        // tiles the CPU's last frame held and the ones the last walk read (a few
        // more uploaded each frame). Once all are in, every frame takes as many of
        // the GPU's grounds (they did not come and go, 2026-11-23).
        let count = width * height;
        let taken = |pixels: &[f32]| pixels[3 * count..].iter().filter(|&&t| t > 0.5).count();
        let mut tiles = held;
        let mut taken_by_frame = Vec::new();
        let mut starts = Vec::new();
        for _ in 0..16 {
            let grown = relief::grown_tiles(&tiles, lookup, std::time::Instant::now());
            let walked = march
                .starts(&gpu.device, &gpu.queue, &request, &grown)
                .expect("relief march");
            taken_by_frame.push(taken(&walked.pixels));
            let frame = render(Some(&walked.pixels)).0;
            tiles = relief::next_tiles(frame.relief_tiles, &walked.read);
            starts = walked.pixels;
        }
        let most = *taken_by_frame.iter().max().unwrap();
        println!("{name}: grounds taken frame by frame {taken_by_frame:?}");
        assert!(
            taken_by_frame[8..]
                .iter()
                .all(|&taken| taken * 10 >= most * 9),
            "{name}: {taken_by_frame:?}"
        );
        let walked = std::time::Instant::now();
        let grown = relief::grown_tiles(&tiles, lookup, std::time::Instant::now());
        march
            .starts(&gpu.device, &gpu.queue, &request, &grown)
            .expect("relief march");
        let walk_time = walked.elapsed();
        println!("{name}: the GPU's walk {walk_time:?}");
        let (frame, gpu_steps) = render(Some(&starts));
        let pixels = frame.pixels;
        let result = parity(&pixels, &expected);
        // The march started from the camera: how much the start alone moves the
        // picture (the CPU's linear steps between exact measurements).
        let mut zeros = vec![0.0; width * height];
        zeros.resize(4 * width * height, -1.0);
        let from_camera = render(Some(&zeros)).0.pixels;
        let hits = crate::relief_hits(width, height, &request.basis, &request.cfg, &lookup);
        let (mut past, mut share, mut counted, mut most) = (0, 0.0, 0, 0.0f64);
        let known = starts[2 * count..3 * count]
            .iter()
            .filter(|&&seen| seen >= 0.0)
            .count();
        let met = starts[count..2 * count]
            .iter()
            .filter(|&&hit| hit >= 0.0)
            .count();
        let trusted = starts[3 * count..]
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
            "{name}: the CPU's part {:?} after the GPU's walk ({walk_time:?}), {:?} with the cones, {flat_time:?} without relief",
            timed(Some(&starts)),
            timed(None)
        );
        assert_eq!(
            relief::past_the_ground(&starts[..count], &hits),
            0,
            "{name}"
        );
        assert!(gpu_steps <= cpu_steps, "{name}");
        let wrong = relief::trusted_past_the_ground(&starts, &hits);
        println!("{name}: {wrong} grounds taken from the GPU past the CPU's");
        assert!(wrong * relief::TRUSTED_PAST_PER <= count, "{name}");
    }
    let march = gpu.relief.take().flatten().unwrap();
    drop(march);
    let mut checked = relief::ReliefMarch::new(&gpu.device, 128);
    checked
        .self_check(&gpu.device, &gpu.queue)
        .expect("the self-check passes");
}
