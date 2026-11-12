//! What's here?, the ruler profile and view links.
use super::*;

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
