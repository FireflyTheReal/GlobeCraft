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

mod assistant;
mod interface;
mod live;
mod map_layers;
mod performance;
mod tools;
mod user_data;

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

/// How many polls in a row the application must be idle before [`settle`] returns.
const SETTLE_QUIET_POLLS: usize = 5;

/// Whether the application is still working: drawing, loading tiles or borders, or
/// flying the camera.
fn busy(app: &App) -> bool {
    !app.imagery.renderer.is_idle()
        || app.cache.inflight() > 0
        || app.borders.loader.is_loading()
        || app.fly.is_some()
}

/// Steps the interface every 100 ms until the application has been idle for half a
/// second, or `seconds` have passed.
fn settle(h: &mut Harness<'static, App>, seconds: f32) {
    let steps = (seconds / 0.1) as usize;
    let mut quiet = 0;
    for _ in 0..steps {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(100));
        quiet = if busy(h.state()) { 0 } else { quiet + 1 };
        if quiet >= SETTLE_QUIET_POLLS {
            break;
        }
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
