//! Frame-time and repaint measurements; run alone with --release.
use super::*;

/// Median CPU time of a whole interface frame (layout, widgets, overlays; no GPU work),
/// stationary after startup. Manual, like `interface_screenshots`.
#[test]
#[ignore = "manual: interface frame-time benchmark; run alone with --release --ignored --nocapture"]
fn interface_frame_time() {
    let mut settings = globecraft_data::settings::Settings::default();
    settings.appearance.theme = globecraft_data::settings::appearance::ThemePreference::Dark;
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1480.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            super::App::new(
                cc,
                settings,
                std::sync::Arc::new(super::RestartRequest::default()),
            )
        });
    // Let fonts, boundaries and the first frames settle.
    for _ in 0..120 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let measure = |h: &mut egui_kittest::Harness<'static, super::App>, what: &str| {
        let mut rounds = Vec::new();
        for _ in 0..7 {
            let started = thread_cpu_ns();
            for _ in 0..100 {
                h.step();
            }
            rounds.push((thread_cpu_ns() - started) as f64 / 1e6 / 100.0);
        }
        rounds.sort_by(f64::total_cmp);
        println!("interface frame ({what}): median {:.3} ms", rounds[3]);
    };
    if std::env::var("GLOBECRAFT_UI_SHAPES").is_ok() {
        let shapes = &h.output().shapes;
        let mut kinds = std::collections::BTreeMap::<String, usize>::new();
        for clipped in shapes {
            let name = format!("{:?}", clipped.shape);
            let kind = name.split(['(', ' ', '{']).next().unwrap_or("").to_owned();
            *kinds.entry(kind).or_default() += 1;
        }
        println!("shapes: {} {kinds:?}", shapes.len());
    }
    measure(&mut h, "default");
    if std::env::var("GLOBECRAFT_UI_PARTS").is_ok() {
        h.state_mut().settings.workspace.ribbon_minimized = true;
        measure(&mut h, "ribbon collapsed");
        h.state_mut().settings.workspace.right_pane = None;
        measure(&mut h, "ribbon collapsed, no side pane");
    }
}

/// A fast scroll zoom over the globe (in and back out), as a user flicks the wheel:
/// reports the slowest UI passes with the altitude they happened at. Offline, so tiles
/// do not arrive; it shows the interface's own cost during a fast zoom.
#[test]
#[ignore = "manual: fast-zoom UI passes; run alone with --release --ignored --nocapture"]
fn fast_zoom_ui_passes() {
    let mut settings = globecraft_data::settings::Settings::default();
    settings.appearance.theme = globecraft_data::settings::appearance::ThemePreference::Dark;
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1480.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            super::App::new(
                cc,
                settings,
                std::sync::Arc::new(super::RestartRequest::default()),
            )
        });
    for _ in 0..120 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let centre = eframe::egui::pos2(900.0, 500.0);
    let wheel = |notches: f32| eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Line,
        delta: eframe::egui::vec2(0.0, notches),
        phase: eframe::egui::TouchPhase::Move,
        modifiers: eframe::egui::Modifiers::NONE,
    };
    let mut passes = Vec::new();
    for (steps, notches) in [(90, 2.0_f32), (90, -2.0)] {
        for _ in 0..steps {
            h.event(eframe::egui::Event::PointerMoved(centre));
            h.event(wheel(notches));
            let (wall, cpu) = (std::time::Instant::now(), thread_cpu_ns());
            h.step();
            let ms = wall.elapsed().as_secs_f64() * 1000.0;
            let cpu_ms = (thread_cpu_ns() - cpu) as f64 / 1e6;
            passes.push((ms, cpu_ms, h.state().cam.alt));
            // About 60 frames a second, with the workers running alongside.
            std::thread::sleep(std::time::Duration::from_millis(12));
        }
    }
    let mut by_time = passes.clone();
    by_time.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut walls: Vec<f64> = passes.iter().map(|p| p.0).collect();
    walls.sort_by(f64::total_cmp);
    println!(
        "fast zoom: {} UI passes, median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        walls.len(),
        walls[walls.len() / 2],
        walls[walls.len() * 95 / 100],
        walls[walls.len() - 1]
    );
    for (ms, cpu_ms, alt) in by_time.iter().take(12) {
        println!("  {ms:7.2} ms wall, {cpu_ms:7.2} ms UI-thread CPU at {alt:9.1} km");
    }
}

/// Frame time over the tilted relief view: 3D terrain on, borders, roads, railways,
/// peaks and waters shown, camera low over the Alps tilted towards the horizon, so the
/// line-of-sight test runs for every overlay point. Waits for tiles first (needs
/// network), then reports the median and p95 of whole UI passes. Manual.
#[test]
#[ignore = "manual: needs network; run alone with --release --ignored --nocapture"]
fn tilted_relief_frame_time() {
    use globecraft_core::layers::MapLayer;
    let mut settings = globecraft_data::settings::Settings::default();
    for layer in [
        MapLayer::Terrain,
        MapLayer::CountryBorders,
        MapLayer::StateBorders,
        MapLayer::Roads,
        MapLayer::Railways,
        MapLayer::Peaks,
        MapLayer::Waters,
    ] {
        settings.set_layer_visible(layer, true);
    }
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1480.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            super::App::new(
                cc,
                settings,
                std::sync::Arc::new(super::RestartRequest::default()),
            )
        });
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 46.55, "lon": 8.0, "altitude_km": 12.0, "heading_deg": 150.0}),
    );
    let _ = h.state_mut().run("camera.set", json!({"tilt_deg": 75.0}));
    // Let elevation, imagery and the overlay tiles arrive.
    let settle_for = std::time::Duration::from_secs(
        std::env::var("GLOBECRAFT_RELIEF_SETTLE_S")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(40),
    );
    let started = std::time::Instant::now();
    while started.elapsed() < settle_for {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let cam = h.state().cam;
    println!(
        "tilted relief view: tilt {:.1}°, altitude {:.1} km",
        cam.tilt, cam.alt
    );
    let mut walls = Vec::new();
    let mut cpus = Vec::new();
    for frame in 0..600 {
        // A slow pan, so the overlays are re-prepared as the user would see them.
        h.state_mut().cam.heading = 150.0 + f64::from(frame) * 0.05;
        let (wall, cpu) = (std::time::Instant::now(), thread_cpu_ns());
        h.step();
        walls.push(wall.elapsed().as_secs_f64() * 1000.0);
        cpus.push((thread_cpu_ns() - cpu) as f64 / 1e6);
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    for (what, values) in [("wall", &mut walls), ("UI-thread CPU", &mut cpus)] {
        values.sort_by(f64::total_cmp);
        println!(
            "tilted relief frame ({what}): median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            values[values.len() / 2],
            values[values.len() * 95 / 100],
            values[values.len() - 1]
        );
    }
}

/// Descends from orbit to municipal altitude over a fixed path and reports the
/// slowest UI passes and when each boundary layer arrived. Needs network access to
/// the boundary services; manual, like the other interface checks.
#[test]
#[ignore = "manual: needs network; run with --release --ignored --nocapture"]
fn boundary_streaming_descent() {
    let mut settings = globecraft_data::settings::Settings::default();
    settings.set_layer_visible(globecraft_core::layers::MapLayer::MunicipalBorders, true);
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1480.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            super::App::new(
                cc,
                settings,
                std::sync::Arc::new(super::RestartRequest::default()),
            )
        });
    let lat = std::env::var("GLOBECRAFT_DESCENT_LAT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48.5);
    let lon = std::env::var("GLOBECRAFT_DESCENT_LON")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9.0);
    let start = std::time::Instant::now();
    let mut slowest: Vec<(f64, f64)> = Vec::new();
    let mut first_states = None;
    let mut first_municipal = None;
    let total = std::time::Duration::from_secs(40);
    while start.elapsed() < total {
        let progress = start.elapsed().as_secs_f64() / total.as_secs_f64();
        // Log-linear descent from 9,000 km to 150 km.
        let altitude = 9_000.0 * (150.0_f64 / 9_000.0).powf(progress.min(1.0));
        {
            let app = h.state_mut();
            app.cam.lat = lat;
            app.cam.lon = lon;
            app.cam.alt = altitude;
        }
        let t = std::time::Instant::now();
        h.step();
        slowest.push((t.elapsed().as_secs_f64() * 1000.0, altitude));
        let data = &h.state().borders.shown;
        if first_states.is_none() && !data.osm_borders.state.is_empty() {
            first_states = Some(altitude);
        }
        if first_municipal.is_none() && !data.osm_borders.municipal.is_empty() {
            first_municipal = Some(altitude);
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    slowest.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "frames {}, slowest UI passes (ms @ km): {:?}",
        slowest.len(),
        &slowest[..8.min(slowest.len())]
    );
    let mut sorted: Vec<f64> = slowest.iter().map(|s| s.0).collect();
    sorted.sort_by(f64::total_cmp);
    println!(
        "median {:.2} ms, p99 {:.2} ms",
        sorted[sorted.len() / 2],
        sorted[sorted.len() * 99 / 100]
    );
    println!("states arrived at {first_states:?} km, municipal at {first_municipal:?} km");
    for line in h.state().borders.log.lock().unwrap().iter() {
        println!("  {line}");
    }
}

/// A Pro Max screenshot rendering in the background while the user zooms a tilted view:
/// the globe on screen must keep following the camera. Stills used to render on the
/// view's thread pool and froze it until they finished (2026-10-25).
#[test]
#[ignore = "manual: starts the application; run alone with --release --ignored --nocapture"]
fn a_screenshot_in_progress_does_not_freeze_the_view() {
    let settings = globecraft_data::settings::Settings {
        screenshot_dir: std::env::temp_dir()
            .join(format!("globecraft-shot-{}", std::process::id()))
            .display()
            .to_string(),
        ..Default::default()
    };
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1480.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            super::App::new(
                cc,
                settings,
                std::sync::Arc::new(super::RestartRequest::default()),
            )
        });
    let pass = |h: &mut egui_kittest::Harness<'static, App>| {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(15));
    };
    for _ in 0..120 {
        pass(&mut h);
    }
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 46.5, "lon": 8.0, "altitude_km": 900.0, "heading_deg": 20.0, "tilt_deg": 55.0}),
    );
    for _ in 0..60 {
        pass(&mut h);
    }
    h.state_mut()
        .run("file.screenshot", json!({"quality": "pro_max"}))
        .expect("screenshot starts");
    let wheel = |notches: f32| eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Line,
        delta: eframe::egui::vec2(0.0, notches),
        phase: eframe::egui::TouchPhase::Move,
        modifiers: eframe::egui::Modifiers::NONE,
    };
    // The longest run of passes in which the camera moved but the picture did not.
    let (mut run, mut longest) = (0, 0);
    let mut shown_before = None;
    for i in 0..240 {
        h.event(eframe::egui::Event::PointerMoved(eframe::egui::pos2(
            740.0, 450.0,
        )));
        h.event(wheel(if (i / 60) % 2 == 0 { 1.0 } else { -1.0 }));
        pass(&mut h);
        let shown = h
            .state()
            .frame
            .renderer
            .displayed_view()
            .map(|(cam, _)| cam.alt);
        run = if shown == shown_before { run + 1 } else { 0 };
        longest = longest.max(run);
        shown_before = shown;
    }
    let _ = h.state_mut().run("file.cancelScreenshot", Value::Null);
    println!(
        "longest frozen run: {longest} passes; tilt {:.1}",
        h.state().cam.tilt
    );
    assert!(longest < 30, "the view froze for {longest} passes");
    assert_eq!(h.state().cam.tilt, 55.0, "zooming in keeps the tilt");
}

/// Idle cost (roadmap 14.6): once the default view has loaded, the window asks for
/// no more repaints (the earlier 4 Hz gamepad poll kept it awake). Manual: needs the
/// network to settle.
#[test]
#[ignore = "manual: idle repaint check; needs the network"]
fn idle_window_requests_no_repaints() {
    let size = egui::vec2(1480.0, 900.0);
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .renderer(WgpuTestRenderer::new())
        .build_eframe(|cc| App::new(cc, Settings::default(), Arc::new(RestartRequest::default())));
    let delay = |h: &Harness<'static, App>| {
        h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay
    };
    // Settle: the first frame, tiles, border tiles and the overlay worker.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut quiet = 0;
    while quiet < 40 && std::time::Instant::now() < until {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(50));
        quiet = if busy(h.state()) || delay(&h) < std::time::Duration::from_secs(5) {
            0
        } else {
            quiet + 1
        };
    }
    h.step();
    h.step();
    let causes = h.ctx.repaint_causes();
    println!("repaint delay {:?}, causes {causes:?}", delay(&h));
    assert!(
        delay(&h) >= std::time::Duration::from_secs(5),
        "an idle window still repaints soon: {:?} {causes:?}",
        delay(&h)
    );
}
