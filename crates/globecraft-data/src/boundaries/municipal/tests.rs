use super::*;

const NO_BORDERS: CountryBorderLines<'static> = CountryBorderLines {
    lines: &[],
    pairs: &[],
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
            exact_topology: false,
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
            UnitData::new(detail, centre, Some(*b"FRA"), &[]).0,
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
        exact_topology: false,
        features: 1,
        parents: vec![],
    };
    for (y, lat) in [(0, 0.0), (10, 10.0), (20, 20.0)] {
        let data = UnitData::new(big(lat), (lat, 0.0), None, &[]).0;
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
        exact_topology: false,
        features: 1,
        parents: vec![Some(state)],
    };
    let mut store = Store::default();
    let (first, _) = UnitData::new(detail(west, 31), (-19.75, -45.25), Some(*b"BRA"), &[]);
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
    streamer.update(view(30.0, -40.0, 300.0), NO_BORDERS, |_, _| None);
    assert!(streamer.store.lock().unwrap().active.is_some());
    streamer.update(view(30.0, -40.0, 5_000.0), NO_BORDERS, |_, _| None);
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
fn one_country_draws_a_shared_border_once_the_land_borders_are_indexed() {
    // A French and a German municipality sharing the edge at 8°E, which Natural
    // Earth's French–German border line follows.
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
        exact_topology: false,
        features: 1,
        parents: vec![Some(state)],
    };
    let mut store = Store::default();
    let (first, _) = UnitData::new(detail(west, 67), (48.25, 7.75), Some(*b"FRA"), &[]);
    store.units.insert(Unit::Country(*b"FRA"), first);
    let area = GeoBounds::of(&east).unwrap();
    let neighbours = store.neighbours(Unit::Country(*b"DEU"), area.expanded(0.02));
    let (second, upgrades) =
        UnitData::new(detail(east, 8), (48.25, 8.25), Some(*b"DEU"), &neighbours);
    store.units.insert(Unit::Country(*b"DEU"), second);
    store.apply(upgrades);
    let on_edge = |p: &(f64, f64)| (p.1 - 8.0).abs() < 1e-9;
    let count = |lines: &[Line]| {
        lines
            .iter()
            .flat_map(|line| line.windows(2))
            .filter(|pair| on_edge(&pair[0]) && on_edge(&pair[1]))
            .count()
    };
    // Before the land borders are indexed: both copies are ordinary municipal lines,
    // and no country line replaces Natural Earth's.
    let patch = assemble(&store, GeoBox::around(48.25, 8.0, 2.0));
    let staging = patch.municipal.unwrap();
    assert!(staging.country_lines.is_empty());
    assert!(patch.lines_big.is_empty(), "never a state border");
    assert_eq!(count(&patch.lines), 2);
    // Indexed: one side draws the border as a country line, the other copy is hidden.
    let natural_earth = line(&[(47.9, 8.01), (48.6, 8.01)]);
    store.borders = Some(Arc::new(borders::CountryBorders::new(vec![(
        natural_earth,
        [*b"FRA", *b"DEU"],
    )])));
    let patch = assemble(&store, GeoBox::around(48.25, 8.0, 2.0));
    let staging = patch.municipal.unwrap();
    assert_eq!(
        count(&staging.country_lines),
        1,
        "{:?}",
        staging.country_lines
    );
    assert_eq!(count(&patch.lines), 0, "the other copy is hidden");
    assert!(staging.country_cells.covers(48.25, 8.01));
}

/// Real data, outside the repository: set `GLOBECRAFT_MUNICIPAL_CHECK` to a folder
/// holding geoBoundaries simplified files named `{ISO}-{LEVEL}.geojson` and the Natural
/// Earth `ne_10m_admin_1_states_provinces_lines.geojson` and
/// `ne_10m_admin_0_boundary_lines_land.geojson`, and `GLOBECRAFT_MUNICIPAL_COUNTRIES`
/// to a list such as `CZE,POL,AUT`. Writes `check.json` (what each layer draws in the
/// area `GLOBECRAFT_MUNICIPAL_AREA` = `lat,lon,radius_deg`) for plotting.
#[test]
#[ignore = "manual check on real data; run with --release --ignored --nocapture"]
fn real_countries_classify_their_borders() {
    let Ok(dir) = std::env::var("GLOBECRAFT_MUNICIPAL_CHECK") else {
        return;
    };
    let countries = std::env::var("GLOBECRAFT_MUNICIPAL_COUNTRIES").unwrap_or_default();
    let area: Vec<f64> = std::env::var("GLOBECRAFT_MUNICIPAL_AREA")
        .unwrap_or_else(|_| "50.0,15.0,3.0".into())
        .split(',')
        .map(|v| v.parse().unwrap())
        .collect();
    let features = |name: &str| -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["features"].as_array().unwrap().clone()
    };
    let mut store = Store::default();
    for iso in countries.split(',').filter(|c| !c.is_empty()) {
        let started = std::time::Instant::now();
        let level = adm::level(iso).unwrap();
        let mut file = adm::MunicipalFile::default();
        for feature in features(&format!("{iso}-{level}.geojson")) {
            adm::read_municipality(&feature, &mut file);
        }
        let states = adm::state_level(iso).map(|state_level| {
            let mut rings = Vec::new();
            for feature in features(&format!("{iso}-{state_level}.geojson")) {
                adm::read_state(&feature, &mut rings);
            }
            super::super::parents::States::new(rings)
        });
        let detail = adm::with_parents(file, states.as_ref());
        if std::env::var("GLOBECRAFT_MUNICIPAL_TOLERANCE").is_ok() {
            // How far apart the copies of borders matched only at the wide tolerance are.
            let parents: Vec<topology::Parent> = detail
                .parents
                .iter()
                .map(|state| topology::Parent {
                    country: None,
                    state: *state,
                })
                .collect();
            let wide = topology::classify(&detail.lines, &parents, 0);
            let exact = topology::classify(&detail.lines, &parents, detail.lines.len());
            let mut widened = 0;
            for (w, e) in wide.iter().zip(&exact) {
                for (w, e) in w.iter().zip(e) {
                    if *e == topology::OUTER && *w != topology::OUTER {
                        widened += 1;
                    }
                }
            }
            let count = |classes: &[Vec<u8>], class: u8| -> usize {
                classes.iter().flatten().filter(|c| **c == class).count()
            };
            println!(
                "{iso}: state-border segments {} at 1.3 km, {} at 50 m; outline {} vs {}",
                count(&wide, topology::MAJOR),
                count(&exact, topology::MAJOR),
                count(&wide, topology::OUTER),
                count(&exact, topology::OUTER)
            );
            for tolerance in [0.001, 0.002, 0.003, 0.005] {
                let mid = topology::classify_at(&detail.lines, &parents, tolerance);
                let still: usize = mid
                    .iter()
                    .zip(&exact)
                    .zip(&wide)
                    .map(|((m, e), w)| {
                        m.iter()
                            .zip(e)
                            .zip(w)
                            .filter(|((m, e), w)| {
                                **e == topology::OUTER
                                    && **w != topology::OUTER
                                    && **m == topology::OUTER
                            })
                            .count()
                    })
                    .sum();
                println!(
                    "{iso}: {widened} segments shared only at 1.3 km; at {tolerance}°, {still} of them stay unshared"
                );
            }
        }
        let unassigned = detail.parents.iter().filter(|p| p.is_none()).count();
        let lines = detail.lines.len();
        let bounds = detail
            .lines
            .iter()
            .filter_map(|line| GeoBounds::of(line))
            .reduce(GeoBounds::union)
            .unwrap();
        let unit = Unit::Country(iso.as_bytes().try_into().unwrap());
        let neighbours = store.neighbours(unit, bounds.expanded(topology::SHARED_TOLERANCE_DEG));
        let (data, upgrades) = UnitData::new(
            detail,
            (0.0, 0.0),
            Some(iso.as_bytes().try_into().unwrap()),
            &neighbours,
        );
        store.units.insert(unit, data);
        store.apply(upgrades);
        println!(
            "{iso}: {lines} rings, {unassigned} without a state, {} ms",
            started.elapsed().as_millis()
        );
    }
    // Natural Earth's land borders with the two countries each separates, as the app
    // hands them to the streamer.
    let mut border_lines = Vec::new();
    for feature in features("ne_10m_admin_0_boundary_lines_land.geojson") {
        let code = |key: &str| -> Option<[u8; 3]> {
            feature["properties"][key]
                .as_str()?
                .as_bytes()
                .try_into()
                .ok()
        };
        let (Some(left), Some(right)) = (code("ADM0_A3_L"), code("ADM0_A3_R")) else {
            continue;
        };
        let mut lines = Vec::new();
        if let Some(coords) = feature.pointer("/geometry/coordinates") {
            super::super::geojson::collect_geojson_lines(coords, &mut lines);
        }
        border_lines.extend(lines.into_iter().map(|line| (line, [left, right])));
    }
    store.borders = Some(Arc::new(borders::CountryBorders::new(border_lines)));
    // Diagnostics: why outline segments near a land border do not match it.
    let mut unmatched = Vec::new();
    if let Some(index) = store.borders.as_deref() {
        let mut by_class = std::collections::BTreeMap::new();
        for (unit, data) in &store.units {
            let Some(country) = unit.country() else {
                continue;
            };
            for ring in &data.lines {
                for (segment, pair) in ring.line.windows(2).enumerate() {
                    let class = ring.classes[segment];
                    let matched = index.across(pair[0], pair[1], country).is_some();
                    *by_class.entry((class, matched)).or_insert(0_usize) += 1;
                    if !matched && matches!(class, topology::OUTER | topology::COUNTRY) {
                        unmatched.push(vec![pair[0], pair[1]]);
                    }
                }
            }
        }
        println!("segments by (class, matches a land border): {by_class:?}");
        if let Ok(spot) = std::env::var("GLOBECRAFT_MUNICIPAL_SPOT") {
            let spot: Vec<f64> = spot.split(',').map(|v| v.parse().unwrap()).collect();
            for data in store.units.values() {
                for ring in &data.lines {
                    for (segment, pair) in ring.line.windows(2).enumerate() {
                        let mid = ((pair[0].0 + pair[1].0) / 2.0, (pair[0].1 + pair[1].1) / 2.0);
                        if (mid.0 - spot[0]).abs() < 0.02 && (mid.1 - spot[1]).abs() < 0.03 {
                            println!(
                                "ring {:016x} ({} points) segment {segment}: class {} {:?}",
                                ring.id,
                                ring.line.len(),
                                ring.classes[segment],
                                pair
                            );
                        }
                    }
                }
            }
        }
    }
    let started = std::time::Instant::now();
    let patch = assemble(&store, GeoBox::around(area[0], area[1], area[2]));
    println!("assembled in {} ms", started.elapsed().as_millis());
    let staging = patch.municipal.unwrap();
    let ne = |name: &str| -> Vec<Line> {
        let mut lines = Vec::new();
        for feature in features(name) {
            if let Some(coords) = feature.pointer("/geometry/coordinates") {
                super::super::geojson::collect_geojson_lines(coords, &mut lines);
            }
        }
        let inside = GeoBox::around(area[0], area[1], area[2] * 1.2);
        lines
            .into_iter()
            .filter(|line| line.iter().any(|p| inside.contains(p.0, p.1)))
            .collect()
    };
    // The Natural Earth lines as the renderer cuts them: vertices covered by a municipal
    // state (or country) border are dropped.
    let kept = |lines: Vec<Line>, cells: &topology::MajorCells| -> Vec<Vec<(f64, f64)>> {
        let mut out = Vec::new();
        for line in lines {
            let mut run = Vec::new();
            for &p in line.iter() {
                if cells.covers(p.0, p.1) {
                    if run.len() >= 2 {
                        out.push(std::mem::take(&mut run));
                    }
                    run.clear();
                } else {
                    run.push(p);
                }
            }
            if run.len() >= 2 {
                out.push(run);
            }
        }
        out
    };
    let plain =
        |lines: &[Line]| -> Vec<Vec<(f64, f64)>> { lines.iter().map(|l| l.to_vec()).collect() };
    let json = serde_json::json!({
        "state": plain(&patch.lines_big),
        "interior": plain(&patch.lines),
        "ne_state": kept(ne("ne_10m_admin_1_states_provinces_lines.geojson"), &staging.major_cells),
        "ne_state_all": plain(&ne("ne_10m_admin_1_states_provinces_lines.geojson")),
        "country": plain(&staging.country_lines),
        "ne_country": plain(&ne("ne_10m_admin_0_boundary_lines_land.geojson")),
        "ne_country_all": plain(&ne("ne_10m_admin_0_boundary_lines_land.geojson")),
        "unmatched_outline": unmatched,
    });
    std::fs::write(format!("{dir}/check.json"), json.to_string()).unwrap();
    println!(
        "area: {} state runs, {} country runs, {} interior runs",
        patch.lines_big.len(),
        staging.country_lines.len(),
        patch.lines.len()
    );
}
