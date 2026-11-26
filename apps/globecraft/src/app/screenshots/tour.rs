//! The demo video's tour: planned camera paths rendered one frame at a time on a
//! fixed clock. Each frame waits until its imagery, relief and buildings are loaded
//! and the full-size picture is drawn, so the film is smooth however long a frame
//! takes. Manual (network):
//!
//! ```sh
//! XDG_CACHE_HOME=/tmp/gc-cache GLOBECRAFT_SHOT_SIZE=1280x720 GLOBECRAFT_SHOT_PPP=1.5 \
//!   TOUR_DIR=/tmp/tour TOUR_SHOTS=space,alps TOUR_STRIDE=30 \
//!   cargo test -p globecraft --release demo_tour -- --ignored --nocapture
//! ```
//!
//! `TOUR_SHOTS` picks shots by name (all by default), `TOUR_STRIDE` renders every
//! n-th frame only (test stills), `TOUR_FROM` skips frames before it. Frames are
//! JPEG (quality 92) in `TOUR_DIR/<shot>/00000.jpg`; frames already there are kept,
//! so a stopped render goes on where it stopped.
//!
//! The tour owns the camera: `MotionState::scripted` stops the app following the
//! ground under the view, and the looked-at point's height comes from a smoothed
//! elevation profile along the shot's path.
use super::{App, ThemePreference, busy, harness_with, settle};
use eframe::egui;
use egui_kittest::Harness;
use globecraft_core::commands::Tab;
use globecraft_data::settings::{Settings, workspace::PaneTab};
use serde_json::json;
use std::time::{Duration, Instant};

/// Frames a second of the film.
const FPS: f64 = 30.0;
/// The elevation zoom of the ground profile: about a valley across, not single rocks.
const GROUND_ZOOM: u8 = 11;
/// The profile's smoothing, frames either side.
const GROUND_SMOOTHING: usize = 15;

/// A camera position at `at` (0–1) through a shot: the looked-at point, the distance
/// to it, the heading, the tilt about the point (≤ 85°) and the look up past it,
/// about the eye.
#[derive(Clone, Copy)]
struct Key {
    at: f64,
    lat: f64,
    lon: f64,
    alt_km: f64,
    heading: f64,
    tilt: f64,
    look: f64,
}

const fn key(at: f64, lat: f64, lon: f64, alt_km: f64, heading: f64, tilt: f64) -> Key {
    Key {
        at,
        lat,
        lon,
        alt_km,
        heading,
        tilt,
        look: 0.0,
    }
}

const fn looking(k: Key, look: f64) -> Key {
    Key { look, ..k }
}

#[derive(Clone, Copy)]
enum Ease {
    Linear,
    InOut,
}

/// The scene time through a shot.
#[derive(Clone, Copy)]
enum Clock {
    /// A fixed moment (Unix seconds).
    At(f64),
    /// From a moment, this many whole minutes of scene time a frame (the frame's time
    /// counts to the minute at rest).
    Lapse(f64, f64),
    /// Through these (shot fraction 0–1, Unix seconds) points, smoothly: a time-lapse
    /// that slows down where it matters.
    Curve(&'static [(f64, f64)]),
    /// The real time, for live data.
    Live,
}

struct Shot {
    name: &'static str,
    seconds: f64,
    ease: Ease,
    clock: Clock,
    layers: &'static [(&'static str, bool)],
    keys: Vec<Key>,
    /// The ribbon tab shown.
    tab: Tab,
    timeline: bool,
    /// Selects the aircraft nearest the view's centre at this point (0–1).
    select_flight_at: Option<f64>,
    /// Key longitudes count from the longitude where it is midnight at the frame's
    /// time (for live shots of the night side).
    from_midnight: bool,
    /// The base map ("satellite", "topo", "streets").
    basemap: &'static str,
    /// A library source added for the shot (`data.add` preset).
    data: Option<&'static str>,
    sunlight: bool,
    /// Waits for each frame's cloud map (archived clouds of a time set back).
    wait_clouds: bool,
    /// A Wayback swipe compare, its split moving from one fraction of the view's
    /// width to the other (the newest release right of it).
    swipe: Option<(f64, f64)>,
    /// Frames rendered instead of `seconds × FPS` (one a moment, blended later).
    frames: Option<usize>,
    /// A side pane open through the shot.
    pane: Option<PaneTab>,
    /// A ruler drawn along these points while the shot plays (from 10 % to 85 %).
    ruler: &'static [(f64, f64)],
    /// Wall-clock seconds a frame takes at least, for layers that move with the real
    /// time (satellites): a steady time-lapse of them.
    pace: Option<f64>,
    /// Recorded CelesTrak answers shown (`GLOBECRAFT_CELESTRAK_DIR`) and this
    /// satellite selected.
    satellite: Option<&'static str>,
}

impl Default for Shot {
    fn default() -> Self {
        Self {
            name: "",
            seconds: 1.0,
            ease: Ease::Linear,
            clock: Clock::Live,
            layers: &[],
            keys: Vec::new(),
            tab: Tab::Explore,
            timeline: false,
            select_flight_at: None,
            from_midnight: false,
            basemap: "satellite",
            data: None,
            sunlight: true,
            wait_clouds: false,
            swipe: None,
            frames: None,
            pane: None,
            ruler: &[],
            pace: None,
            satellite: None,
        }
    }
}

/// 2026-11-20 00:00 UTC.
const DAY: f64 = 1_795_132_800.0;
const HOUR: f64 = 3_600.0;
/// 2014-03-01 00:00 UTC: the Wayback archive's oldest release is of February 2014.
const MARCH_2014: f64 = 1_393_632_000.0;
/// 2026-10-19 12:00 UTC: Super Typhoon Champi east of the Philippines.
const CHAMPI: f64 = 1_792_411_200.0;
/// The sunset shot's clock (shot fraction, Unix seconds): 13:10 → 15:40 UTC in the
/// first 45 %, the sun going down slowly, then the afterglow held.
const SUNSET: &[(f64, f64)] = &[
    (0.0, DAY + 13.0 * HOUR + 10.0 * 60.0),
    (0.45, DAY + 15.0 * HOUR + 40.0 * 60.0),
    (0.85, DAY + 16.0 * HOUR + 14.0 * 60.0),
    (1.0, DAY + 16.0 * HOUR + 16.5 * 60.0),
];
/// The Matterhorn's summit.
const MATTERHORN: (f64, f64) = (45.9763, 7.6586);
/// Each clip of the data montage.
const MONTAGE_SECONDS: f64 = 1.5;

/// The layers every shot starts from: none but the base ones.
const RESET: &[&str] = &[
    "terrain",
    "stars",
    "milky_way",
    "clouds",
    "night_lights",
    "buildings",
    "flights",
    "vessels",
    "aurora",
    "radar",
    "roads",
    "railways",
    "satellites",
    "time_zones",
    "state_borders",
    "country_borders",
    "place_labels",
    "peaks",
];

fn shots() -> Vec<Shot> {
    let (mlat, mlon) = MATTERHORN;
    vec![
        // The whole Earth turning in front of the stars, the sun's light on it.
        Shot {
            name: "space",
            seconds: 5.0,
            clock: Clock::At(DAY + 11.0 * HOUR),
            layers: &[
                ("stars", true),
                ("milky_way", true),
                ("clouds", true),
                ("night_lights", true),
            ],
            keys: vec![
                key(0.0, 18.0, -25.0, 13_000.0, 0.0, 0.0),
                key(1.0, 22.0, 5.0, 11_500.0, 0.0, 0.0),
            ],
            ..Shot::default()
        },
        // Day breaks over Europe: the night lights go out under the moving sun.
        Shot {
            name: "sunrise",
            seconds: 6.0,
            clock: Clock::Lapse(DAY + 3.0 * HOUR, 2.0),
            layers: &[
                ("stars", true),
                ("clouds", true),
                ("night_lights", true),
                ("country_borders", true),
            ],
            keys: vec![
                key(0.0, 44.0, 12.0, 7_500.0, 0.0, 0.0),
                key(1.0, 46.0, 6.0, 6_500.0, 0.0, 0.0),
            ],
            timeline: true,
            ..Shot::default()
        },
        // From orbit down into the Alps; the view leans to the Matterhorn as it comes.
        Shot {
            name: "alps",
            seconds: 8.0,
            ease: Ease::InOut,
            clock: Clock::At(DAY + 15.1 * HOUR),
            layers: &[("terrain", true), ("stars", true)],
            keys: vec![
                key(0.0, 46.2, 6.0, 6_000.0, 0.0, 0.0),
                key(0.45, 46.0, 7.60, 250.0, 5.0, 12.0),
                key(0.78, mlat, mlon, 34.0, 20.0, 52.0),
                key(1.0, mlat, mlon, 14.0, 30.0, 70.0),
            ],
            ..Shot::default()
        },
        // Raising the view about the eye, past the summits into the evening sky.
        Shot {
            name: "sky",
            seconds: 4.0,
            ease: Ease::InOut,
            clock: Clock::At(DAY + 15.1 * HOUR),
            layers: &[("terrain", true), ("stars", true)],
            keys: vec![
                key(0.0, mlat, mlon, 14.0, 30.0, 70.0),
                looking(key(1.0, mlat, mlon, 14.0, 42.0, 70.0), 20.0),
            ],
            ..Shot::default()
        },
        // Manhattan in 3D, with the afternoon sun's shadows.
        Shot {
            name: "city",
            seconds: 6.0,
            ease: Ease::InOut,
            clock: Clock::At(DAY + 19.0 * HOUR),
            layers: &[("buildings", true)],
            keys: vec![
                key(0.0, 40.7440, -73.9900, 1.2, 15.0, 65.0),
                key(1.0, 40.7470, -73.9880, 0.95, 45.0, 68.0),
            ],
            ..Shot::default()
        },
        // Every aircraft in the air over the North-East, live; one picked for its card.
        Shot {
            name: "flights",
            seconds: 7.0,
            ease: Ease::InOut,
            layers: &[
                ("flights", true),
                ("night_lights", true),
                ("country_borders", true),
                ("state_borders", true),
                ("place_labels", true),
            ],
            keys: vec![
                key(0.0, 40.0, -76.0, 1_400.0, 0.0, 0.0),
                key(1.0, 40.6, -74.2, 700.0, 10.0, 25.0),
            ],
            tab: Tab::Live,
            select_flight_at: Some(0.45),
            ..Shot::default()
        },
        // The sun goes down behind the Matterhorn; the peaks' shadows reach across.
        // The afternoon goes by fast, the sunset slowly, and the glow after it holds.
        Shot {
            name: "alps_sunset",
            seconds: 10.0,
            clock: Clock::Curve(SUNSET),
            layers: &[("terrain", true), ("stars", true)],
            keys: vec![
                key(0.0, mlat, mlon, 34.5, 239.0, 65.0),
                key(1.0, mlat, mlon, 25.5, 262.0, 68.5),
            ],
            ..Shot::default()
        },
        // The ruler from Mont Blanc over the Matterhorn to Monte Rosa, its elevation
        // profile growing in the Measure pane.
        Shot {
            name: "ruler",
            seconds: 6.0,
            ease: Ease::InOut,
            clock: Clock::At(DAY + 12.5 * HOUR),
            layers: &[("terrain", true), ("stars", true)],
            keys: vec![
                key(0.0, 45.90, 7.33, 105.0, 8.0, 42.0),
                key(1.0, 45.92, 7.38, 88.0, 14.0, 48.0),
            ],
            pane: Some(PaneTab::Measure),
            ruler: &[(45.8326, 6.8652), (45.9763, 7.6586), (45.9369, 7.8669)],
            ..Shot::default()
        },
        // Wayback: the forest of 2014 at Grünheide, swiped away by today's factory.
        Shot {
            name: "wayback",
            seconds: 5.0,
            ease: Ease::InOut,
            clock: Clock::At(MARCH_2014),
            layers: &[],
            keys: vec![
                key(0.0, 52.3965, 13.8010, 4.6, 0.0, 0.0),
                key(1.0, 52.3965, 13.8010, 3.9, 0.0, 0.0),
            ],
            tab: Tab::Map,
            basemap: "wayback",
            sunlight: false,
            swipe: Some((0.94, 0.06)),
            ..Shot::default()
        },
        // Super Typhoon Champi from the cloud archive, one frame a three-hour map.
        Shot {
            name: "typhoon",
            seconds: 29.0 / 6.0,
            clock: Clock::Lapse(CHAMPI, 180.0),
            layers: &[("clouds", true), ("stars", true)],
            keys: vec![
                key(0.0, 19.0, 130.0, 6_500.0, 0.0, 0.0),
                key(1.0, 19.0, 130.0, 6_500.0, 0.0, 0.0),
            ],
            tab: Tab::Live,
            sunlight: false,
            wait_clouds: true,
            frames: Some(29),
            ..Shot::default()
        },
        // The satellites, live: the ISS on its orbit, the navigation satellites'
        // shells and the weather satellites' ring as the view pulls back.
        Shot {
            name: "satellites",
            seconds: 6.0,
            ease: Ease::InOut,
            layers: &[
                ("satellites", true),
                ("stars", true),
                ("night_lights", true),
                ("clouds", true),
            ],
            keys: vec![
                key(0.0, 20.0, -30.0, 16_000.0, 0.0, 0.0),
                key(1.0, 22.0, -18.0, 46_000.0, 0.0, 0.0),
            ],
            tab: Tab::Live,
            pace: Some(5.0),
            satellite: Some("25544"),
            ..Shot::default()
        },
        // The data montage: one short clip a source, evenly lit.
        Shot {
            name: "m_radar",
            seconds: MONTAGE_SECONDS,
            layers: &[("radar", true), ("country_borders", true)],
            keys: vec![
                key(0.0, 50.0, 6.0, 3_600.0, 0.0, 0.0),
                key(1.0, 50.5, 8.0, 3_300.0, 0.0, 0.0),
            ],
            tab: Tab::Live,
            sunlight: false,
            ..Shot::default()
        },
        Shot {
            name: "m_ships",
            seconds: MONTAGE_SECONDS,
            layers: &[("vessels", true), ("country_borders", true)],
            keys: vec![
                key(0.0, 52.5, 3.0, 900.0, 0.0, 0.0),
                key(1.0, 52.8, 3.6, 800.0, 0.0, 0.0),
            ],
            tab: Tab::Live,
            sunlight: false,
            ..Shot::default()
        },
        Shot {
            name: "m_quakes",
            seconds: MONTAGE_SECONDS,
            layers: &[("country_borders", true)],
            keys: vec![
                key(0.0, 10.0, 130.0, 11_000.0, 0.0, 0.0),
                key(1.0, 12.0, 136.0, 10_500.0, 0.0, 0.0),
            ],
            tab: Tab::Map,
            data: Some("earthquakes"),
            sunlight: false,
            ..Shot::default()
        },
        Shot {
            name: "m_seafloor",
            seconds: MONTAGE_SECONDS,
            layers: &[],
            keys: vec![
                key(0.0, 30.0, -42.0, 7_000.0, 0.0, 0.0),
                key(1.0, 32.0, -38.0, 6_600.0, 0.0, 0.0),
            ],
            tab: Tab::Map,
            data: Some("gebco"),
            sunlight: false,
            ..Shot::default()
        },
        Shot {
            name: "m_forest",
            seconds: MONTAGE_SECONDS,
            layers: &[("country_borders", true)],
            keys: vec![
                key(0.0, -10.5, -62.5, 1_300.0, 0.0, 0.0),
                key(1.0, -10.0, -62.0, 1_150.0, 0.0, 0.0),
            ],
            tab: Tab::Map,
            data: Some("forest_loss"),
            sunlight: false,
            ..Shot::default()
        },
        Shot {
            name: "m_topo",
            seconds: MONTAGE_SECONDS,
            layers: &[],
            keys: vec![
                key(0.0, 46.3, 8.5, 420.0, 0.0, 0.0),
                key(1.0, 46.4, 9.0, 380.0, 0.0, 0.0),
            ],
            tab: Tab::Map,
            basemap: "topo",
            sunlight: false,
            ..Shot::default()
        },
        Shot {
            name: "m_zones",
            seconds: MONTAGE_SECONDS,
            layers: &[("time_zones", true)],
            keys: vec![
                key(0.0, 35.0, -8.0, 8_500.0, 0.0, 0.0),
                key(1.0, 35.0, 0.0, 8_000.0, 0.0, 0.0),
            ],
            tab: Tab::Map,
            sunlight: false,
            ..Shot::default()
        },
        // The aurora over the night side, live, from above the pole.
        Shot {
            name: "aurora",
            seconds: 5.0,
            layers: &[("aurora", true), ("stars", true), ("night_lights", true)],
            keys: vec![
                key(0.0, 66.0, -18.0, 2_600.0, 10.0, 58.0),
                key(1.0, 67.0, 6.0, 2_300.0, 18.0, 62.0),
            ],
            tab: Tab::Live,
            from_midnight: true,
            ..Shot::default()
        },
    ]
}

fn ease(e: Ease, t: f64) -> f64 {
    match e {
        Ease::Linear => t,
        Ease::InOut => 0.5 - 0.5 * (std::f64::consts::PI * t).cos(),
    }
}

/// Monotone cubic interpolation (Fritsch–Carlson) of `ys` at knots `xs`: smooth
/// through every key, without overshooting between them.
fn pchip(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len();
    if n == 1 {
        return ys[0];
    }
    let i = (0..n - 1).find(|&i| x <= xs[i + 1]).unwrap_or(n - 2);
    let d: Vec<f64> = (0..n - 1)
        .map(|k| (ys[k + 1] - ys[k]) / (xs[k + 1] - xs[k]))
        .collect();
    let m = |k: usize| -> f64 {
        if k == 0 {
            d[0]
        } else if k == n - 1 {
            d[n - 2]
        } else if d[k - 1] * d[k] <= 0.0 {
            0.0
        } else {
            let (w1, w2) = (
                2.0 * (xs[k + 1] - xs[k]) + (xs[k] - xs[k - 1]),
                (xs[k + 1] - xs[k]) + 2.0 * (xs[k] - xs[k - 1]),
            );
            (w1 + w2) / (w1 / d[k - 1] + w2 / d[k])
        }
    };
    let h = xs[i + 1] - xs[i];
    let t = ((x - xs[i]) / h).clamp(0.0, 1.0);
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * ys[i]
        + (t3 - 2.0 * t2 + t) * h * m(i)
        + (-2.0 * t3 + 3.0 * t2) * ys[i + 1]
        + (t3 - t2) * h * m(i + 1)
}

/// The camera at `t` (0–1) through the shot; the distance between keys in log space.
fn camera_at(shot: &Shot, t: f64) -> Key {
    let u = ease(shot.ease, t);
    let xs: Vec<f64> = shot.keys.iter().map(|k| k.at).collect();
    let along = |f: fn(&Key) -> f64| {
        let ys: Vec<f64> = shot.keys.iter().map(f).collect();
        pchip(&xs, &ys, u)
    };
    Key {
        at: t,
        lat: along(|k| k.lat),
        lon: along(|k| k.lon),
        alt_km: along(|k| k.alt_km.ln()).exp(),
        heading: along(|k| k.heading),
        tilt: along(|k| k.tilt),
        look: along(|k| k.look),
    }
}

fn frame_t(i: usize, frames: usize) -> f64 {
    i as f64 / (frames - 1).max(1) as f64
}

/// The looked-at point's ground (km) for every frame of a shot: the
/// relief under the path, loaded first, then smoothed so the camera glides over it.
fn ground_profile(h: &mut Harness<'static, App>, shot: &Shot, frames: usize) -> Vec<f64> {
    let points: Vec<(f64, f64)> = (0..frames)
        .map(|i| {
            let k = camera_at(shot, frame_t(i, frames));
            (k.lat, k.lon)
        })
        .collect();
    let sample = |app: &App| -> Vec<Option<f64>> {
        points
            .iter()
            .map(|&(lat, lon)| {
                app.tiles
                    .cache
                    .elevation_m(lat, lon, GROUND_ZOOM)
                    .map(|m| f64::from(m) / 1000.0)
            })
            .collect()
    };
    // Until every point has relief and the samples have held for 2 s: tiles not yet
    // loaded read as sea level, finer ones replace coarser ones as they arrive.
    let started = Instant::now();
    let mut heights = sample(h.state());
    let mut unchanged = 0;
    while started.elapsed() < Duration::from_secs(90) {
        h.step();
        std::thread::sleep(Duration::from_millis(250));
        let next = sample(h.state());
        unchanged = if next == heights { unchanged + 1 } else { 0 };
        heights = next;
        if unchanged >= 12
            && started.elapsed() > Duration::from_secs(20)
            && heights.iter().all(Option::is_some)
        {
            break;
        }
    }
    let missing = heights.iter().filter(|v| v.is_none()).count();
    println!(
        "  ground profile after {:.0?}: max {:.2} km, last {:?}",
        started.elapsed(),
        heights.iter().flatten().fold(0.0_f64, |a, &b| a.max(b)),
        heights.last()
    );
    if missing > 0 {
        println!("  ground: {missing} of {frames} points without relief, taken as sea level");
    }
    let raw: Vec<f64> = heights
        .into_iter()
        .map(|v| v.unwrap_or(0.0).max(0.0))
        .collect();
    let mut smooth = raw.clone();
    for _ in 0..2 {
        let prev = smooth.clone();
        for (i, out) in smooth.iter_mut().enumerate() {
            let lo = i.saturating_sub(GROUND_SMOOTHING);
            let hi = (i + GROUND_SMOOTHING).min(frames - 1);
            *out = prev[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64;
        }
    }
    smooth
}

/// The ruler's points at `t` through the shot: drawn along the path from 10 % to
/// 85 % of the shot (eased), by distance, its end between two points as it goes.
fn ruler_at(path: &[(f64, f64)], t: f64) -> Vec<(f64, f64)> {
    let u = ease(Ease::InOut, ((t - 0.1) / 0.75).clamp(0.0, 1.0));
    if u <= 0.0 || path.len() < 2 {
        return path.iter().take(1).copied().collect();
    }
    let leg = |a: (f64, f64), b: (f64, f64)| {
        let dy = b.0 - a.0;
        let dx = (b.1 - a.1) * a.0.to_radians().cos();
        dx.hypot(dy)
    };
    let total: f64 = path.windows(2).map(|w| leg(w[0], w[1])).sum();
    let mut left = u * total;
    let mut points = vec![path[0]];
    for w in path.windows(2) {
        let length = leg(w[0], w[1]);
        if left >= length {
            points.push(w[1]);
            left -= length;
        } else {
            let f = left / length;
            points.push((
                w[0].0 + (w[1].0 - w[0].0) * f,
                w[0].1 + (w[1].1 - w[0].1) * f,
            ));
            break;
        }
    }
    points
}

/// Shows the recorded CelesTrak answers in `GLOBECRAFT_CELESTRAK_DIR` (STATIONS.json,
/// GNSS.json, WEATHER.json, VISUAL.json), when CelesTrak is not reachable.
fn show_recorded_satellites(h: &mut Harness<'static, App>) {
    use globecraft_data::satellites::SatelliteGroup;
    let Ok(folder) = std::env::var("GLOBECRAFT_CELESTRAK_DIR") else {
        return;
    };
    let answers: Vec<_> = [
        (SatelliteGroup::Station, "STATIONS"),
        (SatelliteGroup::Navigation, "GNSS"),
        (SatelliteGroup::Weather, "WEATHER"),
        (SatelliteGroup::Bright, "VISUAL"),
    ]
    .into_iter()
    .filter_map(|(group, name)| {
        let path = std::path::Path::new(&folder).join(format!("{name}.json"));
        let text = std::fs::read_to_string(&path).ok()?;
        Some((group, serde_json::from_str(&text).expect("CelesTrak JSON")))
    })
    .collect();
    h.state_mut().live.satellites.show_recorded(&answers);
    println!(
        "  {} satellites from {folder}",
        h.state().live.satellites.snapshot().len()
    );
}

/// Whether the picture shown is final: nothing loading or drawing, no fade under way.
fn settled(app: &App) -> bool {
    !busy(app)
        && !app
            .frame
            .fade_started
            .is_some_and(|started| started.elapsed() < crate::app::globe::FRAME_FADE)
}

/// Steps until the picture has been final for a few polls in a row, or `limit`.
fn wait_for_frame(h: &mut Harness<'static, App>, limit: Duration) {
    let started = Instant::now();
    let mut quiet = 0;
    while started.elapsed() < limit {
        // No motion frames: the view counts as at rest at once.
        h.state_mut().frame.last_interact = Instant::now() - Duration::from_secs(5);
        h.step();
        std::thread::sleep(Duration::from_millis(40));
        quiet = if settled(h.state()) { quiet + 1 } else { 0 };
        if quiet >= 4 {
            return;
        }
    }
    let app = h.state();
    println!(
        "  frame not settled after {limit:?}: renderer idle {}, tiles in flight {}, \
         borders loading {}, fading {}",
        app.frame.renderer.is_idle(),
        app.tiles.cache.inflight(),
        app.borders.loader.is_loading(),
        app.frame
            .fade_started
            .is_some_and(|started| started.elapsed() < crate::app::globe::FRAME_FADE)
    );
}

fn pick_flight_near_centre(h: &mut Harness<'static, App>) {
    let size = h.ctx.content_rect().size();
    let centre = egui::pos2(size.x * 0.5, size.y * 0.55);
    for r in (0..400).step_by(6) {
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            let p = centre + egui::vec2(a.cos(), a.sin()) * r as f32;
            let plane = h.state().live.aircraft_overlay.flight_at(p);
            if let Some(plane) = plane {
                let reply = h.state_mut().run(
                    "flight.select",
                    json!({"hex": &*plane.hex, "callsign": plane.callsign.trim()}),
                );
                println!("  selected {} ({:?})", plane.callsign.trim(), reply.is_ok());
                return;
            }
        }
    }
    println!("  no aircraft near the centre");
}

fn env_number<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Puts the camera and the clock where frame `i` of the shot has them.
fn place(h: &mut Harness<'static, App>, shot: &Shot, i: usize, frames: usize, ground: &[f64]) {
    let mut k = camera_at(shot, frame_t(i, frames));
    let app = h.state_mut();
    match shot.clock {
        Clock::At(unix) => {
            app.run("time.set", json!({"time": unix})).expect("time");
        }
        Clock::Lapse(unix, minutes) => {
            let time = unix + (i as f64 * minutes).round() * 60.0;
            app.run("time.set", json!({"time": time})).expect("time");
        }
        Clock::Curve(points) => {
            let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
            let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
            let time = pchip(&xs, &ys, frame_t(i, frames));
            app.run("time.set", json!({"time": time})).expect("time");
        }
        Clock::Live => {}
    }
    if shot.from_midnight {
        let midnight = globecraft_core::math::subsolar(app.scene_unix()).1 + 180.0;
        k.lon = globecraft_core::math::wrap_lon(k.lon + midnight);
    }
    app.run(
        "camera.set",
        json!({"lat": k.lat, "lon": k.lon, "altitude_km": k.alt_km, "heading_deg": k.heading}),
    )
    .expect("camera");
    // Drawn with the exaggeration of this distance, as the app follows the ground.
    app.motion.ground_km =
        ground.get(i).copied().unwrap_or(0.0) * f64::from(app.relief_exaggeration());
    app.cam.set_tilt(k.tilt);
    app.cam.look_up = k.look;
    app.cam.clamp_tilt();
}

#[test]
#[ignore = "manual: needs network; renders the demo video's frames"]
fn demo_tour() {
    let dir = std::path::PathBuf::from(std::env::var("TOUR_DIR").unwrap_or("target/tour".into()));
    let pick: Option<Vec<String>> = std::env::var("TOUR_SHOTS")
        .ok()
        .map(|v| v.split(',').map(str::to_owned).collect());
    let stride: usize = env_number("TOUR_STRIDE", 1);
    let from: usize = env_number("TOUR_FROM", 0);
    let frame_limit = Duration::from_secs(env_number("TOUR_FRAME_LIMIT", 90));
    let mut settings = Settings::default();
    settings.appearance.theme = ThemePreference::Dark;
    // Relief appears whole instead of growing in over several frames.
    settings.smooth_relief = false;
    // The globe and the ribbon: the side pane closed.
    settings.workspace.right_pane = None;
    settings.workspace.ribbon_minimized = false;
    let mut h = harness_with(settings);
    settle(&mut h, 3.0);
    h.state_mut().motion.scripted = true;
    for shot in shots() {
        if pick
            .as_ref()
            .is_some_and(|p| !p.iter().any(|n| n == shot.name))
        {
            continue;
        }
        let out = dir.join(shot.name);
        std::fs::create_dir_all(&out).expect("tour folder");
        let frames = shot.frames.unwrap_or((shot.seconds * FPS).round() as usize);
        println!("{}: {frames} frames", shot.name);
        {
            let app = h.state_mut();
            for layer in RESET {
                let _ = app.run("layer.show", json!({"layer": layer, "visible": false}));
            }
            app.run(
                "layer.show",
                json!({"layer": "sunlight", "visible": shot.sunlight}),
            )
            .expect("sunlight");
            for (layer, visible) in shot.layers {
                app.run("layer.show", json!({"layer": layer, "visible": visible}))
                    .expect(layer);
            }
            app.run("layer.basemap", json!({"basemap": shot.basemap}))
                .expect("basemap");
            let ids: Vec<u64> = app.settings.user_layers.iter().map(|l| l.id).collect();
            for id in ids {
                let _ = app.run("data.remove", json!({"id": id}));
            }
            if let Some(preset) = shot.data {
                app.run("data.add", json!({"preset": preset}))
                    .expect(preset);
            }
            let _ = app.run("flight.clear", json!({}));
            app.settings.workspace.ribbon_tab = shot.tab;
            let _ = app.run("ui.timeline", json!({"on": shot.timeline}));
            if matches!(shot.clock, Clock::Live) {
                let _ = app.run("time.set", json!({"time": "now"}));
            }
        }
        // Warm up at the first frame: clouds, building fades and live feeds arrive.
        let flat = vec![0.0; frames];
        place(&mut h, &shot, from, frames, &flat);
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_secs(20) {
            h.state_mut().frame.last_interact = Instant::now() - Duration::from_secs(5);
            h.step();
            std::thread::sleep(Duration::from_millis(100));
        }
        let ground = if shot.layers.contains(&("terrain", true)) {
            ground_profile(&mut h, &shot, frames)
        } else {
            flat
        };
        {
            let workspace = &mut h.state_mut().settings.workspace;
            workspace.right_pane = None;
            if let Some(pane) = shot.pane {
                workspace.open_right(pane);
            }
        }
        let _ = h.state_mut().run("ruler.clear", json!({}));
        let _ = h.state_mut().run("satellite.clear", json!({}));
        if let Some(norad) = shot.satellite {
            show_recorded_satellites(&mut h);
            let app = h.state_mut();
            app.run("satellite.select", json!({"norad": norad}))
                .expect("satellite");
            app.settings.workspace.right_pane = None;
        }
        let mut selected = false;
        let mut paced = Instant::now();
        let started = Instant::now();
        for i in 0..frames {
            let t = frame_t(i, frames);
            if let Some(at) = shot.select_flight_at
                && !selected
                && t >= at
            {
                pick_flight_near_centre(&mut h);
                selected = true;
            }
            if i < from || i % stride != 0 {
                continue;
            }
            let path = out.join(format!("{i:05}.jpg"));
            if path.exists() && stride == 1 {
                continue;
            }
            if let Some(pace) = shot.pace {
                // The frame before took at least `pace` of real time.
                let due = paced + Duration::from_secs_f64(pace);
                while Instant::now() < due {
                    h.step();
                    std::thread::sleep(Duration::from_millis(50));
                }
                paced = Instant::now();
            }
            place(&mut h, &shot, i, frames, &ground);
            if let Some((from_split, to_split)) = shot.swipe {
                let app = h.state_mut();
                if app.wayback.compare.is_none() {
                    app.set_wayback_compare(true);
                }
                let u = ease(Ease::InOut, frame_t(i, frames));
                if let Some(compare) = app.wayback.compare.as_mut() {
                    compare.split = (from_split + (to_split - from_split) * u) as f32;
                }
                app.frame.last_state = 0;
            }
            if !shot.ruler.is_empty() {
                let app = h.state_mut();
                let _ = app.run("ruler.clear", json!({}));
                for (lat, lon) in ruler_at(shot.ruler, frame_t(i, frames)) {
                    let _ = app.run("ruler.addPoint", json!({"lat": lat, "lon": lon}));
                }
            }
            if shot.wait_clouds {
                let waited = Instant::now();
                loop {
                    h.step();
                    std::thread::sleep(Duration::from_millis(100));
                    let app = h.state();
                    let wish = (!app.time.is_live()).then(|| app.scene_unix());
                    if app.live.maps.clouds_settled(wish) && app.live.maps.clouds_drawn() {
                        break;
                    }
                    if waited.elapsed() > Duration::from_secs(120) {
                        println!("  clouds not loaded for frame {i}");
                        break;
                    }
                }
            }
            wait_for_frame(&mut h, frame_limit);
            let image = h.render().expect("render");
            let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
            let tmp = out.join(format!("{i:05}.part"));
            let file = std::fs::File::create(&tmp).expect("frame file");
            image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 92)
                .encode_image(&rgb)
                .expect("jpeg");
            std::fs::rename(&tmp, &path).expect("rename");
            if i % 15 == 0 || stride > 1 {
                let app = h.state();
                println!(
                    "  {i:05} t={t:.2} alt={:.1} tilt={:.0} look={:.0} ground={:.2} ({:.0?} in)",
                    app.cam.alt,
                    app.cam.tilt,
                    app.cam.look_up,
                    app.motion.ground_km,
                    started.elapsed()
                );
            }
        }
    }
}
