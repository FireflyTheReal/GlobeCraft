//! Offscreen screenshots of the whole window for reviewing the interface. Manual: it starts the
//! real application (workers, network) against the user cache, so run it with a private cache:
//!
//! ```sh
//! XDG_CACHE_HOME=$(mktemp -d) GLOBECRAFT_SHOTS=target/screenshots \
//!   cargo test -p globecraft interface_screenshots -- --ignored --nocapture
//! ```
use super::{App, RestartRequest};
use eframe::egui;
use egui_kittest::{Harness, wgpu::WgpuTestRenderer};
use globecraft_core::commands::Tab;
use globecraft_data::settings::{Settings, appearance::ThemePreference, workspace::RightPane};
use serde_json::{Value, json};
use std::sync::Arc;

const SIZE: egui::Vec2 = egui::vec2(1480.0, 900.0);

fn harness(theme: ThemePreference) -> Harness<'static, App> {
    let mut settings = Settings::default();
    settings.appearance.theme = theme;
    Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(1.5)
        .with_step_dt(0.1)
        .renderer(WgpuTestRenderer::new())
        .build_eframe(move |cc| App::new(cc, settings, Arc::new(RestartRequest::default())))
}

fn settle(h: &mut Harness<'static, App>, seconds: f32) {
    let steps = (seconds / 0.1) as usize;
    for _ in 0..steps {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn save(h: &mut Harness<'static, App>, dir: &std::path::Path, name: &str) {
    let image = h.render().expect("render");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("wrote {}", path.display());
}

#[test]
#[ignore = "manual: renders the full interface offscreen; see the module documentation"]
fn interface_screenshots() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");

    let mut h = harness(ThemePreference::Light);
    settle(&mut h, 6.0);
    save(&mut h, &dir, "01-home-light");

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Layers;
    let _ = app.run("ui.pane.layers", Value::Null);
    let _ = app.run("layer.show", json!({"layer": "grid"}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "02-layers-light");

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Live;
    app.settings.workspace.open_right(RightPane::Measure);
    let _ = app.run("ruler.addPoint", json!({"lat": 38.72, "lon": -9.14}));
    let _ = app.run("ruler.addPoint", json!({"lat": -22.9, "lon": -43.2}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "03-live-measure-light");

    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.settings.workspace.open_right(RightPane::Assistant);
    let _ = app.run("ui.pane.places", Value::Null);
    let _ = app.run(
        "places.add",
        json!({"lat": 27.988, "lon": 86.925, "name": "Mount Everest"}),
    );
    settle(&mut h, 6.0);
    save(&mut h, &dir, "04-assistant-dark");

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::View;
    let _ = app.run("ui.commandSearch", Value::Null);
    settle(&mut h, 1.0);
    save(&mut h, &dir, "05-command-search-dark");
}

/// Answers OpenAI-compatible chat requests in order with `replies`, one connection each.
fn fake_model(replies: Vec<serde_json::Value>) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            // Read the headers, then the body by its length.
            loop {
                let n = socket.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body = reply.to_string();
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

#[test]
#[ignore = "manual: drives the real application against a local fake model; see the module documentation"]
fn assistant_runs_commands_end_to_end() {
    let port = fake_model(vec![
        json!({"choices": [{"finish_reason": "tool_calls", "message": {"role": "assistant",
        "content": "Flying to Paris and turning on the grid.",
        "tool_calls": [
            {"id": "a", "type": "function", "function": {"name": "run_command",
                "arguments": "{\"command\":\"camera.flyTo\",\"params\":{\"lat\":48.8566,\"lon\":2.3522,\"altitude_km\":15}}"}},
            {"id": "b", "type": "function", "function": {"name": "run_command",
                "arguments": "{\"command\":\"layer.show\",\"params\":{\"layer\":\"grid\",\"visible\":true}}"}}
        ]}}]}),
        json!({"choices": [{"finish_reason": "stop", "message": {"role": "assistant",
            "content": "You're over **Paris** at 15 km, with the latitude/longitude grid on."}}]}),
    ]);
    let mut h = harness(ThemePreference::Light);
    let app = h.state_mut();
    app.settings.workspace.open_right(RightPane::Assistant);
    app.ensure_agent();
    let mut config = app.assistant_config("custom");
    config.base_url = format!("http://127.0.0.1:{port}/v1");
    config.model = "fake".into();
    app.apply_assistant_config(config);
    app.assistant_send("Show me Paris with a grid".into());
    for _ in 0..80 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !h.state().assistant.busy() {
            break;
        }
    }
    let app = h.state();
    assert!(!app.assistant.busy(), "the conversation finished");
    assert!(
        app.layer(globecraft_core::layers::MapLayer::Grid),
        "grid shown by the assistant"
    );
    let transcript = &app.assistant.agent.as_ref().expect("agent").transcript;
    assert!(
        transcript
            .iter()
            .any(|e| matches!(e, globecraft_assistant::Entry::Assistant(t) if t.contains("Paris"))),
        "{transcript:?}"
    );
    settle(&mut h, 5.0);
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    save(&mut h, &dir, "06-assistant-conversation");
    let cam = h.state().cam;
    assert!(
        (cam.lat - 48.8566).abs() < 0.5 && (cam.lon - 2.3522).abs() < 0.5,
        "{cam:?}"
    );
}

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
        h.state_mut().settings.sidebar.open = false;
        measure(&mut h, "ribbon collapsed, no left pane");
    }
}

/// CPU time of the calling thread in nanoseconds (Linux scheduler statistics), so background
/// loaders competing for cores do not count; elsewhere, wall time since the first call.
fn thread_cpu_ns() -> u64 {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or_else(|| {
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            START
                .get_or_init(std::time::Instant::now)
                .elapsed()
                .as_nanos() as u64
        })
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
        let data = &h.state().boundaries;
        if first_states.is_none() && !data.states.is_empty() {
            first_states = Some(altitude);
        }
        if first_municipal.is_none() && !data.world_cities.is_empty() {
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
    for line in h.state().boundary_log.lock().unwrap().iter() {
        println!("  {line}");
    }
}
