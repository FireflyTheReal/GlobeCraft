use super::*;

const NO_REFERENCE: ReferenceLines<'static> = ReferenceLines {
    states: &[],
    states_big: &[],
    countries: &[],
};

fn view(lat: f64, lon: f64, altitude_km: f64) -> MunicipalView<'static> {
    MunicipalView {
        enabled: true,
        altitude_km,
        lat,
        lon,
        countries: &[],
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

#[test]
fn only_the_selected_countries_load() {
    let germany = ["DEU".to_owned()];
    let only_germany = MunicipalView {
        countries: &germany,
        ..view(47.0, 4.9, 150.0)
    };
    let (_, units) = plan(only_germany, 150.0 * 0.8 / 111.32, &europe, &HashSet::new());
    assert!(!units.is_empty());
    assert!(
        units
            .iter()
            .all(|u| matches!(u, Unit::Cell { iso3, .. } if iso3 == b"DEU"))
    );
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
            UnitData::new(detail, centre, Some(*b"FRA"), &[], None).0,
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
        let data = UnitData::new(big(lat), (lat, 0.0), None, &[], None).0;
        store.vertices += data.vertices;
        store.units.insert(Unit::OsmCell { x: 0, y }, data);
    }
    store.evict((1.0, 0.0));
    assert!(store.vertices <= STORE_VERTEX_BUDGET);
    assert!(store.units.contains_key(&Unit::OsmCell { x: 0, y: 0 }));
    assert!(!store.units.contains_key(&Unit::OsmCell { x: 0, y: 20 }));
}

#[test]
fn borders_between_units_are_classified_when_the_second_unit_loads() {
    // Two Brazilian states, one municipality each, sharing the edge at lon -45.
    let west = line(&[
        (-20.0, -45.5),
        (-20.0, -45.0),
        (-19.5, -45.0),
        (-19.5, -45.5),
        (-20.0, -45.5),
    ]);
    let east = line(&[
        (-20.0, -45.0),
        (-20.0, -44.5),
        (-19.5, -44.5),
        (-19.5, -45.0),
        (-20.0, -45.0),
    ]);
    let detail = |ring: Line, state: u64| Detail {
        lines: vec![ring],
        labels: vec![],
        features: 1,
        parents: vec![Some(state)],
    };
    let mut store = Store::default();
    let (first, _) = UnitData::new(detail(west, 31), (-19.75, -45.25), Some(*b"BRA"), &[], None);
    store.units.insert(Unit::BrazilState(31), first);
    // Alone, the state's edge has no neighbour: nothing is a state border yet.
    let alone = assemble(&store, GeoBox::around(-19.75, -45.0, 2.0));
    assert!(alone.lines_big.is_empty());

    let area = GeoBounds::of(&east).unwrap();
    let neighbours = store.neighbours(Unit::BrazilState(35), area.expanded(0.02));
    assert_eq!(neighbours.len(), 1);
    let (second, upgrades) = UnitData::new(
        detail(east, 35),
        (-19.75, -44.75),
        Some(*b"BRA"),
        &neighbours,
        None,
    );
    store.units.insert(Unit::BrazilState(35), second);
    store.apply(upgrades);
    let patch = assemble(&store, GeoBox::around(-19.75, -45.0, 2.0));
    // The shared edge is now a state border, drawn once.
    assert_eq!(patch.lines_big.len(), 1, "{:?}", patch.lines_big);
    assert!(patch.lines_big[0].iter().all(|p| (p.1 + 45.0).abs() < 1e-9));
    let staging = patch.municipal.unwrap();
    assert!(staging.major_cells.covers(-19.75, -45.0));
    assert!(!staging.major_cells.covers(-19.75, -44.6));
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
    streamer.update(view(30.0, -40.0, 300.0), NO_REFERENCE, |_, _| None);
    assert!(streamer.store.lock().unwrap().active.is_some());
    streamer.update(view(30.0, -40.0, 5_000.0), NO_REFERENCE, |_, _| None);
    assert!(streamer.store.lock().unwrap().active.is_none());
    let patch = tokio::task::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a published layer")
    })
    .await
    .unwrap();
    assert!(patch.layer == Layer::WorldCities && patch.lines.is_empty());
}

#[test]
fn borders_between_two_countries_units_become_country_borders() {
    // A French and a German cell, one commune each, sharing the edge at 8°E.
    let west = line(&[
        (48.0, 7.5),
        (48.0, 8.0),
        (48.5, 8.0),
        (48.5, 7.5),
        (48.0, 7.5),
    ]);
    let east = line(&[
        (48.0, 8.0),
        (48.0, 8.5),
        (48.5, 8.5),
        (48.5, 8.0),
        (48.0, 8.0),
    ]);
    let detail = |ring: Line, state: u64| Detail {
        lines: vec![ring],
        labels: vec![],
        features: 1,
        parents: vec![Some(state)],
    };
    let mut store = Store::default();
    let (first, _) = UnitData::new(detail(west, 67), (48.25, 7.75), Some(*b"FRA"), &[], None);
    store.units.insert(Unit::Country(*b"FRA"), first);
    let area = GeoBounds::of(&east).unwrap();
    let neighbours = store.neighbours(Unit::Country(*b"DEU"), area.expanded(0.02));
    let (second, upgrades) = UnitData::new(
        detail(east, 8),
        (48.25, 8.25),
        Some(*b"DEU"),
        &neighbours,
        None,
    );
    store.units.insert(Unit::Country(*b"DEU"), second);
    store.apply(upgrades);
    let patch = assemble(&store, GeoBox::around(48.25, 8.0, 2.0));
    let staging = patch.municipal.unwrap();
    // The shared edge is a country border, drawn once, and not a state border.
    assert_eq!(
        staging.country_lines.len(),
        1,
        "{:?}",
        staging.country_lines
    );
    assert!(
        staging.country_lines[0]
            .iter()
            .all(|p| (p.1 - 8.0).abs() < 1e-9)
    );
    assert!(patch.lines_big.is_empty());
    assert!(staging.country_cells.covers(48.25, 8.0));
    // State lines give way along country borders as well.
    assert!(staging.major_cells.covers(48.25, 8.0));
    assert!(!staging.country_cells.covers(48.25, 7.6));
}

#[test]
fn sources_without_parents_trace_the_natural_earth_lines() {
    // Two geoBoundaries municipalities with no state named, sharing the edge at
    // lon 10.0, which a Natural Earth state line follows 1 km off. The western one's
    // left side runs along a Natural Earth country line (its neighbour is not loaded).
    let west = line(&[
        (45.0, 9.5),
        (45.0, 10.0),
        (45.5, 10.0),
        (45.5, 9.5),
        (45.0, 9.5),
    ]);
    let east = line(&[
        (45.0, 10.0),
        (45.0, 10.5),
        (45.5, 10.5),
        (45.5, 10.0),
        (45.0, 10.0),
    ]);
    let state: Line = Arc::new(vec![(44.8, 10.01), (45.7, 10.01)]);
    let country: Line = Arc::new(vec![(44.8, 9.5), (45.7, 9.5)]);
    let reference = (
        Arc::new(reference::Reference::build(&[state], &[country])),
        1,
    );
    let detail = Detail {
        lines: vec![west, east],
        labels: vec![],
        features: 2,
        parents: vec![],
    };
    let mut store = Store {
        reference: Some(reference.clone()),
        ..Store::default()
    };
    let (data, _) = UnitData::new(detail, (45.25, 10.0), Some(*b"ITA"), &[], Some(&reference));
    store.units.insert(Unit::Country(*b"ITA"), data);
    let patch = assemble(&store, GeoBox::around(45.25, 10.0, 2.0));
    // The shared edge is drawn once, as a state border; the west edge as a country
    // border; the rest stays interior.
    assert_eq!(patch.lines_big.len(), 1, "{:?}", patch.lines_big);
    assert!(patch.lines_big[0].iter().all(|p| (p.1 - 10.0).abs() < 1e-9));
    let staging = patch.municipal.unwrap();
    assert_eq!(staging.country_lines.len(), 1);
    assert!(
        staging.country_lines[0]
            .iter()
            .all(|p| (p.1 - 9.5).abs() < 1e-9)
    );
    assert!(!patch.lines.is_empty());
    // Without a traced reference revision, nothing is upgraded.
    store.reference = Some((reference.0, 2));
    let untraced = assemble(&store, GeoBox::around(45.25, 10.0, 2.0));
    assert!(untraced.lines_big.is_empty());
    assert_eq!(store.untraced().len(), 2);
}
