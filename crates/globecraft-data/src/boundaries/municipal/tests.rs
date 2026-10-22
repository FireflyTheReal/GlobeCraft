use super::*;

fn view(lat: f64, lon: f64, altitude_km: f64) -> MunicipalView {
    MunicipalView {
        enabled: true,
        altitude_km,
        lat,
        lon,
    }
}

/// France west of 5°E, Germany east of it, sea south of 43°N.
fn europe(lat: f64, lon: f64) -> Option<String> {
    if lat < 43.0 {
        None
    } else if lon < 5.0 {
        Some("FRA".into())
    } else {
        Some("DEU".into())
    }
}

#[test]
fn units_follow_each_country_source() {
    let none = HashSet::new();
    assert_eq!(
        units_at("FRA", 48.85, 2.35, &none),
        Some(Unit::Cell {
            iso3: *b"FRA",
            x: 9,
            y: 195,
            size_mdeg: 250
        })
    );
    assert_eq!(
        units_at("USA", 40.7, -74.0, &none),
        Some(Unit::Cell {
            iso3: *b"USA",
            x: -74,
            y: 40,
            size_mdeg: 1000
        })
    );
    // São Paulo city lies in the state of São Paulo (IBGE 35).
    assert_eq!(
        units_at("BRA", -23.55, -46.63, &none),
        Some(Unit::BrazilState(35))
    );
    assert_eq!(
        units_at("MEX", 19.4, -99.1, &none),
        Some(Unit::Country(*b"MEX"))
    );
    let osm: HashSet<_> = [*b"MEX"].into();
    assert_eq!(
        units_at("MEX", 19.4, -99.1, &osm),
        Some(Unit::OsmCell { x: -100, y: 19 })
    );
}

#[test]
fn plan_covers_the_view_nearest_first_and_skips_the_sea() {
    let (area, units) = plan(
        view(47.0, 4.9, 150.0),
        150.0 * 0.8 / 111.32,
        &europe,
        &HashSet::new(),
    );
    assert!(area.contains(47.0, 4.9));
    assert!(!units.is_empty());
    // The unit under the centre comes first.
    assert_eq!(
        units[0],
        units_at("FRA", 47.0, 4.9, &HashSet::new()).unwrap()
    );
    // Both countries around the centre are covered, each with its own grid.
    assert!(
        units
            .iter()
            .any(|u| matches!(u, Unit::Cell { iso3, .. } if iso3 == b"DEU"))
    );
    assert!(
        units
            .iter()
            .any(|u| matches!(u, Unit::Cell { iso3, size_mdeg: 250, .. } if iso3 == b"FRA"))
    );
    // Heavy sources stay near the centre even when the view is wide.
    let (_, wide) = plan(view(47.0, 2.0, 600.0), 5.0, &europe, &HashSet::new());
    assert!(wide.iter().all(|u| match u.centre() {
        Some(c) => distance_sq((47.0, 2.0), c) <= (u.max_radius_deg() + 0.5).powi(2),
        None => true,
    }));
    // No duplicates.
    let unique: HashSet<_> = units.iter().collect();
    assert_eq!(unique.len(), units.len());
    // Open sea produces nothing to load.
    let (_, at_sea) = plan(view(30.0, -40.0, 300.0), 2.0, &europe, &HashSet::new());
    assert!(at_sea.is_empty());
}

fn line(points: &[(f64, f64)]) -> Line {
    Arc::new(points.to_vec())
}

fn label(name: &str, lat: f64, lon: f64) -> MapLabel {
    MapLabel {
        lat,
        lon,
        en: name.into(),
        pt: name.into(),
        kind: super::super::LabelKind::City,
        rank: super::super::LOCAL_LABEL_RANK,
        big: false,
    }
}

#[test]
fn assembly_clips_to_the_area_and_keeps_shared_rings_once() {
    let ring = [(48.0, 2.0), (48.0, 2.6), (48.4, 2.6), (48.0, 2.0)];
    let mut store = Store::default();
    for (x, centre) in [(4, (48.25, 2.25)), (5, (48.25, 2.75))] {
        // Both cells return the municipality on their shared edge.
        let detail = Detail {
            lines: vec![
                line(&ring),
                line(&[(48.1 + x as f64, 2.1), (48.2 + x as f64, 2.2)]),
            ],
            labels: vec![label("Shared", 48.2, 2.3), label("Far", 60.0, 30.0)],
            features: 2,
            parents: vec![],
        };
        store.units.insert(
            Unit::Cell {
                iso3: *b"FRA",
                x,
                y: 96,
                size_mdeg: 500,
            },
            UnitData::new(detail, centre),
        );
    }
    let patch = assemble(&store, GeoBox::around(48.2, 2.3, 1.0));
    assert!(patch.replace);
    assert!(patch.layer == Layer::WorldCities);
    // The shared ring once; the far-away extra lines (lat 52+) are clipped away.
    assert_eq!(patch.lines.len(), 1);
    assert_eq!(patch.lines[0].len(), 4);
    assert!(patch.labels.iter().all(|l| l.en == "Shared"));
}

#[test]
fn eviction_keeps_the_units_nearest_the_view() {
    let mut store = Store::default();
    let big = |lat: f64| Detail {
        lines: vec![line(&vec![(lat, 0.0); STORE_VERTEX_BUDGET / 2 + 1])],
        labels: vec![],
        features: 1,
        parents: vec![],
    };
    for (y, lat) in [(0, 0.0), (10, 10.0), (20, 20.0)] {
        let data = UnitData::new(big(lat), (lat, 0.0));
        store.vertices += data.vertices;
        store.units.insert(Unit::OsmCell { x: 0, y }, data);
    }
    store.evict((1.0, 0.0));
    assert!(store.vertices <= STORE_VERTEX_BUDGET);
    assert!(store.units.contains_key(&Unit::OsmCell { x: 0, y: 0 }));
    assert!(!store.units.contains_key(&Unit::OsmCell { x: 0, y: 20 }));
}

const _: () = {
    // Prefetch starts well above the visible band, and unloading above it.
    assert!(MUNICIPAL_PREFETCH_ALTITUDE_KM > MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM * 1.5);
    assert!(UNLOAD_ALTITUDE_KM > MUNICIPAL_PREFETCH_ALTITUDE_KM);
};

#[tokio::test]
async fn leaving_the_altitude_band_publishes_an_empty_layer() {
    let (tx, rx) = mpsc::channel();
    let mut streamer = MunicipalStreamer::new(
        tokio::runtime::Handle::current(),
        tx,
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(|| {}),
    );
    // Over the sea nothing loads, but the area becomes active.
    streamer.update(view(30.0, -40.0, 300.0), |_, _| None);
    assert!(streamer.store.lock().unwrap().active.is_some());
    streamer.update(view(30.0, -40.0, 5_000.0), |_, _| None);
    assert!(streamer.store.lock().unwrap().active.is_none());
    let patch = tokio::task::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a published layer")
    })
    .await
    .unwrap();
    assert!(patch.layer == Layer::WorldCities && patch.lines.is_empty());
}
