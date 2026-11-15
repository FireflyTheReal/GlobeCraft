//! Live flights and symbols against a local fake server.
use super::*;

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
    app.picked.flight.set(SelectedFlight {
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
    app.live.aircraft = LiveAircraft::with_sources(
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
        h.state_mut().live.satellites.show_recorded(&answers);
    }
    for _ in 0..60 {
        if !h.state().live.satellites.snapshot().is_empty() {
            break;
        }
        settle(&mut h, 1.0);
    }
    let satellites = h.state().live.satellites.snapshot();
    assert!(
        !satellites.is_empty(),
        "{}",
        h.state().live.satellites.status(h.state().lang)
    );
    println!(
        "{} satellites, {} with SGP4",
        satellites.len(),
        satellites.iter().filter(|s| s.sgp4.is_some()).count()
    );
    // The ISS selected: one orbit at its height, from far away and from its side.
    let iss = h
        .state()
        .live
        .satellites
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
            .live
            .satellites
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
