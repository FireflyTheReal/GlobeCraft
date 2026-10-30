use super::*;
use globecraft_core::{camera::Cam, earth_model::EarthModel};
use std::{sync::mpsc, time::Duration};

pub(super) fn input(model: EarthModel) -> LiveInput {
    let camera = Cam {
        lat: 0.0,
        lon: 0.0,
        heading: 0.0,
        alt: 100.0,
        tilt: 0.0,
    };
    LiveInput {
        view: LiveView {
            basis: camera.basis_for_model(1.0, model),
            rect: Rect::from_min_size(Pos2::new(20.0, 30.0), Vec2::splat(600.0)),
            altitude_km: camera.alt,
            map_scale: 1.0,
            pixels_per_point: 1.0,
            traffic_width_px: 16,
            traffic_height_px: 16,
            ground: None,
        },
        roads: Some(
            vec![Road {
                name: "Road".into(),
                kind: "primary".into(),
                restricted: false,
                points: vec![
                    (0.0, -0.1),
                    (0.0, 0.0),
                    (0.0, 180.0),
                    (0.0, 0.0),
                    (0.0, 0.1),
                ],
            }]
            .into(),
        ),
        vessels: Some(
            vec![Vessel {
                mmsi: "123456789".into(),
                name: "Ship".into(),
                lat: 0.0,
                lon: 0.0,
                course: 45.0,
                speed_knots: Some(8.0),
                ship_type: None,
                destination: None,
                draught_m: None,
                length_m: None,
                beam_m: None,
                observed_unix: 1_700_000_000.0,
            }]
            .into(),
        ),
        satellites: None,
        traffic: None,
        transport: None,
        transport_layers: Default::default(),
        poi: None,
        poi_layers: Default::default(),
        unix_secs: 1_700_000_000.0,
    }
}
struct Relief;
impl Ground for Relief {
    fn height(&self, _lat: f64, _lon: f64) -> f64 {
        // World units, as `GroundHeights` returns: a few kilometres, exaggerated.
        0.002
    }
}

/// A nine-kilometre ridge across the view near the equator, in world units.
struct Ridge;
impl Ground for Ridge {
    fn height(&self, lat: f64, _lon: f64) -> f64 {
        if (0.2..0.3).contains(&lat) {
            9.0 / globecraft_core::EARTH_RADIUS_KM
        } else {
            0.0
        }
    }
}

#[test]
fn terrain_occlusion_drops_a_road_hidden_behind_a_ridge() {
    let mut input = input(EarthModel::Wgs84);
    input.view.basis = Cam {
        lat: 0.0,
        lon: 0.0,
        heading: 0.0,
        alt: 30.0,
        tilt: 65.0,
    }
    .basis(1.0);
    input.view.altitude_km = 30.0;
    input.roads = Some(
        vec![Road {
            name: "Hidden road".into(),
            kind: "primary".into(),
            restricted: false,
            points: vec![(0.35, 0.0), (0.4, 0.0), (0.45, 0.0)],
        }]
        .into(),
    );
    let open = project_roads(&input, None);
    assert!(
        open.iter().any(|road| !road.segments.is_empty()),
        "without relief the road is drawn"
    );
    let occluded = project_roads(&input, Some(&Ridge));
    assert!(
        occluded.iter().all(|road| road.segments.is_empty()),
        "the ridge must hide the whole road"
    );
}

#[test]
fn live_lines_are_raised_onto_the_relief() {
    let mut input = input(EarthModel::Wgs84);
    input.view.basis = Cam {
        lat: 0.0,
        lon: 0.0,
        heading: 0.0,
        alt: 100.0,
        tilt: 60.0,
    }
    .basis(1.0);
    input.view.altitude_km = 100.0;
    let flat = project_roads(&input, None);
    let raised = project_roads(&input, Some(&Relief));
    assert_ne!(
        flat[0].segments[0][0], raised[0].segments[0][0],
        "a road on a mountain must be projected at the raised position"
    );
}

#[test]
fn roads_vessels_and_hover_match_the_selected_surface_and_keep_all_visible_fragments() {
    for model in EarthModel::ALL {
        let input = input(model);
        let geometry = LivePreparer::default().prepare(input.clone());
        let expected = input
            .view
            .basis
            .project(input.view.basis.surface_position(0.0, 0.0), input.view.rect)
            .unwrap();
        assert_eq!(geometry.vessels[0].position, expected);
        assert!(
            geometry.vessels[0]
                .icon
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite())
        );
        assert_eq!(geometry.roads[0].segments.len(), 2);
        let point = input
            .view
            .basis
            .project(
                input.view.basis.surface_position(0.0, -0.075),
                input.view.rect,
            )
            .unwrap();
        let (source, nearest) = geometry.nearest_road(point, 1.0).unwrap();
        assert_eq!(source, 0);
        assert!((nearest - point).length() < 0.01);
        assert!(
            geometry
                .nearest_road(Pos2::new(-1000.0, -1000.0), 1.0)
                .is_none()
        );
    }
}
#[test]
fn preparer_reuses_components_and_invalidates_changes_to_view_data_and_time() {
    let mut request = input(EarthModel::Wgs84);
    request.traffic = Some(Arc::new(TrafficTiles {
        image: image::RgbaImage::from_pixel(768, 768, image::Rgba([0, 200, 0, 255])),
        zoom: 2,
        left_x: 1,
        top_y: 1,
        loaded: std::time::Instant::now(),
    }));
    // Without moving objects, time alone changes nothing.
    let vessels = request.vessels.take();
    let mut preparer = LivePreparer::default();
    let first = preparer.prepare(request.clone());
    request.unix_secs += 1.0;
    assert!(Arc::ptr_eq(&first, &preparer.prepare(request.clone())));
    request.vessels = Some(Vec::<Vessel>::new().into());
    let second = preparer.prepare(request.clone());
    // Vessels are dead-reckoned, so a new motion step moves them, and only them.
    let mut moving = request.clone();
    moving.vessels = vessels.clone();
    let before = preparer.prepare(moving.clone());
    moving.unix_secs += moving.view.vessel_step();
    let after = preparer.prepare(moving);
    assert!(!Arc::ptr_eq(&before.vessels, &after.vessels));
    assert!(Arc::ptr_eq(&before.roads, &after.roads));
    preparer.prepare(request.clone());
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(&first.roads, &second.roads));
    assert!(Arc::ptr_eq(
        first.traffic.as_ref().unwrap(),
        second.traffic.as_ref().unwrap()
    ));
    assert!(!Arc::ptr_eq(&first.vessels, &second.vessels));
    request.view.rect = request.view.rect.translate(Vec2::new(10.0, 20.0));
    let third = preparer.prepare(request);
    assert!(!Arc::ptr_eq(&second.roads, &third.roads));
    assert!(!Arc::ptr_eq(
        second.traffic.as_ref().unwrap(),
        third.traffic.as_ref().unwrap()
    ));
    assert!(
        third
            .traffic
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 229)
    );
}
#[test]
fn orbital_updates_reuse_roads_and_project_only_current_valid_subpoints() {
    let mut request = input(EarthModel::Wgs84);
    let orbit = globecraft_core::orbit::OrbitalElements {
        inclination_deg: 0.0,
        raan_deg: 0.0,
        eccentricity: 0.001,
        argument_perigee_deg: 0.0,
        mean_anomaly_deg: 0.0,
        mean_motion_rev_per_day: 15.0,
        epoch_unix_secs: request.unix_secs,
    };
    let (lat, lon) = orbit.ground_position(request.unix_secs).unwrap();
    request.view.basis = Cam {
        lat,
        lon,
        alt: 1000.0,
        heading: 0.0,
        tilt: 0.0,
    }
    .basis_for_model(1.0, EarthModel::Wgs84);
    request.satellites = Some(
        vec![Satellite {
            name: "Orbit".into(),
            norad: "1".into(),
            orbit,
        }]
        .into(),
    );
    let mut preparer = LivePreparer::default();
    let first = preparer.prepare(request.clone());
    assert_eq!(first.satellites.len(), 1);
    request.unix_secs += 1.0;
    let second = preparer.prepare(request);
    assert!(Arc::ptr_eq(&first.roads, &second.roads));
    assert!(!Arc::ptr_eq(&first.satellites, &second.satellites));
    assert_ne!(first.satellites[0].position, second.satellites[0].position);
}
#[test]
fn worker_publishes_before_waking_and_reuses_an_unchanged_request() {
    let (send, receive) = mpsc::channel();
    let worker = LiveWorker::new(move || {
        let _ = send.send(());
    })
    .unwrap();
    let input = input(EarthModel::PerfectSphere);
    worker.submit(input.clone());
    receive.recv_timeout(Duration::from_secs(10)).unwrap();
    let first = worker.take_completed().unwrap();
    assert!(first.input.same_request(&input));
    worker.submit(input);
    receive.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(Arc::ptr_eq(&first, &worker.take_completed().unwrap()));
}
#[test]
fn vessels_sail_their_course_for_a_while_after_a_report() {
    let ship = |speed_knots| Vessel {
        mmsi: "1".into(),
        name: String::new(),
        lat: 0.0,
        lon: 0.0,
        course: 90.0,
        speed_knots,
        ship_type: None,
        destination: None,
        draught_m: None,
        length_m: None,
        beam_m: None,
        observed_unix: 0.0,
    };
    let km_east = |(lat, lon): (f64, f64)| {
        assert!(lat.abs() < 1e-9);
        lon.to_radians() * globecraft_core::EARTH_RADIUS_KM
    };
    // 10 knots for six minutes: one nautical mile.
    let moved = km_east(super::dead_reckon(&ship(Some(10.0)), 360.0));
    assert!((moved - 1.852).abs() < 0.01, "{moved}");
    // The reckoning stops after ten minutes.
    let capped = km_east(super::dead_reckon(&ship(Some(10.0)), 7200.0));
    assert!((capped - 3.087).abs() < 0.01, "{capped}");
    // Moored ships and missing speeds stay put.
    assert_eq!(super::dead_reckon(&ship(Some(0.2)), 360.0), (0.0, 0.0));
    assert_eq!(super::dead_reckon(&ship(None), 360.0), (0.0, 0.0));
}

#[test]
fn transport_lines_and_airports_follow_their_layers() {
    use globecraft_data::transport::{
        Airport, LineKind, TransportLine, TransportSnapshot, TransportTile,
    };
    let mut request = input(EarthModel::Wgs84);
    request.view.altitude_km = 30.0;
    let line = |kind| TransportLine {
        kind,
        tunnel: false,
        points: vec![(0.0, -0.5), (0.0, 0.5)],
    };
    request.transport = Some(Arc::new(TransportSnapshot {
        tiles: vec![Arc::new(TransportTile {
            lines: vec![
                line(LineKind::Rail),
                line(LineKind::Ferry),
                line(LineKind::Runway),
            ],
            airports: vec![Airport {
                lat: 0.1,
                lon: 0.1,
                name: "Equator International".into(),
                iata: Some("EQI".into()),
                icao: None,
                class: "international".into(),
            }],
        })],
        zoom: 12,
    }));
    let kinds = |layers| {
        let mut request = request.clone();
        request.transport_layers = layers;
        let geometry = LivePreparer::default().prepare(request);
        let kinds: Vec<_> = geometry
            .transport
            .lines
            .iter()
            .map(|line| line.kind)
            .collect();
        (kinds, geometry.transport.airports.len())
    };
    let all = TransportLayers {
        railways: true,
        ferries: true,
        airports: true,
    };
    assert_eq!(
        kinds(all),
        (vec![LineKind::Ferry, LineKind::Runway, LineKind::Rail], 1)
    );
    assert_eq!(
        kinds(TransportLayers {
            railways: true,
            ..Default::default()
        }),
        (vec![LineKind::Rail], 0)
    );
    assert_eq!(kinds(TransportLayers::default()), (vec![], 0));
}

#[test]
fn poi_names_follow_their_layers_and_altitude() {
    use globecraft_data::poi::{PoiLine, PoiLineKind, PoiPoint, PoiSnapshot, PoiTile, PointKind};
    let mut request = input(EarthModel::Wgs84);
    let point = |kind, name: &str, lon| PoiPoint {
        kind,
        lat: 0.0,
        lon,
        name: name.into(),
        class: String::new(),
        rank: 1,
        elevation_m: None,
    };
    request.poi = Some(Arc::new(PoiSnapshot {
        tiles: vec![Arc::new(PoiTile {
            points: vec![
                point(PointKind::Poi, "Museum", -0.2),
                point(PointKind::Peak, "Peak", 0.0),
                point(PointKind::Water, "Lake", 0.2),
            ],
            lines: vec![PoiLine {
                kind: PoiLineKind::River,
                name: None,
                points: vec![(0.1, -0.5), (0.1, 0.5)],
            }],
        })],
        zoom: 12,
    }));
    let shown = |altitude_km, layers| {
        let mut request = request.clone();
        request.view.altitude_km = altitude_km;
        request.poi_layers = layers;
        let geometry = LivePreparer::default().prepare(request);
        (geometry.poi.labels.len(), geometry.poi.lines.len())
    };
    let all = PoiLayers {
        places: true,
        peaks: true,
        waters: true,
        parks: true,
    };
    // Close up everything shows; from 100 km the places of interest are gone.
    assert_eq!(shown(10.0, all), (3, 1));
    assert_eq!(shown(100.0, all), (2, 1));
    assert_eq!(
        shown(
            10.0,
            PoiLayers {
                peaks: true,
                ..Default::default()
            }
        ),
        (1, 0)
    );
}
