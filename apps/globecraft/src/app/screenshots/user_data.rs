//! Added data: the Add Data window, point layers, archives, legends and the timeline.
use super::*;

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
    h.state_mut().windows.add_data = None;
    settle(&mut h, 1.0);
    save(&mut h, &dir, "09-user-layer");
    // The Data pane, with the sample starred and the earthquakes in the history.
    let app = h.state_mut();
    let sample = app.settings.user_layers[0].clone();
    app.settings.saved_sources.toggle_favorite(&sample);
    let _ = app.run("data.add", json!({"preset": "earthquakes"}));
    settle(&mut h, 1.0);
    // The Layers ribbon at both experience levels: Basic leaves out the Earth models.
    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Map;
    let _ = app.run("settings.experience", json!({"level": "basic"}));
    settle(&mut h, 1.0);
    save(&mut h, &dir, "11-layers-ribbon-basic");
    let _ = h
        .state_mut()
        .run("settings.experience", json!({"level": "advanced"}));
    settle(&mut h, 1.0);
    save(&mut h, &dir, "12-layers-ribbon-advanced");
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
/// (`ocean-presets`); `GLOBECRAFT_SHOT_PRESETS` (comma-separated codes) shows others.
/// Needs network; manual.
#[test]
#[ignore = "manual: needs network; renders the Marine Regions presets"]
fn ocean_presets_screenshot() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    let presets = std::env::var("GLOBECRAFT_SHOT_PRESETS")
        .unwrap_or_else(|_| "eez_boundaries,iho_sea_areas".into());
    for preset in presets.split(',').filter(|code| !code.is_empty()) {
        h.state_mut()
            .run("data.add", json!({"preset": preset}))
            .expect("preset");
    }
    // GLOBECRAFT_MARINE_NAMES_FILE: a names file to use instead of the published one.
    if let Some(path) = std::env::var_os("GLOBECRAFT_MARINE_NAMES_FILE") {
        let bytes = std::fs::read(path).expect("names file");
        let names = globecraft_data::marine_names::MarineNames::from_gzip(&bytes).expect("names");
        h.state_mut().live.marine_names.set(names);
    }
    // GLOBECRAFT_SHOT_LAYERS: built-in layers to turn on too (codes, comma-separated).
    for code in std::env::var("GLOBECRAFT_SHOT_LAYERS")
        .unwrap_or_default()
        .split(',')
    {
        if let Some(layer) = globecraft_core::layers::MapLayer::from_code(code.trim()) {
            h.state_mut().settings.set_layer_visible(layer, true);
        }
    }
    // GLOBECRAFT_SHOT_VIEWS: `lat,lon,alt;…` (default: the North Atlantic).
    let views = std::env::var("GLOBECRAFT_SHOT_VIEWS").unwrap_or_else(|_| "50,-10,4500".into());
    for (index, view) in views.split(';').enumerate() {
        let v: Vec<f64> = view
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        let _ = h.state_mut().run(
            "camera.set",
            json!({"lat": v[0], "lon": v[1], "altitude_km": v[2]}),
        );
        settle(&mut h, 12.0);
        let name = if index == 0 {
            "ocean-presets".to_owned()
        } else {
            format!("ocean-presets-{index}")
        };
        save(&mut h, &dir, &name);
    }
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
    app.picked
        .flight
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
