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
