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
    // GLOBECRAFT_SHOT_SIZE ("1190x775") and _PPP reproduce a user's window.
    let size = std::env::var("GLOBECRAFT_SHOT_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some(egui::Vec2::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(SIZE);
    let ppp = std::env::var("GLOBECRAFT_SHOT_PPP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.5);
    Harness::builder()
        .with_size(size)
        .with_pixels_per_point(ppp)
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

/// Drags the navigator's tilt thumb from straight down to about 60° and checks the
/// camera followed (the default window and controls scale place the ring here).
fn drag_the_tilt_thumb(h: &mut Harness<'static, App>) {
    let ring = egui::pos2(1423.3, 202.0);
    let at = |degrees: f32| {
        let angle = (32.0 + degrees).to_radians();
        ring + egui::vec2(-angle.sin(), -angle.cos()) * 50.0
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(at(0.0)));
    h.step();
    h.event(button(at(0.0), true));
    h.step();
    for degrees in [10.0, 25.0, 40.0, 55.0, 60.0] {
        h.event(egui::Event::PointerMoved(at(degrees)));
        h.step();
    }
    h.event(button(at(60.0), false));
    settle(h, 2.0);
    let tilt = h.state().cam.tilt;
    assert!((tilt - 60.0).abs() < 3.0, "tilt after the drag: {tilt}");
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
    drag_the_tilt_thumb(&mut h);
    save(&mut h, &dir, "01-tilt-arc-light");
    let _ = h.state_mut().run("camera.topDown", Value::Null);
    let _ = h
        .state_mut()
        .run("ui.navigatorStyle", json!({"style": "classic"}));
    settle(&mut h, 0.5);
    save(&mut h, &dir, "01-classic-navigator-light");
    let _ = h
        .state_mut()
        .run("ui.navigatorStyle", json!({"style": "globe"}));

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Layers;
    let _ = app.run("ui.pane.layers", Value::Null);
    let _ = app.run("layer.show", json!({"layer": "grid"}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "02-layers-light");

    // The search narrows the list to matching rows and keeps their groups open.
    let app = h.state_mut();
    app.set_layers_search("parks", false);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "02-layers-search-light");
    let app = h.state_mut();
    app.set_layers_search("", true);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "02-layers-active-light");
    let app = h.state_mut();
    app.set_layers_search("no such layer", false);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "02-layers-search-empty-light");
    let app = h.state_mut();
    app.set_layers_search("", false);

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

    // A tilted view toward the horizon, then a steeper one closer in.
    h.key_press(egui::Key::Escape);
    settle(&mut h, 0.3);
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 46.5, "lon": 8.0, "altitude_km": 900.0, "heading_deg": 20.0, "tilt_deg": 55.0}),
    );
    settle(&mut h, 2.0);
    save(&mut h, &dir, "06-tilted-dark");
    let _ = h
        .state_mut()
        .run("camera.set", json!({"altitude_km": 60.0, "tilt_deg": 75.0}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "07-tilted-close-dark");
    let _ = h.state_mut().run("ui.tileDiagnostics", json!({"on": true}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "08-tile-diagnostics-dark");
}

/// The navigator's view footprint across altitudes and tilts (user, 2026-11-08: the
/// tilt representation was buggy): one screenshot per case, `nav-<alt>-<tilt>`.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn navigator_tilt_sweep() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    settle(&mut h, 1.0);
    for alt in [20_000.0, 6_000.0, 900.0, 60.0, 2.0] {
        for tilt in [
            0.0,
            1.0,
            20.0,
            45.0,
            65.0,
            globecraft_core::camera::MAX_TILT_DEG,
        ] {
            let _ = h.state_mut().run(
                "camera.set",
                json!({"lat": 30.0, "lon": 105.0, "altitude_km": alt, "heading_deg": 15.0, "tilt_deg": tilt}),
            );
            settle(&mut h, 0.2);
            save(&mut h, &dir, &format!("nav-{alt}-{tilt}"));
        }
    }
}

/// The Settings window's outer rectangle: the widest node labelled with its title,
/// because the ribbon also has a "Settings" command with the same label.
fn settings_window_rect(h: &Harness<'static, App>) -> egui::Rect {
    use egui_kittest::kittest::Queryable;
    h.get_all_by_label("Settings")
        .map(|node| node.rect())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .expect("settings window")
}

fn scroll_settings(h: &mut Harness<'static, App>, at: egui::Pos2, down: bool, steps: usize) {
    h.event(egui::Event::PointerMoved(at));
    for _ in 0..steps {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, if down { -400.0 } else { 400.0 }),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    settle(h, 0.3);
}

/// Settings → Network at the window's minimum size, in both languages, with a
/// remote user layer and an assistant provider so the dynamic groups are shown.
/// Manual because it renders offscreen with the project's screenshot renderer.
#[test]
#[ignore = "manual: renders the Settings Network page offscreen at minimum size"]
fn network_settings_screenshots() {
    use egui_kittest::kittest::Queryable;
    use globecraft_data::user_data::{FeatureFormat, UserLayer, UserSource};

    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut settings = Settings::default();
    settings.appearance.theme = ThemePreference::Light;
    settings.user_layers.push(UserLayer {
        id: 9_999,
        name: "City feed".into(),
        source: UserSource::Features {
            location: "https://data.example.org/cities.geojson".into(),
            format: FeatureFormat::GeoJson,
            refresh_minutes: 15,
            value_field: None,
        },
        visible: true,
        opacity: 1.0,
        attribution: "Example data".into(),
        color: "#ffb000".into(),
        preset: None,
    });
    settings.assistant.provider = "ollama".into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 760.0))
        .with_pixels_per_point(1.5)
        .with_step_dt(0.1)
        .renderer(WgpuTestRenderer::new())
        .build_eframe(move |cc| App::new(cc, settings, Arc::new(RestartRequest::default())));
    settle(&mut h, 2.0);
    let _ = h.state_mut().run("ui.settings", Value::Null);
    settle(&mut h, 0.5);

    // Drag the bottom-right corner to the Settings window's 520×360 point minimum.
    let before = settings_window_rect(&h);
    let start = before.max - egui::vec2(4.0, 4.0);
    let delta = egui::vec2(520.0, 360.0) - before.size();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(start));
    h.step();
    h.event(button(start, true));
    h.step();
    for step in 1..=5 {
        h.event(egui::Event::PointerMoved(
            start + delta * (step as f32 / 5.0),
        ));
        h.step();
    }
    h.event(button(start + delta, false));
    settle(&mut h, 0.5);
    let rect = settings_window_rect(&h);
    println!("settings window after resize: {rect:?}");
    assert!(
        (rect.width() - 520.0).abs() < 3.0 && (rect.height() - 360.0).abs() < 4.0,
        "window did not reach its minimum: {rect:?}"
    );

    h.get_by_label("Network").click();
    settle(&mut h, 0.5);
    assert!(
        h.query_by_label_contains("server.arcgisonline.com")
            .is_some(),
        "no basemap host row"
    );
    save(&mut h, &dir, "network-en-top");
    scroll_settings(&mut h, rect.center(), true, 4);
    save(&mut h, &dir, "network-en-middle");
    scroll_settings(&mut h, rect.center(), true, 6);
    save(&mut h, &dir, "network-en-bottom");
    scroll_settings(&mut h, rect.center(), false, 10);
    // The Keys page (adapter D): one field per keyed service.
    h.get_by_label("Keys").click();
    settle(&mut h, 0.5);
    assert!(
        h.query_by_label_contains("NASA FIRMS").is_some(),
        "no FIRMS key row"
    );
    save(&mut h, &dir, "keys-en");
    h.get_by_label("Network").click();
    settle(&mut h, 0.5);

    let _ = h
        .state_mut()
        .run("settings.language", json!({"language": "pt-BR"}));
    settle(&mut h, 0.5);
    save(&mut h, &dir, "network-pt-top");
    scroll_settings(&mut h, rect.center(), true, 4);
    save(&mut h, &dir, "network-pt-middle");
    scroll_settings(&mut h, rect.center(), true, 6);
    save(&mut h, &dir, "network-pt-bottom");
    assert!(
        h.query_by_label_contains("data.example.org").is_some(),
        "no user-layer host row"
    );
    assert!(
        h.query_by_label_contains("localhost:11434").is_some(),
        "no assistant host row"
    );
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
        ]}}], "usage": {"prompt_tokens": 1500, "completion_tokens": 60}}),
        json!({"choices": [{"finish_reason": "stop", "message": {"role": "assistant",
            "content": "You're over **Paris** at 15 km, with the latitude/longitude grid on."}}],
            "usage": {"prompt_tokens": 1700, "completion_tokens": 25}}),
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
    // The turn ends with its tokens (no price: the fake model's family is unknown).
    assert!(
        matches!(transcript.last(), Some(globecraft_assistant::Entry::Usage(u))
            if u.usage.all_input_tokens() == 3200 && u.cost_usd.is_none()),
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

/// Roads with their names, railways and place names close up over Munich, for
/// checking the map-data layers by eye. Needs network; manual.
#[test]
#[ignore = "manual: needs network; renders the map-data layers close up"]
fn map_data_close_up_screenshot() {
    use globecraft_core::layers::MapLayer;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    let env_f64 = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f64>().ok());
    let layers = std::env::var("GLOBECRAFT_SHOT_LAYERS")
        .unwrap_or_else(|_| "roads,railways,parks,waters".into());
    for code in layers.split(',') {
        if let Some(layer) = MapLayer::from_code(code.trim()) {
            h.state_mut().settings.set_layer_visible(layer, true);
        }
    }
    let altitude = std::env::var("GLOBECRAFT_SHOT_ALT_KM")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(3.0);
    let _ = h.state_mut().run(
        "camera.set",
        json!({
            "lat": env_f64("GLOBECRAFT_SHOT_LAT").unwrap_or(48.137),
            "lon": env_f64("GLOBECRAFT_SHOT_LON").unwrap_or(11.575),
            "altitude_km": altitude,
            "heading_deg": env_f64("GLOBECRAFT_SHOT_HEADING").unwrap_or(0.0),
        }),
    );
    let mut waited = 0.0;
    for seconds in [2.0, 4.0, 8.0, 30.0] {
        settle(&mut h, seconds - waited);
        waited = seconds;
        save(
            &mut h,
            &dir,
            &format!("07-map-data-{altitude}km-{seconds}s"),
        );
    }
}

/// Pans across northern Germany at 120 km with roads on while their tiles load,
/// saving a frame every half second: for catching drawing faults in motion. Manual.
#[test]
#[ignore = "manual: needs network; frames of a pan with roads"]
fn roads_pan_frames() {
    use globecraft_core::layers::MapLayer;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    h.state_mut()
        .settings
        .set_layer_visible(MapLayer::Roads, true);
    // GLOBECRAFT_SHOT_LAT / _LON / _ALT_KM / _STEP_DEG move the pan elsewhere.
    let env_f64 = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f64>().ok());
    let lat = env_f64("GLOBECRAFT_SHOT_LAT").unwrap_or(53.3);
    let start = env_f64("GLOBECRAFT_SHOT_LON").unwrap_or(6.0);
    let altitude_km = env_f64("GLOBECRAFT_SHOT_ALT_KM").unwrap_or(120.0);
    let step_deg = env_f64("GLOBECRAFT_SHOT_STEP_DEG").unwrap_or(0.08);
    for step in 0..40 {
        let lon = start + f64::from(step) * step_deg;
        let _ = h.state_mut().run(
            "camera.set",
            json!({"lat": lat, "lon": lon, "altitude_km": altitude_km}),
        );
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if step % 5 == 4 {
            save(&mut h, &dir, &format!("11-pan-{step:02}"));
        }
    }
}

/// The Add Data window's library and a GeoJSON layer of a few cities with a route
/// between them, in the Layers pane and on the globe. Manual.
#[test]
#[ignore = "manual: renders the Add Data window and a user layer"]
fn add_data_screenshots() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let file = std::env::temp_dir().join("globecraft-add-data-sample.geojson");
    std::fs::write(
        &file,
        r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"name":"Lisbon"},"geometry":{"type":"Point","coordinates":[-9.14,38.72]}},
          {"type":"Feature","properties":{"name":"Madrid"},"geometry":{"type":"Point","coordinates":[-3.70,40.42]}},
          {"type":"Feature","properties":{"name":"Paris"},"geometry":{"type":"Point","coordinates":[2.35,48.86]}},
          {"type":"Feature","properties":{"name":"Route"},"geometry":{"type":"LineString","coordinates":[[-9.14,38.72],[-3.70,40.42],[2.35,48.86]]}}
        ]}"#,
    )
    .expect("sample file");
    let mut h = harness(ThemePreference::Dark);
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 43.5, "lon": -3.0, "altitude_km": 2500.0}),
    );
    let app = h.state_mut();
    app.add_user_layer(globecraft_data::user_data::UserLayer {
        id: 0,
        name: "Sample cities".into(),
        source: globecraft_data::user_data::UserSource::Features {
            location: file.to_string_lossy().into_owned(),
            format: globecraft_data::user_data::FeatureFormat::GeoJson,
            refresh_minutes: 0,
            value_field: None,
        },
        visible: true,
        opacity: 1.0,
        attribution: "Sample data".into(),
        color: "#ffb000".into(),
        preset: None,
    });
    let _ = app.run("ui.pane.layers", Value::Null);
    let _ = app.run("ui.addData", Value::Null);
    settle(&mut h, 4.0);
    save(&mut h, &dir, "08-add-data-library");
    h.state_mut().add_data = None;
    settle(&mut h, 1.0);
    save(&mut h, &dir, "09-user-layer");
    // The Data pane, with the sample starred and the earthquakes in the history.
    let app = h.state_mut();
    let sample = app.settings.user_layers[0].clone();
    app.settings.saved_sources.toggle_favorite(&sample);
    let _ = app.run("data.add", json!({"preset": "earthquakes"}));
    let _ = app.run("ui.pane.data", Value::Null);
    settle(&mut h, 1.0);
    save(&mut h, &dir, "10-data-pane");
    // The Layers ribbon at both experience levels: Basic leaves out the Earth models.
    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Layers;
    let _ = app.run("settings.experience", json!({"level": "basic"}));
    settle(&mut h, 1.0);
    save(&mut h, &dir, "11-layers-ribbon-basic");
    let _ = h
        .state_mut()
        .run("settings.experience", json!({"level": "advanced"}));
    settle(&mut h, 1.0);
    save(&mut h, &dir, "12-layers-ribbon-advanced");
}

/// Zooms in over Berlin Mitte with places of interest on, as in the user's recording
/// of 2026-11-03, and counts names that were drawn and then vanished or changed side
/// while their point stayed well inside the view. Saves a frame per step. Manual.
#[test]
#[ignore = "manual: needs network; zooms in with places of interest on"]
fn poi_zoom_stability() {
    use globecraft_core::layers::MapLayer;
    use std::collections::HashMap;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    let place_names = std::env::var_os("GLOBECRAFT_SHOT_NO_PLACES").is_none();
    h.state_mut()
        .settings
        .set_layer_visible(MapLayer::PointsOfInterest, true);
    h.state_mut()
        .settings
        .set_layer_visible(MapLayer::PlaceLabels, place_names);
    let at = |h: &mut Harness<'static, App>, altitude_km: f64| {
        let _ = h.state_mut().run(
            "camera.set",
            json!({"lat": 52.5206, "lon": 13.3942, "altitude_km": altitude_km, "heading_deg": 0.0}),
        );
    };
    at(&mut h, 6.0);
    settle(&mut h, 20.0);
    // Each drawn name's side and point, and the inner part of the view, last step.
    let mut previous: HashMap<String, (Option<usize>, egui::Pos2)> = HashMap::new();
    let mut previous_inner = egui::Rect::NOTHING;
    let (mut vanished, mut moved, mut drawn_total) = (0, 0, 0);
    for step in 0..30 {
        at(&mut h, 6.0 * 0.96f64.powi(step + 1));
        settle(&mut h, 0.4);
        let drawn: Vec<_> = h.state().poi_shapes.drawn().to_vec();
        let now: HashMap<String, (Option<usize>, egui::Pos2)> = drawn
            .iter()
            .filter(|(_, side, _)| side.is_some())
            .map(|(key, side, point)| (format!("{key:?}"), (*side, *point)))
            .collect();
        // A name well inside the view a step ago is still in view after zooming in
        // 4 %: it should still be drawn, on the same side.
        for (key, (side, point)) in &previous {
            if !previous_inner.contains(*point) {
                continue;
            }
            match now.get(key) {
                None => {
                    vanished += 1;
                    let reason = match drawn.iter().find(|(k, _, _)| format!("{k:?}") == *key) {
                        Some(_) => "name blocked",
                        None if h
                            .state()
                            .poi_shapes
                            .blocked()
                            .iter()
                            .any(|k| format!("{k:?}") == *key) =>
                        {
                            "marker blocked"
                        }
                        None => "not projected",
                    };
                    println!("vanished at step {step}: {key} ({reason})");
                }
                Some((new, _)) if new != side => moved += 1,
                _ => {}
            }
        }
        // The area the points cover, less a fifth at each side.
        let covered =
            egui::Rect::from_points(&drawn.iter().map(|(_, _, p)| *p).collect::<Vec<_>>());
        previous_inner = covered.shrink2(covered.size() * 0.2);
        drawn_total += now.len();
        previous = now;
        save(&mut h, &dir, &format!("14-poi-zoom-{step:02}"));
    }
    println!(
        "POI zoom: {drawn_total} names drawn over 30 steps, {vanished} vanished, {moved} changed side"
    );
}

/// A ring of 400 quakes along the Pacific rim, magnitude 2.5 to 7.4, one every 36
/// minutes over the ten days before 2026-11-04 09:38 UTC, as a GeoJSON file.
fn quake_sample() -> std::path::PathBuf {
    let ring = [
        (35.0, 140.0),
        (45.0, 150.0),
        (55.0, 160.0),
        (52.0, -170.0),
        (58.0, -150.0),
        (40.0, -125.0),
        (20.0, -105.0),
        (5.0, -82.0),
        (-15.0, -75.0),
        (-33.0, -72.0),
        (-40.0, 175.0),
        (-20.0, 170.0),
        (-5.0, 150.0),
        (10.0, 125.0),
        (25.0, 125.0),
    ];
    let mut features = Vec::new();
    for i in 0..400u32 {
        let (lat, lon) = ring[(i as usize) % ring.len()];
        let jitter = |k: u32| {
            ((i.wrapping_mul(2654435761).wrapping_add(k) % 1000) as f64 / 1000.0 - 0.5) * 6.0
        };
        let mag = 2.5 + (i.wrapping_mul(40503) % 50) as f64 / 10.0;
        features.push(format!(
            r#"{{"type":"Feature","properties":{{"title":"M {mag:.1} - Quake {i}","mag":{mag:.1},"time":{},"depth":{}}},"geometry":{{"type":"Point","coordinates":[{},{}]}}}}"#,
            1_793_785_080_000i64 - i64::from(i) * 86_400_000 / 40,
            10 + i % 600,
            lon + jitter(1),
            lat + jitter(2),
        ));
    }
    let file = std::env::temp_dir().join("globecraft-quakes.geojson");
    std::fs::write(
        &file,
        format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        ),
    )
    .expect("sample file");
    file
}

fn quake_layer(file: &std::path::Path) -> globecraft_data::user_data::UserLayer {
    globecraft_data::user_data::UserLayer {
        id: 0,
        name: "Earthquakes (sample)".into(),
        source: globecraft_data::user_data::UserSource::Features {
            location: file.to_string_lossy().into_owned(),
            format: globecraft_data::user_data::FeatureFormat::GeoJson,
            refresh_minutes: 0,
            value_field: None,
        },
        visible: true,
        opacity: 1.0,
        attribution: "Synthetic sample".into(),
        color: "#ff6a3d".into(),
        preset: None,
    }
}

/// The timeline (roadmap 4.H): the bar, sunlight at another time, and the sample quakes
/// filtered to the day before the app's time. Manual.
#[test]
#[ignore = "manual: renders the timeline and time-filtered points"]
fn timeline_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let file = quake_sample();
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    let _ = app.run(
        "camera.set",
        json!({"lat": 20.0, "lon": -170.0, "altitude_km": 18000.0}),
    );
    app.add_user_layer(quake_layer(&file));
    app.run("ui.timeline", Value::Null).expect("timeline");
    app.run("layer.show", json!({"layer": "sunlight", "visible": true}))
        .expect("sunlight");
    app.run("time.set", json!({"time": "2026-11-04T09:38:00Z"}))
        .expect("time.set");
    settle(&mut h, 4.0);
    save(&mut h, &dir, "18-timeline-all");
    let app = h.state_mut();
    app.run("time.window", json!({"hours": 24}))
        .expect("window");
    settle(&mut h, 1.0);
    save(&mut h, &dir, "19-timeline-last-day");
    // Three days back: the quakes of 4 to 3 days before, and the sun of that hour.
    let app = h.state_mut();
    app.run("time.step", json!({"hours": -72})).expect("step");
    settle(&mut h, 1.0);
    save(&mut h, &dir, "20-timeline-three-days-back");
    // Midsummer noon over the Pacific: the night side moves.
    let app = h.state_mut();
    app.run("time.set", json!({"time": "2026-06-21T23:00:00Z"}))
        .expect("time.set");
    app.run("time.window", json!({"hours": 0})).expect("window");
    settle(&mut h, 1.0);
    save(&mut h, &dir, "21-timeline-june");
}

/// A point layer of 400 synthetic earthquakes around the Pacific: markers sized by
/// magnitude, clusters with their counts, the size legend, and the hover card of the
/// busiest cluster. Manual.
#[test]
#[ignore = "manual: renders a point layer with clusters"]
fn point_layer_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let file = quake_sample();
    let mut h = harness(ThemePreference::Dark);
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 20.0, "lon": -170.0, "altitude_km": 18000.0}),
    );
    let app = h.state_mut();
    app.add_user_layer(quake_layer(&file));
    let _ = app.run("ui.pane.layers", Value::Null);
    settle(&mut h, 4.0);
    save(&mut h, &dir, "15-point-layer");
    if let Some(at) = h.state().user_layers.busiest_cluster() {
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        settle(&mut h, 1.0);
        save(&mut h, &dir, "16-point-layer-hover");
    }
    // Closer: single quakes sized by magnitude, with their names.
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": -33.0, "lon": -72.0, "altitude_km": 900.0}),
    );
    settle(&mut h, 3.0);
    save(&mut h, &dir, "17-point-layer-close");
}

/// A WMS layer's legend in the Layers pane. `GLOBECRAFT_SHOT_WMS` is the service's
/// address (a local server answering any request with a legend image will do).
#[test]
#[ignore = "manual: renders a WMS layer's legend; needs GLOBECRAFT_SHOT_WMS"]
fn wms_legend_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let base = std::env::var("GLOBECRAFT_SHOT_WMS").expect("GLOBECRAFT_SHOT_WMS");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.add_user_layer(globecraft_data::user_data::UserLayer {
        id: 0,
        name: "Water depth".into(),
        source: globecraft_data::user_data::UserSource::Tiles {
            template: globecraft_data::user_data::wms_template(&base, "depth"),
            min_zoom: 0,
            max_zoom: 18,
            tile_px: 256,
        },
        visible: true,
        opacity: 1.0,
        attribution: "Local test service".into(),
        color: "#4aa3ff".into(),
        preset: None,
    });
    let _ = app.run("ui.pane.layers", Value::Null);
    settle(&mut h, 3.0);
    save(&mut h, &dir, "13-wms-legend");
}

/// The "What's here?" card over Munich. Manual (needs network for the ground height).
#[test]
#[ignore = "manual: renders the What's here card"]
fn whats_here_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 48.2, "lon": 11.6, "altitude_km": 400.0}),
    );
    h.state_mut()
        .run("ui.here", json!({"lat": 48.1702, "lon": 11.6290}))
        .expect("card");
    settle(&mut h, 8.0);
    save(&mut h, &dir, "10-whats-here");
    // Close up, where the municipal borders and town names load (roadmap 6.12).
    let _ = h.state_mut().run(
        "layer.show",
        json!({"layer": "municipal_borders", "visible": true}),
    );
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 48.1702, "lon": 11.6290, "altitude_km": 12.0}),
    );
    settle(&mut h, 12.0);
    let here = h
        .state_mut()
        .run("info.here", json!({"lat": 48.1702, "lon": 11.6290}));
    println!("info.here: {here:?}");
    save(&mut h, &dir, "10-whats-here-close");
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
    for line in h.state().boundary_log.lock().unwrap().iter() {
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
        let shown = h.state().renderer.displayed_view().map(|(cam, _)| cam.alt);
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

/// `data.search` finds library sources, and a GeoJSON, GPX or CSV file dropped on
/// the window becomes a layer (roadmap 2.6 and 2.9). No rendering.
#[test]
fn data_search_and_dropped_feature_files() {
    let mut h = Harness::builder()
        .with_size(SIZE)
        .build_eframe(|cc| App::new(cc, Settings::default(), Arc::new(RestartRequest::default())));
    let found = h
        .state_mut()
        .run("data.search", json!({"query": "ocean"}))
        .expect("search");
    let codes: Vec<&str> = found["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["preset"].as_str())
        .collect();
    assert!(
        codes.contains(&"gebco") && !codes.contains(&"earthquakes"),
        "{codes:?}"
    );
    let all = h.state_mut().run("data.search", json!({})).expect("search");
    assert_eq!(
        all["results"].as_array().unwrap().len(),
        globecraft_data::user_data::presets::PRESETS.len()
    );
    let file = std::env::temp_dir().join("globecraft-drop-test.gpx");
    std::fs::write(
        &file,
        "<gpx><wpt lat=\"1\" lon=\"2\"><name>A</name></wpt></gpx>",
    )
    .unwrap();
    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(file.clone())));
    h.step();
    let layers = &h.state().settings.user_layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "globecraft-drop-test");
    assert!(matches!(
        &layers[0].source,
        globecraft_data::user_data::UserSource::Features {
            format: globecraft_data::user_data::FeatureFormat::Gpx,
            ..
        }
    ));
    let _ = std::fs::remove_file(file);
}

/// Interface frames while the time plays, against a still time (roadmap 4.H). Manual.
#[test]
#[ignore = "manual: measures frames while the timeline plays"]
fn timeline_play_frames() {
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.run("layer.show", json!({"layer": "sunlight", "visible": true}))
        .expect("sunlight");
    app.run("ui.timeline", Value::Null).expect("timeline");
    settle(&mut h, 3.0);
    let measure = |h: &mut Harness<'static, App>, label: &str| {
        let mut times = Vec::new();
        for _ in 0..60 {
            let start = std::time::Instant::now();
            h.step();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "TIMELINE {label}: median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            times[30], times[57], times[59]
        );
    };
    measure(&mut h, "still");
    h.state_mut()
        .run("time.play", json!({"hours_per_second": 1}))
        .expect("play");
    measure(&mut h, "playing 1 h/s");
    h.state_mut()
        .run("time.play", json!({"hours_per_second": 24}))
        .expect("play");
    measure(&mut h, "playing 1 day/s");
}

/// The aurora layer (roadmap 5, `HAZ-01`) with a forecast of the usual shape: a ring
/// about 20° around each geomagnetic pole, strongest on its night side. Manual.
#[test]
#[ignore = "manual: renders the aurora oval on the night side"]
fn aurora_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let moment = "2026-11-04T04:00:00Z";
    let unix = globecraft_core::time::parse_iso(moment).unwrap();
    let (sun_lat, sun_lon) = globecraft_core::math::subsolar(unix);
    let sun = globecraft_core::math::ll_to_v3(sun_lat, sun_lon);
    let mut rows = Vec::new();
    for lon in 0..360 {
        for lat in -90..=90 {
            let here = globecraft_core::math::ll_to_v3(f64::from(lat), f64::from(lon));
            let mut chance: f64 = 0.0;
            for pole in [(80.7, -72.7), (-80.7, 107.3)] {
                let pole = globecraft_core::math::ll_to_v3(pole.0, pole.1);
                let distance = here.dot(pole).clamp(-1.0, 1.0).acos().to_degrees();
                // Wider and stronger on the night side, as OVATION's ovals are.
                let night = (0.5 - 0.5 * here.dot(sun)).clamp(0.0, 1.0);
                let width = 3.0 + 3.0 * night;
                chance = chance
                    .max((25.0 + 50.0 * night) * (-((distance - 20.0) / width).powi(2)).exp());
            }
            rows.push(json!([lon, lat, chance.round()]));
        }
    }
    let grid = globecraft_data::aurora::AuroraGrid::parse(&json!({
        "Observation Time": moment,
        "Forecast Time": moment,
        "coordinates": rows,
    }))
    .expect("aurora sample");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.live_aurora.set_grid(grid);
    app.run("layer.show", json!({"layer": "aurora", "visible": true}))
        .expect("aurora");
    app.run("layer.show", json!({"layer": "sunlight", "visible": true}))
        .expect("sunlight");
    app.run("time.set", json!({"time": moment})).expect("time");
    app.run(
        "camera.set",
        json!({"lat": 62.0, "lon": -95.0, "altitude_km": 9000.0}),
    )
    .expect("camera");
    settle(&mut h, 3.0);
    save(&mut h, &dir, "22-aurora-north-america");
    h.state_mut()
        .run(
            "camera.set",
            json!({"lat": 85.0, "lon": 0.0, "altitude_km": 14000.0}),
        )
        .expect("camera");
    settle(&mut h, 2.0);
    save(&mut h, &dir, "23-aurora-over-the-pole");
    // Sunlight off: the whole oval.
    h.state_mut()
        .run("layer.show", json!({"layer": "sunlight", "visible": false}))
        .expect("sunlight");
    settle(&mut h, 2.0);
    save(&mut h, &dir, "24-aurora-without-sunlight");
}

/// An MBTiles map as a layer (roadmap 1.2): a generated checkerboard archive over
/// Europe at zoom 0-5, drawn over the imagery (`archive-layer`). Manual.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn archive_layer_screenshot() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let file = std::env::temp_dir().join("globecraft-archive-sample.mbtiles");
    let _ = std::fs::remove_file(&file);
    let db = rusqlite::Connection::open(&file).expect("mbtiles");
    db.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);",
    )
    .expect("schema");
    for z in 0..=5u32 {
        for x in 0..(1u32 << z) {
            for y in 0..(1u32 << z) {
                let shade = if (x + y + z) % 2 == 0 { 40 } else { 220 };
                let colour = [shade, (z * 40) as u8, 255 - shade, 255];
                let mut png = std::io::Cursor::new(Vec::new());
                image::RgbaImage::from_fn(256, 256, |px, py| {
                    if px < 3 || py < 3 {
                        image::Rgba([255, 255, 255, 255])
                    } else {
                        image::Rgba(colour)
                    }
                })
                .write_to(&mut png, image::ImageFormat::Png)
                .expect("png");
                db.execute(
                    "INSERT INTO tiles VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![z, x, (1u32 << z) - 1 - y, png.into_inner()],
                )
                .expect("tile");
            }
        }
    }
    drop(db);
    let mut h = harness(ThemePreference::Dark);
    h.state_mut()
        .add_user_layer(globecraft_data::user_data::UserLayer {
            id: 0,
            name: "Checkerboard".into(),
            source: globecraft_data::user_data::UserSource::Tiles {
                template: format!("mbtiles:{}", file.display()),
                min_zoom: 0,
                max_zoom: 5,
                tile_px: 256,
            },
            visible: true,
            opacity: 0.6,
            attribution: String::new(),
            color: "#ffb000".into(),
            preset: None,
        });
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 48.0, "lon": 10.0, "altitude_km": 3000.0}),
    );
    settle(&mut h, 3.0);
    save(&mut h, &dir, "archive-layer");
    let _ = std::fs::remove_file(file);
}

/// The sea border and sea area presets over Europe and the North Atlantic
/// (`ocean-presets`). Needs network; manual.
#[test]
#[ignore = "manual: needs network; renders the Marine Regions presets"]
fn ocean_presets_screenshot() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    for preset in ["eez_boundaries", "iho_sea_areas"] {
        h.state_mut()
            .run("data.add", json!({"preset": preset}))
            .expect("preset");
    }
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 50.0, "lon": -10.0, "altitude_km": 4500.0}),
    );
    settle(&mut h, 12.0);
    save(&mut h, &dir, "ocean-presets");
}

/// The About window with the logo, light and dark (`about-<theme>`).
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn about_window() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, name) in [
        (ThemePreference::Light, "about-light"),
        (ThemePreference::Dark, "about-dark"),
    ] {
        let mut h = harness(theme);
        settle(&mut h, 0.5);
        h.state_mut().run("ui.about", json!({})).expect("ui.about");
        settle(&mut h, 0.3);
        save(&mut h, &dir, name);
    }
}

/// Every icon the ribbon, the Layers pane and the timeline show, with drafts beside
/// each (`name~a`, `~b`, `~c` when drawn). `GLOBECRAFT_ICON_NAMES` lists the names,
/// `GLOBECRAFT_ICON_DRAFTS=1` adds the drafts. Manual.
#[test]
#[ignore = "manual: renders an icon sheet"]
fn icon_sheet() {
    use egui_kittest::Harness;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let names: Vec<String> = std::env::var("GLOBECRAFT_ICON_NAMES")
        .expect("GLOBECRAFT_ICON_NAMES")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let drafts = std::env::var("GLOBECRAFT_ICON_DRAFTS").is_ok();
    let variants: Vec<&str> = if drafts {
        vec!["", "~a", "~b", "~c"]
    } else {
        vec![""]
    };
    let per_row = if drafts { 3 } else { 8 };
    let cell_w = 70.0 * variants.len() as f32 + 20.0;
    let cell_h = 112.0;
    let rows = names.len().div_ceil(per_row);
    let size = egui::Vec2::new(cell_w * per_row as f32 + 20.0, cell_h * rows as f32 + 20.0);
    for (theme, dark) in [("dark", true), ("light", false)] {
        let (names, variants) = (names.clone(), variants.clone());
        let mut fonts_set = false;
        let mut h = Harness::builder()
            .with_size(size)
            .with_pixels_per_point(1.5)
            .renderer(WgpuTestRenderer::new())
            .build_ui(move |ui| {
                // The interface's fonts arrive with the next frame; draw from then.
                if !fonts_set {
                    ui.ctx().set_fonts(super::fonts::base_definitions());
                    fonts_set = true;
                    ui.ctx().request_repaint();
                    return;
                }
                let (bg, card, ink, text) = if dark {
                    (
                        egui::Color32::from_rgb(24, 26, 30),
                        egui::Color32::from_rgb(36, 39, 44),
                        egui::Color32::from_rgb(220, 224, 230),
                        egui::Color32::from_rgb(200, 204, 210),
                    )
                } else {
                    (
                        egui::Color32::from_rgb(232, 234, 238),
                        egui::Color32::from_rgb(250, 251, 252),
                        egui::Color32::from_rgb(40, 44, 52),
                        egui::Color32::from_rgb(60, 64, 72),
                    )
                };
                let accent = egui::Color32::from_rgb(0x2B, 0x7B, 0xE4);
                let painter = ui.painter();
                painter.rect_filled(ui.max_rect(), 0.0, bg);
                for (n, name) in names.iter().enumerate() {
                    let origin = egui::pos2(
                        10.0 + (n % per_row) as f32 * cell_w,
                        10.0 + (n / per_row) as f32 * cell_h,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_size(origin, egui::vec2(cell_w - 8.0, cell_h - 8.0)),
                        8.0,
                        card,
                    );
                    painter.text(
                        origin + egui::vec2(10.0, 6.0),
                        egui::Align2::LEFT_TOP,
                        format!("{} {name}", n + 1),
                        egui::FontId::proportional(12.0),
                        text,
                    );
                    for (v, variant) in variants.iter().enumerate() {
                        let x = origin.x + 10.0 + v as f32 * 70.0;
                        let big = egui::Rect::from_min_size(
                            egui::pos2(x, origin.y + 26.0),
                            egui::Vec2::splat(44.0),
                        );
                        let full = format!("{name}{variant}");
                        super::ui::icons::paint(painter, big, &full, ink, accent);
                        let small = egui::Rect::from_min_size(
                            egui::pos2(x + 48.0, origin.y + 46.0),
                            egui::Vec2::splat(20.0),
                        );
                        super::ui::icons::paint(painter, small, &full, ink, accent);
                        let caption = match *variant {
                            "" => "now",
                            "~a" => "A",
                            "~b" => "B",
                            _ => "C",
                        };
                        painter.text(
                            egui::pos2(x + 22.0, origin.y + 84.0),
                            egui::Align2::CENTER_CENTER,
                            caption,
                            egui::FontId::proportional(12.0),
                            text,
                        );
                    }
                }
            });
        h.run();
        h.run();
        let image = h.render().expect("render");
        let suffix = if drafts { "-drafts" } else { "" };
        image
            .save(dir.join(format!("icons{suffix}-{theme}.png")))
            .expect("save");
    }
}

/// An airport code in the search box (roadmap 5, `TRK-02`), with a sample airport
/// list: the airport is the first suggestion, and Enter flies there. Manual.
#[test]
#[ignore = "manual: renders the search box with an airport code"]
fn airport_search_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let sample = "kind\tiata\ticao\tlat\tlon\televation_ft\tcountry\tmunicipality\tname\n\
        L\tFRA\tEDDF\t50.03625\t8.55929\t364\tDE\tFrankfurt am Main\tFrankfurt am Main Airport\n\
        L\tMUC\tEDDM\t48.35378\t11.78609\t1487\tDE\tMunich\tMunich Airport\n";
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.live_airports
        .set(globecraft_data::airports::Airports::parse(sample).expect("sample"));
    app.search.query = "fra".into();
    app.on_query_changed();
    settle(&mut h, 1.0);
    save(&mut h, &dir, "25-airport-search");
    let ctx = h.ctx.clone();
    h.state_mut().start_search(&ctx);
    settle(&mut h, 4.0);
    save(&mut h, &dir, "26-airport-flown-to");
}

/// A river's name written along it, as street names are (made-up river). Manual.
#[test]
#[ignore = "manual: renders a river name along the river"]
fn river_name_screenshot() {
    use egui_kittest::Harness;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0));
    let geometry = super::live_layers::river_scene(rect);
    let mut shapes = super::live_layers::PoiShapes::default();
    let mut fonts_set = false;
    let mut h = Harness::builder()
        .with_size(rect.size())
        .with_pixels_per_point(2.0)
        .renderer(WgpuTestRenderer::new())
        .build_ui(move |ui| {
            if !fonts_set {
                ui.ctx().set_fonts(super::fonts::base_definitions());
                fonts_set = true;
                return;
            }
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 40, 30));
            shapes.prepare(
                painter,
                &geometry,
                &globecraft_data::settings::map_style::MapStyle::default(),
                &globecraft_render::overlays::LabelSpace::default(),
            );
            shapes.draw_lines(painter);
            shapes.draw_labels(painter);
        });
    h.run_steps(3);
    std::thread::sleep(std::time::Duration::from_millis(900));
    h.run_steps(2);
    let image = h.render().expect("render");
    image.save(dir.join("27-river-name.png")).expect("save");
}

/// The Time zones layer (roadmap 5, `REF-05`) with made-up zones 15° wide: their
/// borders, and the local time under the cursor over Berlin at the app's time. Manual.
#[test]
#[ignore = "manual: renders time zone borders and the local time"]
fn time_zones_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    // A zone's outline with a point every 5° along its sides, as real outlines have.
    let ring = |west: f64| {
        let east = west + 15.0;
        let mut points: Vec<[f64; 2]> = (0..=34)
            .map(|k| [east, -85.0 + 5.0 * f64::from(k)])
            .collect();
        points.extend((0..=34).map(|k| [west, 85.0 - 5.0 * f64::from(k)]));
        points.push([east, -85.0]);
        points
    };
    let mut features = Vec::new();
    for i in -12..12 {
        let west = f64::from(i) * 15.0 - 7.5;
        let tzid = match i {
            1 => "Europe/Berlin".to_owned(),
            0 => "Europe/London".to_owned(),
            _ => format!("Etc/GMT{:+}", -i),
        };
        features.push(json!({
            "type": "Feature",
            "properties": {"tzid": tzid},
            "geometry": {"type": "Polygon", "coordinates": [ring(west)]}
        }));
    }
    // Where the clock changes, and each clock's label, as the published file has.
    for i in -12..=12 {
        let lon = f64::from(i) * 15.0 - 7.5;
        features.push(json!({
            "type": "Feature", "properties": {"kind": "line"},
            "geometry": {"type": "LineString", "coordinates": [[lon, -85.0], [lon, 85.0]]}
        }));
    }
    for i in -12..12 {
        let label = if i == 0 {
            "UTC/+1".to_owned()
        } else if i == 1 {
            "UTC+1/+2".to_owned()
        } else {
            format!("UTC{i:+}")
        };
        features.push(json!({
            "type": "Feature", "properties": {"kind": "label", "label": label},
            "geometry": {"type": "Point", "coordinates": [f64::from(i) * 15.0, 20.0]}
        }));
    }
    // The published file instead, when given.
    let real = std::env::var("GLOBECRAFT_TIMEZONES_FILE").ok();
    let zones = match &real {
        Some(path) => globecraft_data::timezones::TimeZones::from_gzip(
            &std::fs::read(path).expect("time zones file"),
        ),
        None => globecraft_data::timezones::TimeZones::parse(&json!({
            "type": "FeatureCollection", "features": features
        })),
    }
    .expect("zones");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.live_time_zones.set(zones);
    app.run(
        "layer.show",
        json!({"layer": "time_zones", "visible": true}),
    )
    .expect("layer");
    app.run("time.set", json!({"time": "2026-07-01T12:00:00Z"}))
        .expect("time");
    app.run(
        "camera.set",
        json!({"lat": 50.0, "lon": 10.0, "altitude_km": 9000.0}),
    )
    .expect("camera");
    settle(&mut h, 3.0);
    // The pointer near the middle of the globe's view (10° E: the Berlin zone).
    let size = h.ctx.content_rect().size();
    let centre = egui::pos2(160.0 + size.x / 2.0, 95.0 + size.y / 2.0);
    for _ in 0..3 {
        h.event(egui::Event::PointerMoved(centre));
        h.step();
    }
    settle(&mut h, 1.0);
    save(
        &mut h,
        &dir,
        if real.is_some() {
            "28-time-zones-published"
        } else {
            "28-time-zones"
        },
    );
    let cursor = h.state().cursor.expect("pointer over the globe");
    assert!((2.5..17.5).contains(&cursor.1), "{cursor:?}");
    let local = h.state().cursor_local_time().expect("local time");
    assert_eq!(local, "14:00 CEST (UTC+2)");
    if real.is_some() {
        // The Americas, and the whole Earth from far away.
        for (name, lat, lon, altitude) in [
            ("28-time-zones-americas", 25.0, -90.0, 9000.0),
            ("28-time-zones-asia", 30.0, 95.0, 9000.0),
            // North of Greenland and the date line (user, 2026-11-08).
            ("28-time-zones-greenland", 80.0, -40.0, 4000.0),
            ("28-time-zones-date-line", 0.0, 180.0, 9000.0),
            ("28-time-zones-date-line-north", 62.0, -175.0, 4000.0),
            // The Antarctic Peninsula's base, where the coast lines strayed (user,
            // 2026-11-08).
            ("28-time-zones-peninsula", -72.3, -71.2, 520.0),
            ("28-time-zones-peninsula-wide", -72.0, -70.0, 2500.0),
        ] {
            h.state_mut()
                .run(
                    "camera.set",
                    json!({"lat": lat, "lon": lon, "altitude_km": altitude}),
                )
                .expect("camera");
            settle(&mut h, 3.0);
            save(&mut h, &dir, name);
        }
        // Opacity and solo (roadmap 6.1): the zones at 35 %, shown alone, with the
        // Layers pane open on them.
        let app = h.state_mut();
        app.settings.workspace.ribbon_tab = Tab::Layers;
        let _ = app.run("ui.pane.layers", Value::Null);
        app.run(
            "layer.opacity",
            json!({"layer": "time_zones", "opacity": 0.35}),
        )
        .expect("opacity");
        app.run("layer.solo", json!({"layer": "time_zones"}))
            .expect("solo");
        assert!(!app.layer(globecraft_core::layers::MapLayer::CountryBorders));
        app.run(
            "camera.set",
            json!({"lat": 48.0, "lon": 10.0, "altitude_km": 9000.0}),
        )
        .expect("camera");
        settle(&mut h, 3.0);
        save(&mut h, &dir, "28-time-zones-solo-faded");
        let app = h.state_mut();
        app.run("layer.solo", Value::Null).expect("show all");
        assert!(app.layer(globecraft_core::layers::MapLayer::CountryBorders));
    }
}

/// A selected flight: its track so far coloured by altitude, the rest
/// of its way to its destination dashed, both airports and the route card. Made-up
/// Frankfurt to New York flight over the Atlantic; nothing is downloaded. Manual.
#[test]
#[ignore = "manual: renders a selected flight's track, route and card"]
fn selected_flight_screenshot() {
    use globecraft_data::aircraft::{
        Aircraft,
        route::{Route, RouteAirport, SelectedFlight, TrackPoint},
    };
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let (frankfurt, new_york) = ((50.033, 8.570), (40.640, -73.779));
    // Three hours along the great circle, one point a minute, climbing for 25 min.
    let now = globecraft_core::math::now_unix();
    let path = globecraft_core::math::great_circle(frankfurt, new_york, 480);
    let track: Vec<TrackPoint> = path[..=180]
        .iter()
        .enumerate()
        .map(|(minute, point)| {
            let (lat, lon) = globecraft_core::math::v3_to_ll(*point);
            TrackPoint {
                unix: now - 60.0 * (180 - minute) as f64 - 4.0,
                lat,
                lon,
                altitude_feet: Some((minute as f64 * 1500.0).min(37_000.0)),
            }
        })
        .collect();
    let last = *track.last().expect("track");
    let next = globecraft_core::math::v3_to_ll(path[181]);
    let heading = {
        let (a, b) = (
            (last.lat.to_radians(), last.lon.to_radians()),
            (next.0.to_radians(), next.1.to_radians()),
        );
        let y = (b.1 - a.1).sin() * b.0.cos();
        let x = a.0.cos() * b.0.sin() - a.0.sin() * b.0.cos() * (b.1 - a.1).cos();
        y.atan2(x).to_degrees().rem_euclid(360.0)
    };
    let airport = |code: &str, name: &str, (lat, lon): (f64, f64)| RouteAirport {
        code: code.into(),
        name: name.into(),
        lat,
        lon,
    };
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.run("layer.show", json!({"layer": "flights", "visible": true}))
        .expect("layer");
    app.flight_selection.set(SelectedFlight {
        hex: "3c6a9a".into(),
        callsign: "DLH400".into(),
        track: track.into(),
        aircraft: globecraft_data::aircraft::route::AircraftInfo {
            description: Some("BOEING 747-8".into()),
            operator: Some("Lufthansa".into()),
            year: Some("2015".into()),
        },
        route: Some(Route {
            origin: airport("FRA", "Frankfurt am Main", frankfurt),
            destination: airport("JFK", "New York", new_york),
            plausible: true,
        }),
        position: Some(Aircraft {
            hex: "3c6a9a".into(),
            callsign: "DLH400".into(),
            latitude: last.lat,
            longitude: last.lon,
            heading_deg: heading,
            speed_knots: Some(482.0),
            altitude_feet: Some(37_000.0),
            vertical_rate_fpm: Some(0.0),
            turn_rate_deg_s: None,
            observed_unix: now - 4.0,
            details: Some(Arc::new(globecraft_data::aircraft::AircraftDetails {
                registration: Some("D-ABYT".into()),
                type_code: Some("B748".into()),
                category: Some("A5".into()),
                squawk: Some("2311".into()),
                geometric_altitude_feet: Some(37_425.0),
                indicated_airspeed_knots: Some(262.0),
                true_airspeed_knots: Some(478.0),
                mach: Some(0.84),
                wind: Some((265.0, 62.0)),
                outside_air_celsius: Some(-56.0),
                selected_altitude_feet: Some(37_008.0),
                selected_heading_deg: Some(281.0),
                source: Some("adsb_icao".into()),
                ..Default::default()
            })),
            class: globecraft_data::aircraft::AircraftClass::Heavy,
        }),
        route_known: true,
        error: None,
    });
    app.run(
        "camera.set",
        json!({"lat": 52.0, "lon": -30.0, "altitude_km": 7000.0}),
    )
    .expect("camera");
    settle(&mut h, 3.0);
    save(&mut h, &dir, "29-selected-flight");
    // From behind Frankfurt, tilted: the track climbs from the ground to its height.
    h.state_mut()
        .run(
            "camera.set",
            json!({"lat": 50.4, "lon": 9.6, "altitude_km": 60.0, "tilt_deg": 74.0, "heading_deg": 290.0}),
        )
        .expect("camera");
    settle(&mut h, 3.0);
    save(&mut h, &dir, "29-selected-flight-tilted");
    let selected = h.state_mut().run("flight.clear", json!({})).expect("clear");
    assert_eq!(selected["selected"], Value::Null);
    assert!(
        h.state_mut()
            .run("flight.select", json!({"hex": "nothex"}))
            .is_err()
    );
}

/// The legend: it opens by itself with a selected flight, rain radar, aurora and
/// places of interest on, closes with `ui.legend`, stays closed while nothing new
/// appears, and opens again when a key comes back. Manual.
#[test]
#[ignore = "manual: renders the legend window"]
fn legend_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    for layer in ["flights", "radar", "aurora", "points_of_interest"] {
        app.run("layer.show", json!({"layer": layer, "visible": true}))
            .expect("layer");
    }
    app.flight_selection
        .set(globecraft_data::aircraft::route::SelectedFlight {
            hex: "3c6a9a".into(),
            callsign: "DLH400".into(),
            route_known: true,
            ..Default::default()
        });
    settle(&mut h, 2.0);
    assert!(h.state().legend_open(), "the legend opens by itself");
    save(&mut h, &dir, "30-legend");
    h.state_mut()
        .run("ui.legend", json!({"on": false}))
        .expect("close");
    settle(&mut h, 0.5);
    assert!(!h.state().legend_open(), "closed stays closed");
    h.state_mut()
        .run("layer.show", json!({"layer": "aurora", "visible": false}))
        .expect("aurora off");
    settle(&mut h, 0.5);
    assert!(
        !h.state().legend_open(),
        "a key going away does not open it"
    );
    h.state_mut()
        .run("layer.show", json!({"layer": "aurora", "visible": true}))
        .expect("aurora on");
    settle(&mut h, 0.5);
    assert!(h.state().legend_open(), "a key coming back opens it");
}

/// The polar caps (roadmap 3.5): both poles from afar and Antarctica closer, with
/// the country borders and, given `GLOBECRAFT_TIMEZONES_FILE`, the time zones. Needs
/// the network (NASA GIBS polar tiles). Manual.
#[test]
#[ignore = "manual: renders both poles with their polar imagery"]
fn polar_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    if let Ok(path) = std::env::var("GLOBECRAFT_TIMEZONES_FILE") {
        let zones = globecraft_data::timezones::TimeZones::from_gzip(
            &std::fs::read(path).expect("time zones file"),
        )
        .expect("zones");
        let app = h.state_mut();
        app.live_time_zones.set(zones);
        app.run(
            "layer.show",
            json!({"layer": "time_zones", "visible": true}),
        )
        .expect("time zones");
    }
    for (name, lat, lon, altitude, heading) in [
        ("31-polar-south", -90.0, 0.0, 6_000.0, 0.0),
        ("31-polar-south-close", -86.5, 30.0, 900.0, 0.0),
        // The seam between Esri's imagery and the cap, close enough to see its tones.
        ("31-polar-south-seam", -83.0, 60.0, 1_200.0, 0.0),
        // The user's view of 2026-11-05.
        ("31-polar-south-user", -85.676, -38.41, 6_882.0, 227.0),
        ("31-polar-north", 90.0, 0.0, 6_000.0, 0.0),
    ] {
        h.state_mut()
            .run(
                "camera.set",
                json!({"lat": lat, "lon": lon, "altitude_km": altitude, "heading_deg": heading}),
            )
            .expect("camera");
        settle(&mut h, 8.0);
        save(&mut h, &dir, name);
    }
}

/// Answers every request forever: OpenSky-style state vectors for `states/all`,
/// ADSB.lol-style aircraft for anything else.
fn fake_aircraft_server(opensky: serde_json::Value, adsb: serde_json::Value) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else {
                return;
            };
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let body = if request.contains("states/all") {
                opensky.to_string()
            } else {
                adsb.to_string()
            };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

/// Roadmap 13.6 and G: every kind of aircraft with its own symbol (made-up flights
/// over the Strait of Dover from a local server), ships coloured by type and sized by
/// length (OpenSeaFeed), and satellites by group at their real height, predicted with
/// SGP4 from CelesTrak's elements, with the legend. Needs the network. Manual.
#[test]
#[ignore = "manual: renders aircraft, ship and satellite symbols"]
fn live_symbols_screenshot() {
    use globecraft_data::aircraft::LiveAircraft;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    // ADSB.lol categories for most, OpenSky's numbers (index 17) for the rest.
    let adsb: Vec<serde_json::Value> = [
        ("A3", "EZY81AB", 0.0, 0.0, 60.0),
        ("A5", "BAW117", 0.0, 0.35, 250.0),
        ("A1", "GBXYZ", 0.0, 0.7, 120.0),
        ("A6", "RRR7201", 0.25, 0.0, 300.0),
        ("A7", "GHELI", 0.25, 0.35, 15.0),
        ("B1", "GLIDER1", 0.25, 0.7, 200.0),
    ]
    .iter()
    .enumerate()
    .map(|(index, (category, flight, dlat, dlon, track))| {
        json!({
            "hex": format!("40{index:04x}"), "flight": flight, "category": category,
            "lat": 50.75 + dlat, "lon": 1.0 + dlon, "track": track, "gs": 240.0,
            "alt_baro": 4000 + 6000 * index, "seen_pos": 0.5
        })
    })
    .collect();
    let now = globecraft_core::math::now_unix();
    let opensky: Vec<serde_json::Value> = [(10, 0.0), (12, 0.35), (14, 0.7)]
        .iter()
        .enumerate()
        .map(|(index, (category, dlon))| {
            json!([
                format!("41{index:04x}"),
                format!("TEST{index}"),
                "United Kingdom",
                now,
                now,
                1.0 + dlon,
                50.5,
                600.0,
                false,
                30.0,
                90.0,
                0.0,
                null,
                null,
                null,
                false,
                0,
                category
            ])
        })
        .collect();
    let port = fake_aircraft_server(json!({"states": opensky}), json!({"ac": adsb}));
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.live_aircraft = LiveAircraft::with_sources(
        app.runtime.handle(),
        || {},
        format!("http://127.0.0.1:{port}/api/states/all?extended=1"),
        format!("http://127.0.0.1:{port}/v2"),
    );
    for layer in ["flights", "vessels"] {
        app.run("layer.show", json!({"layer": layer, "visible": true}))
            .expect("layer");
    }
    app.run(
        "camera.set",
        json!({"lat": 50.75, "lon": 1.35, "altitude_km": 160.0}),
    )
    .expect("camera");
    settle(&mut h, 12.0);
    save(&mut h, &dir, "32-live-symbols");
    // Tilted, the aircraft fly at their height above the Channel; flat, on it.
    h.state_mut()
        .run(
            "camera.set",
            json!({"lat": 50.2, "lon": 1.35, "altitude_km": 25.0, "tilt_deg": 72.0}),
        )
        .expect("camera");
    settle(&mut h, 6.0);
    save(&mut h, &dir, "32-live-symbols-tilted");
    h.state_mut()
        .run("layer.flatLiveObjects", json!({"on": true}))
        .expect("flat");
    settle(&mut h, 3.0);
    save(&mut h, &dir, "32-live-symbols-flat");
    h.state_mut()
        .run("layer.flatLiveObjects", json!({"on": false}))
        .expect("heights");
    let app = h.state_mut();
    for (layer, visible) in [("flights", false), ("vessels", false), ("satellites", true)] {
        app.run("layer.show", json!({"layer": layer, "visible": visible}))
            .expect("layer");
    }
    // Recorded answers when CelesTrak is not reachable from here (it limits repeated
    // downloads): GLOBECRAFT_CELESTRAK_DIR holds STATIONS.json, GNSS.json, WEATHER.json
    // and VISUAL.json.
    if let Ok(folder) = std::env::var("GLOBECRAFT_CELESTRAK_DIR") {
        use globecraft_data::satellites::SatelliteGroup;
        let answers: Vec<_> = [
            (SatelliteGroup::Station, "STATIONS"),
            (SatelliteGroup::Navigation, "GNSS"),
            (SatelliteGroup::Weather, "WEATHER"),
            (SatelliteGroup::Bright, "VISUAL"),
        ]
        .into_iter()
        .map(|(group, name)| {
            let path = std::path::Path::new(&folder).join(format!("{name}.json"));
            let text = std::fs::read_to_string(&path).expect("recorded CelesTrak answer");
            (group, serde_json::from_str(&text).expect("CelesTrak JSON"))
        })
        .collect();
        h.state_mut().live_satellites.show_recorded(&answers);
    }
    for _ in 0..60 {
        if !h.state().live_satellites.snapshot().is_empty() {
            break;
        }
        settle(&mut h, 1.0);
    }
    let satellites = h.state().live_satellites.snapshot();
    assert!(
        !satellites.is_empty(),
        "{}",
        h.state().live_satellites.status(h.state().lang)
    );
    println!(
        "{} satellites, {} with SGP4",
        satellites.len(),
        satellites.iter().filter(|s| s.sgp4.is_some()).count()
    );
    // The ISS selected: one orbit at its height, from far away and from its side.
    let iss = h
        .state()
        .live_satellites
        .snapshot()
        .iter()
        .find(|satellite| satellite.norad == "25544")
        .and_then(|satellite| satellite.position(globecraft_core::math::now_unix()));
    if let Some(at) = iss {
        h.state_mut()
            .run("satellite.select", json!({"norad": "25544"}))
            .expect("select");
        for (name, camera) in [
            (
                "32-satellite-orbit",
                json!({"lat": at.lat, "lon": at.lon, "altitude_km": 14_000.0}),
            ),
            (
                "32-satellite-orbit-tilted",
                json!({"lat": at.lat, "lon": at.lon, "altitude_km": 2_500.0, "tilt_deg": 65.0}),
            ),
            // The craft docked to it drawn as the ISS, "+n" and the card listing them
            // (user, 2026-11-08).
            (
                "32-satellite-docked",
                json!({"lat": at.lat, "lon": at.lon, "altitude_km": 800.0, "tilt_deg": 0.0}),
            ),
        ] {
            h.state_mut().run("camera.set", camera).expect("camera");
            settle(&mut h, 4.0);
            save(&mut h, &dir, name);
        }
        h.state_mut()
            .run("satellite.clear", json!({}))
            .expect("clear");
    }
    // Far orbits, closed loops in space (user, 2026-11-08): a GLONASS satellite (12 h,
    // round) and Arktika-M 1 (12 h, very eccentric), from 50,000 km.
    for (name, norad) in [
        ("32-satellite-orbit-glonass", "32395"),
        ("32-satellite-orbit-arktika", "47719"),
    ] {
        let loaded = h
            .state()
            .live_satellites
            .snapshot()
            .iter()
            .any(|satellite| satellite.norad == norad);
        if !loaded {
            println!("{norad} not in the recorded groups: {name} skipped");
            continue;
        }
        h.state_mut()
            .run("satellite.select", json!({"norad": norad}))
            .expect("select");
        h.state_mut()
            .run(
                "camera.set",
                json!({"lat": 0.0, "lon": -60.0, "altitude_km": 50_000.0}),
            )
            .expect("camera");
        settle(&mut h, 3.0);
        save(&mut h, &dir, name);
        h.state_mut()
            .run("satellite.clear", json!({}))
            .expect("clear");
    }
    for (name, camera) in [
        (
            "32-satellites",
            json!({"lat": 20.0, "lon": 10.0, "altitude_km": 50_000.0}),
        ),
        (
            "32-satellites-tilted",
            json!({"lat": 35.0, "lon": 5.0, "altitude_km": 4_000.0, "tilt_deg": 60.0}),
        ),
    ] {
        h.state_mut().run("camera.set", camera).expect("camera");
        settle(&mut h, 12.0);
        save(&mut h, &dir, name);
    }
}

/// The ruler's elevation profile (roadmap 1.10): Geneva over Mont Blanc to Aosta,
/// the Measure pane open, then the pointer on the profile's highest point.
#[test]
#[ignore = "manual: downloads elevation tiles"]
fn ruler_profile_screenshot() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.cam.lat = 45.95;
    app.cam.lon = 6.75;
    app.cam.alt = 180.0;
    app.settings.workspace.open_right(RightPane::Measure);
    for (lat, lon) in [(46.204, 6.143), (45.833, 6.865), (45.737, 7.320)] {
        let _ = app.run("ruler.addPoint", json!({"lat": lat, "lon": lon}));
    }
    // Until every sample has its height (or 30 s).
    let start = std::time::Instant::now();
    let mut answer = Value::Null;
    while start.elapsed() < std::time::Duration::from_secs(30) {
        settle(&mut h, 0.5);
        answer = h
            .state_mut()
            .run("ruler.profile", Value::Null)
            .expect("profile");
        if answer["loaded"] == answer["sampled"] {
            break;
        }
    }
    println!(
        "profile: {} of {} samples, lowest {} m, highest {} m, up {} m, down {} m",
        answer["loaded"],
        answer["sampled"],
        answer["lowest_m"],
        answer["highest_m"],
        answer["ascent_m"],
        answer["descent_m"]
    );
    let highest = answer["highest_m"].as_f64().expect("heights");
    assert!(
        (4_000.0..4_900.0).contains(&highest),
        "Mont Blanc is 4,806 m: {highest}"
    );
    settle(&mut h, 1.0);
    save(&mut h, &dir, "33-ruler-profile");
    // The pointer on the plot, a little right of the middle: Mont Blanc's side.
    let plot = {
        use egui_kittest::kittest::Queryable;
        h.get_by_label("Elevation profile").rect()
    };
    let at = egui::pos2(plot.center().x + 30.0, plot.bottom() + 60.0);
    h.hover_at(at);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "33-ruler-profile-hover");
    // Closed: the third leg back to Geneva, and the area of the triangle.
    let answer = h
        .state_mut()
        .run("ruler.closeShape", json!({"on": true}))
        .expect("close");
    let area = answer["area_km2"].as_f64().expect("an area");
    println!("closed shape: {area:.0} km², {} points", answer["points"]);
    // By hand (flat, 111 km a degree north, 77.5 east): about 430 km².
    assert!((400.0..480.0).contains(&area), "{area}");
    settle(&mut h, 1.0);
    save(&mut h, &dir, "33-ruler-area");
}

/// A view link (roadmap 2.6) copied, the view changed, then the link pasted into the
/// search box: the camera, basemap, layers and web data come back.
#[test]
#[ignore = "manual: builds the whole app"]
fn view_link_round_trip() {
    use globecraft_core::layers::MapLayer;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.run(
        "camera.set",
        json!({"lat": 27.988, "lon": 86.925, "altitude_km": 40.0, "heading_deg": 200.0, "tilt_deg": 60.0}),
    )
    .expect("camera");
    app.run("layer.basemap", json!({"basemap": "topo"}))
        .expect("basemap");
    app.run("layer.show", json!({"layer": "peaks", "visible": true}))
        .expect("peaks");
    app.run(
        "layer.show",
        json!({"layer": "country_borders", "visible": false}),
    )
    .expect("borders");
    app.run("data.add", json!({"preset": "earthquakes"}))
        .expect("data");
    let link = app.run("view.copyLink", Value::Null).expect("copy")["link"]
        .as_str()
        .expect("link")
        .to_owned();
    println!("{link}");
    // Somewhere else entirely, with the layers changed and the data removed.
    app.run(
        "camera.set",
        json!({"lat": 0.0, "lon": 0.0, "altitude_km": 9000.0, "tilt_deg": 0.0}),
    )
    .expect("camera");
    app.run("layer.basemap", json!({"basemap": "satellite"}))
        .expect("basemap");
    app.run("layer.show", json!({"layer": "peaks", "visible": false}))
        .expect("peaks");
    app.run(
        "layer.show",
        json!({"layer": "country_borders", "visible": true}),
    )
    .expect("borders");
    let id = app.settings.user_layers[0].id;
    app.run("data.remove", json!({"id": id})).expect("remove");
    // The link, typed into the search box.
    app.search.query = link.clone();
    let ctx = app.ctx.clone();
    app.start_search(&ctx);
    let app = h.state();
    assert!((app.cam.lat - 27.988).abs() < 1e-4 && (app.cam.lon - 86.925).abs() < 1e-4);
    assert!((app.cam.alt - 40.0).abs() < 1e-6 && (app.cam.tilt - 60.0).abs() < 0.1);
    assert_eq!(app.settings.layers.basemap.code(), "topo");
    assert!(app.layer(MapLayer::Peaks) && !app.layer(MapLayer::CountryBorders));
    assert_eq!(app.settings.user_layers.len(), 1);
    assert_eq!(
        app.settings.user_layers[0].preset.as_deref(),
        Some("earthquakes")
    );
    settle(&mut h, 4.0);
    save(&mut h, &dir, "34-view-link-opened");
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
        let app = h.state();
        let loading =
            !app.renderer.is_idle() || app.cache.inflight() > 0 || app.boundary_loader.is_loading();
        quiet = if loading || delay(&h) < std::time::Duration::from_secs(5) {
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
