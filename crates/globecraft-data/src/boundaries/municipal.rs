//! Municipal outlines streamed by area around the view.
//!
//! The area around the camera is covered by *units*: fixed grid cells of a national
//! service (stable requests, so a pan never refetches what is already loaded), one
//! state of Brazil (IBGE), or one whole country from geoBoundaries' simplified files.
//! Units load nearest first, ahead of the altitude where municipal lines fade in,
//! and stay in a bounded in-memory store. A publisher assembles the lines and names
//! near the view from the store and sends one replacing patch at most every
//! `PUBLISH_INTERVAL`, so panning adds cells smoothly instead of swapping the whole
//! layer after every request.
use super::{
    Detail, GeoBounds, Layer, Line, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM, MapLabel, Patch, StatusLog,
    adm, brazil, log_status, national, world,
};
mod reference;
pub(crate) mod topology;
pub use reference::ReferenceLines;
use reference::{Reference, ReferenceKey};

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Municipal units start loading this far above the altitude where the lines
/// appear, so the nearest ones are ready before the fade-in begins.
pub(super) const MUNICIPAL_PREFETCH_ALTITUDE_KM: f64 = MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM * 1.6;
/// Above this altitude the layer is emptied (the store keeps its units for a
/// later descent). The margin over the prefetch altitude stops altitude jitter
/// from emptying and refilling the layer.
const UNLOAD_ALTITUDE_KM: f64 = MUNICIPAL_PREFETCH_ALTITUDE_KM * 1.15;
/// Ground distance from the view centre to its farthest corner per kilometre of
/// altitude, for the vertical field of view and a typical 16:10 window.
const VIEW_RADIUS_PER_ALTITUDE: f64 = 0.8;
const KM_PER_DEGREE: f64 = 111.32;
/// Units outside the view still load up to this margin beyond it, so a short
/// pan finds them ready.
const PREFETCH_MARGIN: f64 = 1.25;
/// Never request or publish more than this far from the view centre.
const MAX_RADIUS_DEG: f64 = 6.0;
const MAX_INFLIGHT: usize = 3;
/// Overpass is a shared community service: one request at a time.
const MAX_OVERPASS_INFLIGHT: usize = 1;
const FAILED_RETRY: Duration = Duration::from_secs(90);
/// The store evicts the units farthest from the view beyond this many vertices.
const STORE_VERTEX_BUDGET: usize = 4_000_000;
const PUBLISH_INTERVAL: Duration = Duration::from_millis(250);
/// Countries whose units are probed on this grid around the view centre.
const PROBE_STEP_DEG: f64 = 0.5;

/// A latitude/longitude box in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeoBox {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}
impl GeoBox {
    /// Limited to the Web Mercator latitudes and the valid longitude range, as
    /// map services expect.
    pub(crate) fn clamped(self) -> Self {
        Self {
            min_lat: self.min_lat.clamp(-85.0, 85.0),
            max_lat: self.max_lat.clamp(-85.0, 85.0),
            min_lon: self.min_lon.max(-180.0),
            max_lon: self.max_lon.min(180.0),
        }
    }
    fn around(lat: f64, lon: f64, radius_deg: f64) -> Self {
        // Longitude degrees shrink with latitude; widen them to keep the radius in km.
        let lon_radius = radius_deg / lat.to_radians().cos().max(0.2);
        Self {
            min_lat: lat - radius_deg,
            max_lat: lat + radius_deg,
            min_lon: lon - lon_radius,
            max_lon: lon + lon_radius,
        }
    }
    fn intersects(self, b: GeoBounds) -> bool {
        b.min_lat <= self.max_lat
            && b.max_lat >= self.min_lat
            && b.min_lon <= self.max_lon
            && b.max_lon >= self.min_lon
    }
    fn contains(self, lat: f64, lon: f64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat && lon >= self.min_lon && lon <= self.max_lon
    }
}

/// One loadable piece of municipal data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Unit {
    /// A national service's grid cell; `size_mdeg` is the cell edge in millidegrees.
    Cell {
        iso3: [u8; 3],
        x: i32,
        y: i32,
        size_mdeg: u32,
    },
    /// An OSM (Overpass) cell, for countries whose whole-country file failed.
    OsmCell { x: i32, y: i32 },
    /// One Brazilian state from IBGE.
    BrazilState(u32),
    /// A whole country from geoBoundaries (simplified), clipped when published.
    Country([u8; 3]),
}
const OSM_CELL_DEG: f64 = 1.0;

/// OSM cells are heavy for a shared service: only the nearest ones load.
const OSM_MAX_RADIUS_DEG: f64 = 1.0;

impl Unit {
    /// How far from the view centre this kind of unit is worth loading.
    fn max_radius_deg(self) -> f64 {
        match self {
            Self::Cell { iso3, .. } => {
                national::max_radius_deg(std::str::from_utf8(&iso3).unwrap_or_default())
            }
            Self::OsmCell { .. } => OSM_MAX_RADIUS_DEG,
            Self::BrazilState(_) | Self::Country(_) => MAX_RADIUS_DEG,
        }
    }
    fn iso(iso3: &str) -> Option<[u8; 3]> {
        iso3.as_bytes().try_into().ok()
    }
    fn cell_box(x: i32, y: i32, size_deg: f64) -> GeoBox {
        GeoBox {
            min_lat: f64::from(y) * size_deg,
            max_lat: f64::from(y + 1) * size_deg,
            min_lon: f64::from(x) * size_deg,
            max_lon: f64::from(x + 1) * size_deg,
        }
    }
    fn centre(self) -> Option<(f64, f64)> {
        let middle = |b: GeoBox| ((b.min_lat + b.max_lat) / 2.0, (b.min_lon + b.max_lon) / 2.0);
        match self {
            Self::Cell {
                x, y, size_mdeg, ..
            } => Some(middle(Self::cell_box(x, y, f64::from(size_mdeg) / 1000.0))),
            Self::OsmCell { x, y } => Some(middle(Self::cell_box(x, y, OSM_CELL_DEG))),
            Self::BrazilState(uf) => brazil::BR_UF_BOUNDS
                .iter()
                .find(|b| b.0 == uf)
                .map(|&(_, a, b, c, d)| ((a + c) / 2.0, (b + d) / 2.0)),
            Self::Country(_) => None,
        }
    }
    /// The country whose municipalities this unit loads; OSM cells can span several.
    fn country(self) -> Option<[u8; 3]> {
        match self {
            Self::Cell { iso3, .. } | Self::Country(iso3) => Some(iso3),
            Self::BrazilState(_) => Some(*b"BRA"),
            Self::OsmCell { .. } => None,
        }
    }
    fn is_overpass(self) -> bool {
        matches!(self, Self::OsmCell { .. })
    }
}

/// The units that cover a point of a country.
fn units_at(iso3: &str, lat: f64, lon: f64, osm_countries: &HashSet<[u8; 3]>) -> Option<Unit> {
    let iso = Unit::iso(iso3)?;
    if iso3 == "BRA" {
        return brazil::BR_UF_BOUNDS
            .iter()
            .find(|&&(_, a, b, c, d)| lat >= a && lat <= c && lon >= b && lon <= d)
            .map(|&(uf, ..)| Unit::BrazilState(uf));
    }
    if let Some(size) = national::cell_size_deg(iso3) {
        return Some(Unit::Cell {
            iso3: iso,
            x: (lon / size).floor() as i32,
            y: (lat / size).floor() as i32,
            size_mdeg: (size * 1000.0).round() as u32,
        });
    }
    // OSM cells cover countries without a geoBoundaries municipal level, and those
    // whose file failed.
    if osm_countries.contains(&iso) || adm::level(iso3).is_none() {
        return Some(Unit::OsmCell {
            x: (lon / OSM_CELL_DEG).floor() as i32,
            y: (lat / OSM_CELL_DEG).floor() as i32,
        });
    }
    Some(Unit::Country(iso))
}

/// A loaded unit: its outlines (with their bounds, for clipping) and names.
struct UnitLine {
    line: Line,
    bounds: GeoBounds,
    /// Identity across units (see `topology::ring_id`).
    id: u64,
    /// One `topology` class per segment.
    classes: Arc<[u8]>,
    /// The class each segment traces on the Natural Earth lines (see `reference`),
    /// computed for `Store::reference_revision` `traced_revision`; empty until then.
    traced: Arc<[u8]>,
    traced_revision: u64,
    parent: topology::Parent,
    area_km2: f64,
}

/// A ring already in the store near a unit that is being classified.
struct Neighbour {
    unit: Unit,
    index: usize,
    id: u64,
    line: Line,
    parent: topology::Parent,
}

/// Classes found for a stored ring once its neighbouring unit loaded.
struct Upgrade {
    unit: Unit,
    index: usize,
    id: u64,
    classes: Vec<u8>,
}

struct UnitData {
    lines: Vec<UnitLine>,
    labels: Vec<MapLabel>,
    vertices: usize,
    centre: (f64, f64),
}
impl UnitData {
    /// Classifies every segment against the unit's own rings and the stored rings of
    /// neighbouring units, and returns the classes those neighbours gain (their
    /// segments on the shared edge were `OUTER` until now). CPU work, so it runs on
    /// the feature pool.
    fn new(
        detail: Detail,
        centre: (f64, f64),
        country: Option<[u8; 3]>,
        neighbours: &[Neighbour],
        reference: Option<&(Arc<Reference>, u64)>,
    ) -> (Self, Vec<Upgrade>) {
        let vertices = detail.lines.iter().map(|line| line.len()).sum();
        let own = detail.lines.len();
        let mut parents: Vec<topology::Parent> = (0..own)
            .map(|i| topology::Parent {
                country,
                state: detail.parents.get(i).copied().flatten(),
            })
            .collect();
        let own_ids: HashSet<u64> = detail.lines.iter().map(topology::ring_id).collect();
        // Copies of this unit's own rings stored by another unit add nothing.
        let others: Vec<&Neighbour> = neighbours
            .iter()
            .filter(|n| !own_ids.contains(&n.id))
            .collect();
        let mut combined = detail.lines.clone();
        combined.extend(others.iter().map(|n| n.line.clone()));
        parents.extend(others.iter().map(|n| n.parent));
        let mut classes = topology::classify(&combined, &parents);
        let upgrades = classes
            .drain(own..)
            .zip(&others)
            .map(|(classes, n)| Upgrade {
                unit: n.unit,
                index: n.index,
                id: n.id,
                classes,
            })
            .collect();
        let lines = detail
            .lines
            .into_iter()
            .zip(classes)
            .zip(parents)
            .filter_map(|((line, classes), parent)| {
                let bounds = GeoBounds::of(&line)?;
                let (traced, traced_revision) = match reference {
                    Some((reference, revision)) => (reference.trace(&line), *revision),
                    None => (Arc::from([]), 0),
                };
                Some(UnitLine {
                    id: topology::ring_id(&line),
                    area_km2: topology::ring_area_km2(&line),
                    classes: classes.into(),
                    traced,
                    traced_revision,
                    parent,
                    line,
                    bounds,
                })
            })
            .collect();
        (
            Self {
                lines,
                labels: detail.labels,
                vertices,
                centre,
            },
            upgrades,
        )
    }
    fn empty(centre: (f64, f64)) -> Self {
        Self {
            lines: Vec::new(),
            labels: Vec::new(),
            vertices: 0,
            centre,
        }
    }
}

impl Store {
    /// Stored rings within `margin` of `area`, from units other than `unit`.
    fn neighbours(&self, unit: Unit, area: GeoBounds) -> Vec<Neighbour> {
        let mut out = Vec::new();
        for (&other, data) in &self.units {
            if other == unit {
                continue;
            }
            for (index, ring) in data.lines.iter().enumerate() {
                if ring.bounds.overlaps(area) {
                    out.push(Neighbour {
                        unit: other,
                        index,
                        id: ring.id,
                        line: ring.line.clone(),
                        parent: ring.parent,
                    });
                }
            }
        }
        out
    }
    /// Applies classes found for stored rings, if they are still stored.
    fn apply(&mut self, upgrades: Vec<Upgrade>) {
        for upgrade in upgrades {
            let Some(ring) = self
                .units
                .get_mut(&upgrade.unit)
                .and_then(|data| data.lines.get_mut(upgrade.index))
                .filter(|ring| ring.id == upgrade.id)
            else {
                continue;
            };
            let mut classes = ring.classes.to_vec();
            topology::upgrade_outer(&mut classes, &upgrade.classes);
            ring.classes = classes.into();
        }
    }
    /// Stored rings not yet traced against the current reference.
    fn untraced(&self) -> Vec<(Unit, usize, u64, Line)> {
        let Some((_, revision)) = self.reference else {
            return Vec::new();
        };
        self.units
            .iter()
            .flat_map(|(&unit, data)| {
                data.lines
                    .iter()
                    .enumerate()
                    .filter(move |(_, ring)| ring.traced_revision != revision)
                    .map(move |(index, ring)| (unit, index, ring.id, ring.line.clone()))
            })
            .collect()
    }
}

#[derive(Default)]
struct Store {
    units: HashMap<Unit, UnitData>,
    inflight: HashSet<Unit>,
    failed: HashMap<Unit, Instant>,
    vertices: usize,
    /// Countries whose geoBoundaries file failed; they fall back to OSM cells.
    osm_countries: HashSet<[u8; 3]>,
    /// The area the layer shows, or `None` while the layer should be empty.
    active: Option<GeoBox>,
    /// Bumped whenever the published content may change.
    revision: u64,
    /// Bumped when any load finishes, so waiting units get their turn.
    completed: u64,
    /// The Natural Earth lines outlines are traced against, with its revision
    /// (starting at 1; 0 marks untraced lines).
    reference: Option<(Arc<Reference>, u64)>,
}
impl Store {
    /// Drops the units farthest from `centre` until the vertex budget holds.
    fn evict(&mut self, centre: (f64, f64)) {
        if self.vertices <= STORE_VERTEX_BUDGET {
            return;
        }
        let mut order: Vec<(Unit, f64)> = self
            .units
            .iter()
            .map(|(unit, data)| (*unit, distance_sq(centre, data.centre)))
            .collect();
        order.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (unit, _) in order {
            if self.vertices <= STORE_VERTEX_BUDGET {
                break;
            }
            if let Some(data) = self.units.remove(&unit) {
                self.vertices -= data.vertices;
            }
        }
    }
}

fn distance_sq(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dlon = (a.1 - b.1) * a.0.to_radians().cos();
    (a.0 - b.0).powi(2) + dlon.powi(2)
}

/// What the camera shows, from the app, once per frame.
#[derive(Clone, Copy, Debug)]
pub struct MunicipalView<'a> {
    pub enabled: bool,
    pub altitude_km: f64,
    pub lat: f64,
    pub lon: f64,
    /// ISO3 codes of the countries to load; empty loads every country.
    pub countries: &'a [String],
}

/// The request derived from a view, compared between frames so an unchanged view
/// costs nothing.
#[derive(Clone, Copy, PartialEq)]
struct Request {
    lat_cell: i64,
    lon_cell: i64,
    radius_cell: i64,
    completed: u64,
    /// Hash of the country selection, so a change re-plans.
    countries: u64,
}

pub(super) struct MunicipalStreamer {
    runtime: tokio::runtime::Handle,
    store: Arc<Mutex<Store>>,
    log: StatusLog,
    wake_publisher: Arc<tokio::sync::Notify>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    last: Option<Request>,
    /// The reference lines last handed to the tracer.
    reference: Option<ReferenceKey>,
}

impl MunicipalStreamer {
    pub(super) fn new(
        runtime: tokio::runtime::Handle,
        tx: mpsc::Sender<Patch>,
        log: StatusLog,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let store = Arc::new(Mutex::new(Store::default()));
        let wake_publisher = Arc::new(tokio::sync::Notify::new());
        runtime.spawn(publish(
            store.clone(),
            wake_publisher.clone(),
            tx,
            repaint.clone(),
        ));
        Self {
            runtime,
            store,
            log,
            wake_publisher,
            repaint,
            last: None,
            reference: None,
        }
    }

    pub(super) fn is_loading(&self) -> bool {
        self.store
            .lock()
            .is_ok_and(|store| !store.inflight.is_empty())
    }

    /// Follows the camera: chooses the units around the view, starts the nearest
    /// missing ones, and moves the published area. Cheap when nothing changed.
    pub(super) fn update(
        &mut self,
        view: MunicipalView<'_>,
        reference: ReferenceLines<'_>,
        country_at: impl Fn(f64, f64) -> Option<String>,
    ) {
        self.follow_reference(reference);
        let Ok(mut store) = self.store.lock() else {
            return;
        };
        if !view.enabled || !view.altitude_km.is_finite() || view.altitude_km > UNLOAD_ALTITUDE_KM {
            if store.active.take().is_some() {
                store.revision += 1;
                self.wake_publisher.notify_one();
            }
            self.last = None;
            return;
        }
        if view.altitude_km > MUNICIPAL_PREFETCH_ALTITUDE_KM {
            // Between prefetch and unload: keep what is shown, start nothing new.
            return;
        }
        let view_radius = (view.altitude_km * VIEW_RADIUS_PER_ALTITUDE / KM_PER_DEGREE)
            .clamp(PROBE_STEP_DEG, MAX_RADIUS_DEG);
        let request = Request {
            lat_cell: (view.lat / PROBE_STEP_DEG * 4.0).round() as i64,
            lon_cell: (view.lon / PROBE_STEP_DEG * 4.0).round() as i64,
            radius_cell: (view_radius / PROBE_STEP_DEG * 4.0).round() as i64,
            completed: store.completed,
            countries: {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::hash::DefaultHasher::new();
                view.countries.hash(&mut hasher);
                hasher.finish()
            },
        };
        if self.last == Some(request) {
            return;
        }
        self.last = Some(request);
        let (area, wanted) = plan(view, view_radius, &country_at, &store.osm_countries);
        if store.active != Some(area) {
            store.active = Some(area);
            self.wake_publisher.notify_one();
        }
        let now = Instant::now();
        for unit in wanted {
            if store.inflight.len() >= MAX_INFLIGHT {
                break;
            }
            if store.units.contains_key(&unit)
                || store.inflight.contains(&unit)
                || store
                    .failed
                    .get(&unit)
                    .is_some_and(|at| now.duration_since(*at) < FAILED_RETRY)
                || (unit.is_overpass()
                    && store.inflight.iter().filter(|u| u.is_overpass()).count()
                        >= MAX_OVERPASS_INFLIGHT)
            {
                continue;
            }
            store.inflight.insert(unit);
            self.runtime.spawn(load(
                unit,
                (view.lat, view.lon),
                self.store.clone(),
                self.wake_publisher.clone(),
                self.log.clone(),
                self.repaint.clone(),
            ));
        }
    }
}

impl MunicipalStreamer {
    /// Rebuilds the trace reference when the Natural Earth lines changed (a detail
    /// level or the state lines arrived), then traces every stored ring against it.
    fn follow_reference(&mut self, lines: ReferenceLines<'_>) {
        let key = lines.key();
        if lines.is_empty() || self.reference == Some(key) {
            return;
        }
        self.reference = Some(key);
        let (states, countries) = lines.to_owned();
        let store = self.store.clone();
        let wake = self.wake_publisher.clone();
        let repaint = self.repaint.clone();
        self.runtime.spawn(async move {
            let Ok(reference) = crate::cpu::run(crate::cpu::Service::Features, move || {
                Ok(Arc::new(Reference::build(&states, &countries)))
            })
            .await
            else {
                return;
            };
            let (pending, current) = {
                let Ok(mut store) = store.lock() else {
                    return;
                };
                let revision = store.reference.as_ref().map_or(0, |(_, r)| *r) + 1;
                store.reference = Some((reference.clone(), revision));
                (store.untraced(), revision)
            };
            let Ok(traced) = crate::cpu::run(crate::cpu::Service::Features, move || {
                Ok(pending
                    .into_iter()
                    .map(|(unit, index, id, line)| (unit, index, id, reference.trace(&line)))
                    .collect::<Vec<_>>())
            })
            .await
            else {
                return;
            };
            if let Ok(mut store) = store.lock() {
                if store.reference.as_ref().is_some_and(|(_, r)| *r == current) {
                    for (unit, index, id, classes) in traced {
                        if let Some(ring) = store
                            .units
                            .get_mut(&unit)
                            .and_then(|data| data.lines.get_mut(index))
                            .filter(|ring| ring.id == id)
                        {
                            ring.traced = classes;
                            ring.traced_revision = current;
                        }
                    }
                }
                store.revision += 1;
            }
            wake.notify_one();
            repaint();
        });
    }
}

/// The published area around the view and the units covering it, nearest first.
fn plan(
    view: MunicipalView<'_>,
    view_radius: f64,
    country_at: &impl Fn(f64, f64) -> Option<String>,
    osm_countries: &HashSet<[u8; 3]>,
) -> (GeoBox, Vec<Unit>) {
    let radius = (view_radius * PREFETCH_MARGIN).min(MAX_RADIUS_DEG);
    let area = GeoBox::around(view.lat, view.lon, radius);
    // Probe a grid over the area for the countries and units it covers.
    let mut wanted: Vec<(Unit, f64)> = Vec::new();
    let steps = (radius / PROBE_STEP_DEG).ceil() as i32;
    let lon_scale = view.lat.to_radians().cos().max(0.2);
    for dy in -steps..=steps {
        for dx in -steps..=steps {
            let lat = view.lat + f64::from(dy) * PROBE_STEP_DEG;
            let lon = view.lon + f64::from(dx) * PROBE_STEP_DEG / lon_scale;
            if !area.contains(lat, lon) {
                continue;
            }
            let Some(iso3) = country_at(lat, lon) else {
                continue;
            };
            if !view.countries.is_empty() && !view.countries.contains(&iso3) {
                continue;
            }
            let Some(unit) = units_at(&iso3, lat, lon, osm_countries) else {
                continue;
            };
            let distance = distance_sq((view.lat, view.lon), (lat, lon));
            if distance > unit.max_radius_deg().powi(2) {
                continue;
            }
            match wanted.iter_mut().find(|(known, _)| *known == unit) {
                Some((_, best)) => *best = best.min(distance),
                None => wanted.push((unit, distance)),
            }
        }
    }
    wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
    (area, wanted.into_iter().map(|(unit, _)| unit).collect())
}

async fn load_unit(unit: Unit) -> Result<Detail, crate::Error> {
    match unit {
        Unit::Cell {
            iso3,
            x,
            y,
            size_mdeg,
        } => {
            let iso3 = std::str::from_utf8(&iso3).unwrap_or_default().to_owned();
            let area = Unit::cell_box(x, y, f64::from(size_mdeg) / 1000.0);
            national::load_cell(&iso3, area)
                .await
                .unwrap_or_else(|| Err(format!("{iso3}: no national source").into()))
        }
        Unit::OsmCell { x, y } => {
            let lines = world::load_osm_cell(Unit::cell_box(x, y, OSM_CELL_DEG)).await?;
            Ok(Detail {
                features: lines.len(),
                parents: vec![],
                lines,
                labels: vec![],
            })
        }
        Unit::BrazilState(uf) => brazil::load_state_municipalities(uf).await,
        Unit::Country(iso3) => adm::load(std::str::from_utf8(&iso3).unwrap_or_default()).await,
    }
}

fn describe(unit: Unit) -> String {
    let iso = |code: [u8; 3]| String::from_utf8_lossy(&code).into_owned();
    match unit {
        Unit::Cell {
            iso3,
            x,
            y,
            size_mdeg,
        } => {
            let size = f64::from(size_mdeg) / 1000.0;
            format!(
                "{} cell {:.1}°, {:.1}°",
                national::source_label(&iso(iso3)).unwrap_or("national"),
                f64::from(y) * size,
                f64::from(x) * size
            )
        }
        Unit::OsmCell { x, y } => format!("OSM cell {y}°, {x}°"),
        Unit::BrazilState(uf) => format!("IBGE state {uf}"),
        Unit::Country(code) => format!("geoBoundaries {} (whole country)", iso(code)),
    }
}

async fn load(
    unit: Unit,
    view_centre: (f64, f64),
    store: Arc<Mutex<Store>>,
    wake_publisher: Arc<tokio::sync::Notify>,
    log: StatusLog,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let started = Instant::now();
    let centre = unit.centre().unwrap_or(view_centre);
    let result = match load_unit(unit).await {
        Ok(detail) => {
            // Rings already stored around this unit, so borders between units are
            // classified too (Brazilian states, countries, cells).
            let area = detail
                .lines
                .iter()
                .filter_map(|line| GeoBounds::of(line))
                .reduce(GeoBounds::union)
                .map(|bounds| bounds.expanded(topology::SHARED_TOLERANCE_DEG));
            let (neighbours, reference) = match store.lock() {
                Ok(store) => (
                    area.map(|area| store.neighbours(unit, area))
                        .unwrap_or_default(),
                    store.reference.clone(),
                ),
                Err(_) => (Vec::new(), None),
            };
            crate::cpu::run(crate::cpu::Service::Features, move || {
                Ok(UnitData::new(
                    detail,
                    centre,
                    unit.country(),
                    &neighbours,
                    reference.as_ref(),
                ))
            })
            .await
        }
        Err(error) => Err(error),
    };
    {
        let Ok(mut store) = store.lock() else {
            return;
        };
        store.inflight.remove(&unit);
        store.completed += 1;
        match result {
            Ok((data, upgrades)) if !data.lines.is_empty() => {
                log_status(
                    &log,
                    format!(
                        "✔ {}: {} lines, {} names in {} ms",
                        describe(unit),
                        data.lines.len(),
                        data.labels.len(),
                        started.elapsed().as_millis()
                    ),
                );
                store.vertices += data.vertices;
                store.units.insert(unit, data);
                store.apply(upgrades);
                store.evict(view_centre);
                store.revision += 1;
            }
            Ok(_) => {
                // An empty answer (open sea, an unmapped area) is final for this unit.
                store.units.insert(unit, UnitData::empty(centre));
            }
            Err(error) => {
                log_status(&log, format!("✖ {}: {error}", describe(unit)));
                store.failed.insert(unit, Instant::now());
                if let Unit::Country(iso3) = unit {
                    // The whole-country file is unavailable: use OSM cells instead.
                    store.osm_countries.insert(iso3);
                }
                store.revision += 1;
            }
        }
    }
    wake_publisher.notify_one();
    repaint();
}

/// One ring in an assembled area: its points, its merged topology classes and the
/// classes it traces on the Natural Earth lines.
type MergedRing = (Line, Vec<u8>, Arc<[u8]>);

/// The lines and names of the store inside `area`, with lines shared by
/// neighbouring units kept once.
fn assemble(store: &Store, area: GeoBox) -> Patch {
    // The same municipality can come from two units (a cell edge); merge its classes.
    let current = store.reference.as_ref().map(|(_, revision)| *revision);
    let mut rings: HashMap<u64, MergedRing> = HashMap::new();
    let mut order = Vec::new();
    let mut labels = Vec::new();
    let (mut area_sum, mut area_count) = (0.0, 0_usize);
    for data in store.units.values() {
        for unit_line in &data.lines {
            if !area.intersects(unit_line.bounds) {
                continue;
            }
            match rings.entry(unit_line.id) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let traced = if Some(unit_line.traced_revision) == current {
                        unit_line.traced.clone()
                    } else {
                        Arc::from([])
                    };
                    entry.insert((unit_line.line.clone(), unit_line.classes.to_vec(), traced));
                    order.push(unit_line.id);
                    if unit_line.area_km2 > 0.0 {
                        area_sum += unit_line.area_km2;
                        area_count += 1;
                    }
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    topology::merge(&mut entry.get_mut().1, &unit_line.classes);
                }
            }
        }
        labels.extend(
            data.labels
                .iter()
                .filter(|label| area.contains(label.lat, label.lon))
                .cloned(),
        );
    }
    let mut split = topology::SplitLines::default();
    for id in order {
        let (line, classes, traced) = &rings[&id];
        topology::split(line, &topology::with_traced(classes, traced), &mut split);
    }
    let topology::SplitLines {
        country,
        major,
        interior,
    } = split;
    let mean_area_km2 = if area_count > 0 {
        area_sum / area_count as f64
    } else {
        0.0
    };
    // Natural Earth state lines also give way along municipal country borders.
    let major_cells = Arc::new(topology::MajorCells::of(
        &major.iter().chain(&country).cloned().collect::<Vec<_>>(),
    ));
    let country_cells = Arc::new(topology::MajorCells::of(&country));
    Patch {
        layer: Layer::WorldCities,
        lines: interior,
        lines_big: major,
        labels,
        regions: vec![],
        replace: true,
        municipal: Some(super::MunicipalStaging {
            detail_max_km: topology::detail_altitude_km(
                mean_area_km2,
                MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM,
            ),
            major_cells,
            country_lines: country,
            country_cells,
        }),
    }
}

/// Sends the assembled layer whenever the store or the area changed, at most once
/// per `PUBLISH_INTERVAL`.
async fn publish(
    store: Arc<Mutex<Store>>,
    wake: Arc<tokio::sync::Notify>,
    tx: mpsc::Sender<Patch>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let mut sent: Option<(u64, Option<GeoBox>)> = None;
    loop {
        wake.notified().await;
        tokio::time::sleep(PUBLISH_INTERVAL).await;
        let patch = {
            let Ok(store) = store.lock() else {
                return;
            };
            let state = (store.revision, store.active);
            if sent == Some(state) {
                continue;
            }
            sent = Some(state);
            match store.active {
                Some(area) => assemble(&store, area),
                None => Patch {
                    layer: Layer::WorldCities,
                    lines: vec![],
                    lines_big: vec![],
                    labels: vec![],
                    regions: vec![],
                    replace: true,
                    municipal: None,
                },
            }
        };
        if tx.send(patch).is_err() {
            return;
        }
        repaint();
    }
}

#[cfg(test)]
mod tests;
