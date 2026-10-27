use super::*;
use globecraft_core::{camera::Cam, math::ll_to_v3};

struct ReferenceFrame {
    name: &'static str,
    cfg: RenderCfg,
    loaded: bool,
    expected: &'static [u8],
}

#[test]
fn raster_matches_regression_fixtures() {
    let (w, h) = (64, 48);
    let basis = Cam {
        lat: -15.0,
        lon: -50.0,
        heading: 25.0,
        alt: 14000.0,
        tilt: 0.0,
    }
    .basis(w as f64 / h as f64);
    let config = |grid, atmo, sun| RenderCfg {
        sky: None,
        provider: Provider::Satellite,
        terrain: None,
        rel: 0,
        grid,
        grid_step: 10.0,
        atmo,
        sun,
        lod_scale: 1.0,
        level_blend: true,
        fixed_level: None,
        night_lights: false,
        stars: true,
    };
    let sun = Some(ll_to_v3(20.0, 45.0));
    let scenarios = [
        ReferenceFrame {
            name: "plain",
            cfg: config(false, false, None),
            loaded: false,
            expected: include_bytes!("../tests/fixtures/plain.rgba"),
        },
        ReferenceFrame {
            name: "atmosphere",
            cfg: config(false, true, None),
            loaded: false,
            expected: include_bytes!("../tests/fixtures/atmosphere.rgba"),
        },
        ReferenceFrame {
            name: "grid-sun",
            cfg: config(true, true, sun),
            loaded: false,
            expected: include_bytes!("../tests/fixtures/grid-sun.rgba"),
        },
        ReferenceFrame {
            name: "loaded",
            cfg: config(true, true, sun),
            loaded: true,
            expected: include_bytes!("../tests/fixtures/loaded.rgba"),
        },
    ];
    let tile = Arc::new(Tile::decode(include_bytes!("../tests/fixtures/tile.png")).unwrap());
    for ReferenceFrame {
        name,
        cfg,
        loaded,
        expected,
    } in scenarios
    {
        let lookup = |key| {
            if loaded && key == make_key(Provider::Satellite, 0, 0, 0, 0) {
                Lookup::Ready(tile.clone())
            } else {
                Lookup::Absent
            }
        };
        let (buf, misses) = render_pixels(w, h, &basis, &cfg, &lookup);
        assert!(!misses.is_empty(), "missing tiles must still be reported");
        assert_eq!(buf.len(), expected.len());
        for (i, (actual, reference)) in buf.iter().zip(expected).enumerate() {
            // Permit one quantization step for platform floating-point differences.
            assert!(
                actual.abs_diff(*reference) <= 1,
                "{name}: channel {i}: {actual} != {reference}"
            );
        }
    }
}

#[test]
fn local_tile_lookup_is_deduplicated_and_keeps_the_nearest_miss() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let lookup = |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Lookup::Absent
    };
    let mut loc = Local {
        lookup: &lookup,
        map: HashMap::default(),
        dem: Vec::new(),
        dem_index: HashMap::default(),
    };
    for dist in [100.0, 80.0, 40.0, 30.0, 50.0] {
        assert!(loc.tile_color(1, dist, 0.0, 0.0, 1.0).is_none());
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(loc.misses().collect::<Vec<_>>(), vec![(1, 30.0)]);
}
struct BatchReference {
    name: &'static str,
    width: usize,
    height: usize,
    provider: Provider,
    cam: Cam,
}
fn batch_references() -> [BatchReference; 6] {
    [
        BatchReference {
            name: "odd-satellite",
            width: 65,
            height: 49,
            provider: Provider::Satellite,
            cam: Cam {
                lat: -15.0,
                lon: -50.0,
                heading: 25.0,
                alt: 14_000.0,
                tilt: 0.0,
            },
        },
        BatchReference {
            name: "polar-streets",
            width: 67,
            height: 51,
            provider: Provider::Streets,
            cam: Cam {
                lat: 89.9,
                lon: 179.9,
                heading: -130.0,
                alt: 1_000.0,
                tilt: 0.0,
            },
        },
        BatchReference {
            name: "dateline-topo",
            width: 63,
            height: 47,
            provider: Provider::Topo,
            cam: Cam {
                lat: -40.0,
                lon: 179.9,
                heading: 70.0,
                alt: 500.0,
                tilt: 0.0,
            },
        },
        BatchReference {
            name: "narrow-wayback",
            width: 3,
            height: 9,
            provider: Provider::Wayback,
            cam: Cam {
                lat: 0.0,
                lon: -180.0,
                heading: 180.0,
                alt: 30_000.0,
                tilt: 0.0,
            },
        },
        BatchReference {
            name: "one-pixel-column",
            width: 1,
            height: 7,
            provider: Provider::Satellite,
            cam: Cam {
                lat: -15.0,
                lon: -50.0,
                heading: 25.0,
                alt: 14_000.0,
                tilt: 0.0,
            },
        },
        BatchReference {
            name: "low-altitude",
            width: 7,
            height: 5,
            provider: Provider::Satellite,
            cam: Cam {
                lat: 89.9,
                lon: -179.9,
                heading: -20.0,
                alt: globecraft_core::MIN_CAMERA_ALTITUDE_KM,
                tilt: 0.0,
            },
        },
    ]
}
fn batch_reference_config(provider: Provider) -> RenderCfg {
    RenderCfg {
        sky: None,
        provider,
        terrain: None,
        rel: 7,
        grid: true,
        grid_step: 10.0,
        atmo: true,
        sun: Some(ll_to_v3(20.0, 45.0)),
        lod_scale: 1.0,
        level_blend: true,
        fixed_level: None,
        night_lights: false,
        stars: true,
    }
}

#[test]
fn ray_batches_preserve_partial_rows_provider_pixels_and_missing_tile_priorities() {
    let tile = Arc::new(Tile::decode(include_bytes!("../tests/fixtures/tile.png")).unwrap());
    let frames: [&[u8]; 6] = [
        include_bytes!("../tests/fixtures/batches/odd-satellite.rgba"),
        include_bytes!("../tests/fixtures/batches/polar-streets.rgba"),
        include_bytes!("../tests/fixtures/batches/dateline-topo.rgba"),
        include_bytes!("../tests/fixtures/batches/narrow-wayback.rgba"),
        include_bytes!("../tests/fixtures/batches/one-pixel-column.rgba"),
        include_bytes!("../tests/fixtures/batches/low-altitude.rgba"),
    ];
    let expected_misses: [&[u8]; 6] = [
        include_bytes!("../tests/fixtures/batches/odd-satellite.misses"),
        include_bytes!("../tests/fixtures/batches/polar-streets.misses"),
        include_bytes!("../tests/fixtures/batches/dateline-topo.misses"),
        include_bytes!("../tests/fixtures/batches/narrow-wayback.misses"),
        include_bytes!("../tests/fixtures/batches/one-pixel-column.misses"),
        include_bytes!("../tests/fixtures/batches/low-altitude.misses"),
    ];
    for ((case, expected), expected_misses) in batch_references()
        .into_iter()
        .zip(frames)
        .zip(expected_misses)
    {
        let cfg = batch_reference_config(case.provider);
        let lookup = |key| {
            if key == make_key(case.provider, 0, 0, 0, 7) {
                Lookup::Ready(tile.clone())
            } else {
                Lookup::Absent
            }
        };
        let (buf, mut misses) = render_pixels(
            case.width,
            case.height,
            &case.cam.basis(case.width as f64 / case.height as f64),
            &cfg,
            &lookup,
        );
        assert_eq!(buf.len(), expected.len(), "{}", case.name);
        for (i, (actual, reference)) in buf.iter().zip(expected).enumerate() {
            assert!(
                actual.abs_diff(*reference) <= 1,
                "{}: channel {i}: {actual} != {reference}",
                case.name
            );
        }
        misses.sort_unstable_by_key(|&(key, _)| key);
        let bytes: Vec<u8> = misses
            .into_iter()
            .flat_map(|(key, dist)| key.to_le_bytes().into_iter().chain(dist.to_le_bytes()))
            .collect();
        assert_eq!(
            bytes, expected_misses,
            "{}: padding must not add requests or change priority",
            case.name
        );
    }
}

#[test]
fn tracked_frames_watch_pending_tiles_without_enqueuing_them_again() {
    let case = &batch_references()[0];
    let cfg = batch_reference_config(case.provider);
    let basis = case.cam.basis(case.width as f64 / case.height as f64);
    let (_, missing, waiting) =
        render_pixels_tracked(case.width, case.height, &basis, &cfg, &|_| Lookup::Pending);
    assert!(missing.is_empty());
    assert!(!waiting.is_empty());
    let (_, absent, absent_waiting) =
        render_pixels_tracked(case.width, case.height, &basis, &cfg, &|_| Lookup::Absent);
    let keys: std::collections::HashSet<_> = absent.into_iter().map(|(key, _)| key).collect();
    assert_eq!(keys, waiting.into_iter().collect());
    assert_eq!(keys, absent_waiting.into_iter().collect());
}

#[test]
fn unavailable_tiles_fall_back_to_coarser_levels_without_waiting() {
    let case = &batch_references()[0];
    let cfg = batch_reference_config(case.provider);
    let basis = case.cam.basis(case.width as f64 / case.height as f64);
    // Only the root tile exists; every finer level is unavailable at the source.
    let root = Arc::new(Tile::decode(include_bytes!("../tests/fixtures/tile.png")).unwrap());
    let lookup = |key: TileKey| {
        if unkey(key).1 == 0 {
            Lookup::Ready(root.clone())
        } else {
            Lookup::Unavailable
        }
    };
    let (_, missing, waiting) =
        render_pixels_tracked(case.width, case.height, &basis, &cfg, &lookup);
    assert!(
        missing.is_empty(),
        "unavailable tiles must not be requested"
    );
    assert!(waiting.is_empty(), "nothing is left to wait for");
}

#[test]
fn reused_buffers_overwrite_all_pixels_and_preserve_allocation() {
    let case = &batch_references()[0];
    let cfg = batch_reference_config(case.provider);
    let basis = case.cam.basis(case.width as f64 / case.height as f64);
    let lookup = |_| Lookup::Absent;
    let (expected, _, _) = render_pixels_tracked(case.width, case.height, &basis, &cfg, &lookup);
    let mut buffer = vec![123; expected.len() + 128];
    let pointer = buffer.as_ptr();
    buffer.truncate(expected.len());
    let (pixels, _, _) = render_pixels_into(
        case.width,
        case.height,
        0..case.height,
        &basis,
        &cfg,
        &lookup,
        buffer,
    );
    assert_eq!(pixels.as_ptr(), pointer);
    assert_eq!(pixels, expected);
}

#[test]
fn celestial_backdrop_changes_only_sky_and_preserves_tile_priorities() {
    let (width, height) = (65, 49);
    let cam = Cam {
        lat: 40.0,
        lon: -100.0,
        heading: 20.0,
        alt: 14_000.0,
        tilt: 0.0,
    };
    let basis = cam.basis(width as f64 / height as f64);
    let mut cfg = batch_reference_config(Provider::Satellite);
    let (off, off_misses) = render_pixels(width, height, &basis, &cfg, &|_| Lookup::Absent);
    let image = Arc::new(
        globecraft_data::sky::SkyImage::decode(include_bytes!(
            "../../../assets/sky/milkyway-2020-4k-sdr.jpg"
        ))
        .unwrap(),
    );
    cfg.sky = Some(
        crate::celestial::SkyBackdrop::new(image, 1_760_000_000.0, cam, DVec3::X, true).unwrap(),
    );
    let (on, on_misses) = render_pixels(width, height, &basis, &cfg, &|_| Lookup::Absent);
    assert_eq!(
        off_misses
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>(),
        on_misses
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    let rays = RayCaster::new(width, height, &basis);
    let mut changed = 0;
    for y in 0..height {
        for x in 0..width {
            let ray = &rays.cast(x, y)[0];
            let offset = (y * width + x) * 4;
            if ray.discriminant >= 0.0 && ray.camera_dot < 0.0 {
                assert_eq!(&off[offset..offset + 4], &on[offset..offset + 4]);
            } else if off[offset..offset + 4] != on[offset..offset + 4] {
                changed += 1;
            }
        }
    }
    assert!(changed > 100);
}

#[test]
fn filter_ratio_exponent_threshold_is_a_1_25_texels_per_pixel_ratio() {
    assert!((tuning::TILE_FILTER_RATIO_EXONENT_MIN.exp2() - 1.25).abs() < 1e-12);
}

/// A 256-pixel tile of one colour, as the network loader would decode it.
fn solid_tile(rgb: [u8; 3]) -> Arc<Tile> {
    let image = image::RgbaImage::from_pixel(256, 256, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    Arc::new(Tile::decode(&bytes).unwrap())
}

#[test]
fn without_level_blend_every_pixel_shows_one_level() {
    // Even levels red, odd levels blue: a blended pixel mixes both.
    let (red, blue) = (solid_tile([255, 0, 0]), solid_tile([0, 0, 255]));
    let lookup = |key: TileKey| {
        if unkey(key).1.is_multiple_of(2) {
            Lookup::Ready(red.clone())
        } else {
            Lookup::Ready(blue.clone())
        }
    };
    let (width, height) = (96, 64);
    // A tilted view spans several levels, so some pixels sit in the fade band.
    let basis = Cam {
        lat: 45.0,
        lon: 7.0,
        heading: 0.0,
        alt: 300.0,
        tilt: 70.0,
    }
    .basis(width as f64 / height as f64);
    let mut cfg = batch_reference_config(Provider::Satellite);
    cfg.atmo = false;
    cfg.sky = None;
    cfg.sun = None;
    // The same view in each colour alone: a single-level pixel equals one of them
    // (up to rounding in the texel averaging).
    let near = |a: &[u8], b: &[u8]| a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 2);
    let (all_red, _, _) =
        render_pixels_tracked(width, height, &basis, &cfg, &|_| Lookup::Ready(red.clone()));
    let (all_blue, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &|_| {
        Lookup::Ready(blue.clone())
    });
    let mixed = |pixels: &[u8]| {
        pixels
            .chunks(RGBA_CHANNELS)
            .zip(all_red.chunks(RGBA_CHANNELS))
            .zip(all_blue.chunks(RGBA_CHANNELS))
            .filter(|((pixel, red), blue)| !near(pixel, red) && !near(pixel, blue))
            .count()
    };
    let (blended, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    cfg.level_blend = false;
    let (single, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    assert!(
        mixed(&blended) > 0,
        "the fixture view must cross a fade band"
    );
    assert_eq!(mixed(&single), 0);
}

/// Synthetic mountains around 30.3° N 79.8° E (heights in metres).
fn probe_height(lat: f64, lon: f64) -> f32 {
    let (x, y) = ((lon - 79.8) * 96.0, (lat - 30.3) * 111.0);
    let h = 2500.0
        + 2000.0 * (x / 7.0).sin() * (y / 9.0).cos()
        + 1200.0 * ((x + y) / 3.3).sin()
        + 400.0 * (x / 0.9).sin() * (y / 1.1).sin();
    h.max(0.0) as f32
}

fn probe_dem_tile(z: u8, tx: u32, ty: u32) -> Arc<Tile> {
    let size = 512;
    let n = f64::from(1u32 << z);
    let heights = (0..size * size)
        .map(|i| {
            let (px, py) = ((i % size) as f64 + 0.5, (i / size) as f64 + 0.5);
            let u = (f64::from(tx) + px / size as f64) / n;
            let v = (f64::from(ty) + py / size as f64) / n;
            let lon = u * 360.0 - 180.0;
            let lat = (PI * (1.0 - 2.0 * v)).sinh().atan().to_degrees();
            probe_height(lat, lon)
        })
        .collect();
    Arc::new(Tile::from_elevation(
        globecraft_data::tiles::Elevation::from_heights(size, heights),
    ))
}

fn probe_image_tile(z: u8, tx: u32, ty: u32) -> Arc<Tile> {
    let hue = [(200, 80, 60), (60, 160, 80), (70, 90, 200), (190, 170, 60)]
        [((tx + ty + u32::from(z)) % 4) as usize];
    let image = image::RgbaImage::from_fn(256, 256, |x, y| {
        let line = x % 32 == 0 || y % 32 == 0;
        let c = if line { (240, 240, 240) } else { hue };
        image::Rgba([c.0, c.1, c.2, 255])
    });
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    Arc::new(Tile::decode(&bytes).unwrap())
}

/// Timed renders per view in `terrain_views` (the fastest counts).
const PROBE_RUNS: usize = 5;

#[test]
#[ignore = "manual: renders synthetic relief views to GLOBECRAFT_PROBE_DIR with timings"]
fn terrain_views() {
    use std::collections::HashMap;
    use std::sync::Mutex;
    let dir = std::path::PathBuf::from(std::env::var("GLOBECRAFT_PROBE_DIR").unwrap());
    let tiles: Mutex<HashMap<TileKey, Arc<Tile>>> = Mutex::new(HashMap::new());
    let lookup = |key: TileKey| {
        let (provider, z, x, y, _) = unkey(key);
        if provider == Provider::Satellite && z > 14 {
            return Lookup::Unavailable;
        }
        let mut tiles = tiles.lock().unwrap();
        let tile = tiles.entry(key).or_insert_with(|| {
            if provider == Provider::TerrainDem {
                probe_dem_tile(z, x, y)
            } else {
                probe_image_tile(z, x, y)
            }
        });
        Lookup::Ready(tile.clone())
    };
    let (width, height) = (960, 600);
    for (name, alt, tilt, heading) in [
        ("top-16km", 16.0, 0.0, 0.0),
        ("tilt-16km", 16.0, 70.0, 0.0),
        ("tilt-52km", 52.0, 75.0, 0.0),
        ("tilt-6km", 6.0, 72.0, 30.0),
    ] {
        if std::env::var("PROBE_VIEW").is_ok_and(|only| only != name) {
            continue;
        }
        let basis = Cam {
            lat: 30.3,
            lon: 79.8,
            heading,
            alt,
            tilt,
        }
        .basis_for_model(
            width as f64 / height as f64,
            globecraft_core::earth_model::EarthModel::Wgs84,
        );
        let mut cfg = batch_reference_config(Provider::Satellite);
        cfg.sky = None;
        cfg.atmo = true;
        cfg.terrain = Some(crate::TerrainCfg::new(1.0));
        // Warm the tile map, then time: the fastest of a few runs, as the machine's
        // other work only ever adds time.
        let (mut pixels, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
        let fastest = |cfg: &RenderCfg, pixels: &mut Vec<u8>| {
            (0..PROBE_RUNS)
                .map(|_| {
                    let started = std::time::Instant::now();
                    *pixels = render_pixels_tracked(width, height, &basis, cfg, &lookup).0;
                    started.elapsed()
                })
                .min()
                .unwrap()
        };
        let elapsed = fastest(&cfg, &mut pixels);
        // Counted on one thread, outside the timing.
        crate::terrain::COUNTS.with(|c| c.set((0, 0)));
        let (steps, lookups) = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| {
                crate::terrain::COUNTS.with(|c| c.set((0, 0)));
                render_pixels_tracked(width, height, &basis, &cfg, &lookup);
                crate::terrain::COUNTS.with(|c| c.get())
            });
        let flat = RenderCfg {
            terrain: None,
            ..cfg.clone()
        };
        let flat_elapsed = fastest(&flat, &mut Vec::new());
        println!(
            "{name}: {:.0} ms ({:.0} ms without relief), {:.1} steps, {:.2} look-ups per pixel",
            elapsed.as_secs_f64() * 1e3,
            flat_elapsed.as_secs_f64() * 1e3,
            steps as f64 / (width * height) as f64,
            lookups as f64 / (width * height) as f64
        );
        image::RgbaImage::from_raw(width as u32, height as u32, pixels)
            .unwrap()
            .save(dir.join(format!("{name}.png")))
            .unwrap();
    }
}

#[test]
fn a_fixed_level_shows_one_tile_tier_across_the_picture() {
    // Even levels red, odd levels blue.
    let (red, blue) = (solid_tile([255, 0, 0]), solid_tile([0, 0, 255]));
    let lookup = |key: TileKey| {
        if unkey(key).1.is_multiple_of(2) {
            Lookup::Ready(red.clone())
        } else {
            Lookup::Ready(blue.clone())
        }
    };
    let (width, height) = (120, 80);
    let mut cfg = batch_reference_config(Provider::Satellite);
    cfg.atmo = false;
    cfg.sky = None;
    cfg.sun = None;
    cfg.level_blend = false;
    let tiers = |pixels: &[u8]| {
        let reds = pixels
            .chunks(RGBA_CHANNELS)
            .filter(|p| p[0] > 200 && p[2] < 50)
            .count();
        let blues = pixels
            .chunks(RGBA_CHANNELS)
            .filter(|p| p[2] > 200 && p[0] < 50)
            .count();
        (reds, blues)
    };
    // An altitude where the curved globe spans two levels from centre to edge.
    let basis = [300.0, 600.0, 900.0, 1500.0, 2500.0, 4000.0]
        .into_iter()
        .map(|alt| {
            Cam {
                lat: 54.0,
                lon: 8.0,
                heading: 0.0,
                alt,
                tilt: 0.0,
            }
            .basis(width as f64 / height as f64)
        })
        .find(|basis| {
            let (pixels, _, _) = render_pixels_tracked(width, height, basis, &cfg, &lookup);
            let (reds, blues) = tiers(&pixels);
            reds > 0 && blues > 0
        })
        .expect("a view spanning two tiers");
    cfg.fixed_level = dominant_level(width, height, &basis, &cfg);
    let (fixed, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    let (reds, blues) = tiers(&fixed);
    assert!(
        reds == 0 || blues == 0,
        "{reds} red and {blues} blue pixels"
    );
}

#[test]
fn night_lights_glow_on_the_night_side_and_replace_the_day_without_sun() {
    let (ground, lights) = (solid_tile([90, 110, 70]), solid_tile([250, 220, 120]));
    let lookup = |key: TileKey| {
        if unkey(key).0 == Provider::NightLights {
            Lookup::Ready(lights.clone())
        } else {
            Lookup::Ready(ground.clone())
        }
    };
    let (width, height) = (64, 48);
    let cam = Cam {
        lat: 48.0,
        lon: 11.0,
        heading: 0.0,
        alt: 400.0,
        tilt: 0.0,
    };
    let basis = cam.basis(width as f64 / height as f64);
    let mut cfg = batch_reference_config(Provider::Satellite);
    cfg.atmo = false;
    cfg.sky = None;
    // The sun on the far side: the whole view is night.
    cfg.sun = Some(-ll_to_v3(cam.lat, cam.lon));
    let brightness = |pixels: &[u8]| pixels.iter().map(|&c| u64::from(c)).sum::<u64>();
    let (dark, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    cfg.night_lights = true;
    let (lit, missing, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    assert!(missing.is_empty());
    assert!(brightness(&lit) > brightness(&dark) * 2);
    // By day the lights add nothing.
    cfg.sun = Some(ll_to_v3(cam.lat, cam.lon));
    let (day_lit, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    cfg.night_lights = false;
    let (day, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    assert_eq!(day_lit, day);
    // Without the sun the lights are the surface.
    cfg.sun = None;
    cfg.night_lights = true;
    let (plain, _, _) = render_pixels_tracked(width, height, &basis, &cfg, &lookup);
    let centre = (height / 2 * width + width / 2) * RGBA_CHANNELS;
    assert!(plain[centre].abs_diff(250) <= 2 && plain[centre + 2].abs_diff(120) <= 2);
}
