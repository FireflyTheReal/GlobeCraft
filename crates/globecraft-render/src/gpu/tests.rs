use super::*;
use globecraft_core::{camera::Cam, math::ll_to_v3};
use globecraft_data::tiles::{Lookup, Provider, make_key};

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored --nocapture"]
fn gpu_overview_matches_cpu() {
    let mut gpu = GpuRenderer::new().expect("native GPU adapter and validated shader");
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
    let (w, h) = (65, 49);
    // The GPU matches the CPU within one level per channel almost everywhere: its f32
    // trigonometry moves a few texel lookups on the test pattern's sharp edges. The
    // error statistics are reported and bounded (`PARITY_SHARE_OVER_ONE`). A software
    // rasterizer (accepted only with GLOBECRAFT_GPU_ALLOW_SOFTWARE, for CI without a
    // GPU) may hand a frame back to the CPU, as the app does; a GPU must not.
    let software = gpu.software;
    let (mut channels, mut over_one, mut total_error, mut fallbacks) =
        (0usize, 0usize, 0u64, 0usize);
    for provider in Provider::basemaps() {
        for (lat, lon, alt) in [
            (-15.0, -50.0, 14000.0),
            (89.9, 179.9, 1000.0),
            (-89.9, -179.9, 14000.0),
            (0.0, 180.0, 30000.0),
        ] {
            for enabled in [false, true] {
                let cam = Cam {
                    lat,
                    lon,
                    alt,
                    heading: 25.0,
                    tilt: 0.0,
                };
                let request = RenderRequest {
                    width: w,
                    height: h,
                    cam,
                    basis: cam.basis(w as f64 / h as f64),
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
                        night_lights: false,
                        stars: true,
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
                            vec![(key, tile.clone())]
                        } else {
                            Vec::new()
                        })
                        .unwrap();
                    let lookup = |k| {
                        if loaded && k == key {
                            Lookup::Ready(tile.clone())
                        } else if pending && k == key {
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
                            println!("FALLBACK {provider:?} ({lat},{lon}) alt={alt}: {error}");
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
                            "DIFF {provider:?} ({lat},{lon}) alt={alt} enabled={enabled} loaded={loaded} pending={pending}: {} channels >1, max {max}, at {pixels:?}",
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
            night_lights: false,
            stars: true,
        },
        overlays: None,
        aircraft: None,
        live_layers: None,
        presentation: 0,
        compare: None,
    };
    assert!(GpuRenderer::supports(&request));
    request.basis = request.cam.basis_for_model(
        request.width as f64 / request.height as f64,
        globecraft_core::earth_model::EarthModel::Wgs84,
    );
    assert!(!GpuRenderer::supports(&request));
    request.basis = request
        .cam
        .basis(request.width as f64 / request.height as f64);
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
            night_lights: false,
            stars: true,
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
