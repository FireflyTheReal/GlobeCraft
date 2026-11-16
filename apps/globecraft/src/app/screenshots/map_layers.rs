//! Map layers close up: map data, roads, places of interest, names, time zones, the poles, aurora.
use super::*;

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
        let drawn: Vec<_> = h.state().map_data.poi_shapes.drawn().to_vec();
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
                            .map_data
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
    app.live.aurora.set_grid(grid);
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
    app.live
        .airports
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
    let geometry = super::super::live_layers::river_scene(rect);
    let mut shapes = super::super::live_layers::PoiShapes::default();
    let mut fonts_set = false;
    let mut h = Harness::builder()
        .with_size(rect.size())
        .with_pixels_per_point(2.0)
        .renderer(WgpuTestRenderer::new())
        .build_ui(move |ui| {
            if !fonts_set {
                ui.ctx().set_fonts(super::super::fonts::base_definitions());
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
    app.live.time_zones.set(zones);
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
    let cursor = h.state().pointer.cursor.expect("pointer over the globe");
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
        app.settings.workspace.ribbon_tab = Tab::Map;
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
        app.live.time_zones.set(zones);
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

/// Buildings as blocks over a city, straight down and tilted, by day with the sun and
/// without it: for judging their look by eye (roadmap 8.6). Manual.
/// `GLOBECRAFT_SHOT_LAT` / `_LON` move it elsewhere (default: Munich's old town).
#[test]
#[ignore = "manual: needs network; buildings over a city"]
fn buildings_screenshots() {
    use globecraft_core::layers::MapLayer;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let env_f64 = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f64>().ok());
    let lat = env_f64("GLOBECRAFT_SHOT_LAT").unwrap_or(48.1374);
    let lon = env_f64("GLOBECRAFT_SHOT_LON").unwrap_or(11.5755);
    let mut h = harness(ThemePreference::Dark);
    for layer in [MapLayer::Buildings, MapLayer::Roads] {
        h.state_mut().settings.set_layer_visible(layer, true);
    }
    // (name, altitude km, tilt, heading, sunlight)
    for (name, altitude_km, tilt_deg, heading_deg, sunlight) in [
        ("12-buildings-top", 1.2, 0.0, 0.0, false),
        ("12-buildings-tilt", 0.8, 60.0, 20.0, false),
        ("12-buildings-tilt-low", 0.35, 72.0, 200.0, false),
        ("12-buildings-sun", 0.8, 60.0, 20.0, true),
    ] {
        h.state_mut()
            .settings
            .set_layer_visible(MapLayer::Sunlight, sunlight);
        let _ = h.state_mut().run(
            "camera.set",
            json!({"lat": lat, "lon": lon, "altitude_km": altitude_km,
                "tilt_deg": tilt_deg, "heading_deg": heading_deg}),
        );
        settle(&mut h, 25.0);
        save(&mut h, &dir, name);
        // What a camera move costs: the blocks projected again for this view.
        let app = h.state();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let input = app.live_layer_input(app.view_basis(1.6), rect, app.cam, 2.0);
        let blocks = input.buildings.as_ref().map_or(0, |blocks| blocks.len());
        let uncoloured = globecraft_render::live_layers::LiveInput {
            imagery: None,
            ..input.clone()
        };
        for (label, input) in [("imagery colours", input), ("default colours", uncoloured)] {
            let started = std::time::Instant::now();
            let geometry = globecraft_render::live_layers::LivePreparer::with_elevation(
                app.tiles.cache.clone(),
            )
            .prepare(input);
            println!(
                "{name}: {blocks} blocks, {} triangles, projected in {:?} ({label})",
                geometry.buildings.indices.len() / 3,
                started.elapsed()
            );
        }
    }
}
