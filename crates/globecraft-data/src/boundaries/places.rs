//! Town, village and island names around the view, from OpenStreetMap.
//!
//! Natural Earth's populated places (about 7,300 worldwide) name only the larger towns,
//! and nothing names islands. Below `LOAD_MAX_ALTITUDE_KM` the names in fixed grid cells
//! around the view are fetched from Overpass, a few cells at a time, and kept in memory
//! while the camera stays near them. Smaller places show only closer in (see
//! `MapLabel::visible_with_detail`).
use super::{LabelKind, Layer, MapLabel, Patch, StatusLog, log_status};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Labels of towns and big islands.
pub(super) const PLACE_TOWN_RANK: u8 = 11;
/// Labels of villages and mid-sized islands.
pub(super) const PLACE_VILLAGE_RANK: u8 = 12;
/// Labels of hamlets and islets.
pub(super) const PLACE_HAMLET_RANK: u8 = 13;

/// Cell size: a cell of hamlets in a dense region is a few hundred names.
const CELL_DEG: f64 = 0.5;
/// Names load below this altitude (towns show from 150 km).
const LOAD_MAX_ALTITUDE_KM: f64 = 160.0;
/// Ground radius around the view whose cells are wanted, per km of altitude.
const VIEW_RADIUS_PER_ALTITUDE: f64 = 1.5;
const KM_PER_DEGREE: f64 = 111.32;
const MAX_INFLIGHT: usize = 2;
const MAX_STORED_CELLS: usize = 300;
const FAILED_RETRY: Duration = Duration::from_secs(300);
const MAX_RESPONSE_BYTES: u64 = 8_000_000;
/// Islands at least this big (km², from their bounding box) count as towns, at least
/// `ISLAND_VILLAGE_KM2` as villages, smaller ones as hamlets.
const ISLAND_TOWN_KM2: f64 = 50.0;
const ISLAND_VILLAGE_KM2: f64 = 2.0;
const ENDPOINTS: [&str; 2] = [
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass-api.de/api/interpreter",
];

/// What the place layer should show.
#[derive(Clone, Copy)]
pub struct PlaceView {
    pub enabled: bool,
    pub lat: f64,
    pub lon: f64,
    pub altitude_km: f64,
}

type Cell = (i32, i32);

#[derive(Default)]
struct Store {
    cells: HashMap<Cell, (Arc<Vec<MapLabel>>, Instant)>,
    inflight: HashSet<Cell>,
    failed: HashMap<Cell, Instant>,
    /// The cells published last time, so an unchanged set is not sent again.
    published: Vec<Cell>,
}

pub(super) struct PlaceStreamer {
    runtime: tokio::runtime::Handle,
    tx: mpsc::Sender<Patch>,
    log: StatusLog,
    repaint: Arc<dyn Fn() + Send + Sync>,
    store: Arc<Mutex<Store>>,
}

fn cell_of(lat: f64, lon: f64) -> Cell {
    (
        (lat / CELL_DEG).floor() as i32,
        (lon / CELL_DEG).floor() as i32,
    )
}

/// The cells around the view, nearest first.
fn wanted(view: PlaceView) -> Vec<Cell> {
    let radius = (view.altitude_km * VIEW_RADIUS_PER_ALTITUDE / KM_PER_DEGREE).clamp(0.2, 2.0);
    let lon_radius = radius / view.lat.to_radians().cos().max(0.2);
    let (y0, x0) = cell_of(view.lat - radius, view.lon - lon_radius);
    let (y1, x1) = cell_of(view.lat + radius, view.lon + lon_radius);
    let (cy, cx) = cell_of(view.lat, view.lon);
    let mut cells: Vec<Cell> = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (y, x)))
        .filter(|(y, _)| (-180..180).contains(y))
        .map(|(y, x)| (y, (x + 360).rem_euclid(720) - 360))
        .collect();
    cells.sort_by_key(|&(y, x)| (y - cy).pow(2) + (x - cx).pow(2));
    cells.dedup();
    cells
}

impl PlaceStreamer {
    pub(super) fn new(
        runtime: tokio::runtime::Handle,
        tx: mpsc::Sender<Patch>,
        log: StatusLog,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            runtime,
            tx,
            log,
            repaint,
            store: Arc::default(),
        }
    }

    /// Follows the camera: starts the nearest missing cells and publishes the names
    /// of the cells around the view. Cheap when nothing changed.
    pub(super) fn update(&mut self, view: PlaceView) {
        let active = view.enabled && view.altitude_km <= LOAD_MAX_ALTITUDE_KM;
        let cells = if active { wanted(view) } else { Vec::new() };
        let Ok(mut store) = self.store.lock() else {
            return;
        };
        let now = Instant::now();
        for &cell in &cells {
            if store.inflight.len() >= MAX_INFLIGHT {
                break;
            }
            if store.cells.contains_key(&cell)
                || store.inflight.contains(&cell)
                || store
                    .failed
                    .get(&cell)
                    .is_some_and(|at| now.duration_since(*at) < FAILED_RETRY)
            {
                continue;
            }
            store.inflight.insert(cell);
            self.runtime.spawn(load(
                cell,
                self.store.clone(),
                self.tx.clone(),
                self.log.clone(),
                self.repaint.clone(),
            ));
        }
        for cell in &cells {
            if let Some(entry) = store.cells.get_mut(cell) {
                entry.1 = now;
            }
        }
        publish_if_changed(&mut store, &cells, &self.tx, self.repaint.as_ref());
    }
}

/// Sends the names of `cells` that are stored, unless that set was sent already.
fn publish_if_changed(
    store: &mut Store,
    cells: &[Cell],
    tx: &mpsc::Sender<Patch>,
    repaint: &(dyn Fn() + Send + Sync),
) {
    let shown: Vec<Cell> = cells
        .iter()
        .copied()
        .filter(|cell| store.cells.contains_key(cell))
        .collect();
    if shown == store.published {
        return;
    }
    let labels = shown
        .iter()
        .flat_map(|cell| store.cells[cell].0.iter().cloned())
        .collect();
    store.published = shown;
    let _ = tx.send(Patch {
        layer: Layer::Places,
        pairs: Vec::new(),
        disputed: Vec::new(),
        lines: Vec::new(),
        lines_big: Vec::new(),
        labels,
        regions: Vec::new(),
        replace: true,
        municipal: None,
    });
    repaint();
}

async fn load(
    cell: Cell,
    store: Arc<Mutex<Store>>,
    tx: mpsc::Sender<Patch>,
    log: StatusLog,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let result = fetch(cell).await;
    let Ok(mut store) = store.lock() else {
        return;
    };
    store.inflight.remove(&cell);
    match result {
        Ok(labels) => {
            store.cells.insert(cell, (Arc::new(labels), Instant::now()));
            if store.cells.len() > MAX_STORED_CELLS {
                // Forget the cells the view left longest ago.
                let mut by_age: Vec<(Cell, Instant)> = store
                    .cells
                    .iter()
                    .map(|(cell, (_, at))| (*cell, *at))
                    .collect();
                by_age.sort_by_key(|(_, at)| *at);
                let excess = store.cells.len() - MAX_STORED_CELLS;
                for (old, _) in by_age.into_iter().take(excess) {
                    store.cells.remove(&old);
                }
            }
            // The next `update` publishes the new names with the cells around the view.
            store.published.clear();
        }
        Err(error) => {
            log_status(&log, format!("✖ Place names {cell:?}: {error}"));
            store.failed.insert(cell, Instant::now());
        }
    }
    drop(store);
    drop(tx);
    repaint();
}

async fn fetch(cell: Cell) -> Result<Vec<MapLabel>, crate::Error> {
    let (s, w) = (cell.0 as f64 * CELL_DEG, cell.1 as f64 * CELL_DEG);
    let (n, e) = (s + CELL_DEG, w + CELL_DEG);
    let bbox = format!("{s:.3},{w:.3},{n:.3},{e:.3}");
    let query = format!(
        "[out:json][timeout:25];(\
         node[\"place\"~\"^(city|town|village|hamlet|island|islet)$\"][\"name\"]({bbox});\
         way[\"place\"~\"^(island|islet)$\"][\"name\"]({bbox});\
         relation[\"place\"~\"^(island|islet|archipelago)$\"][\"name\"]({bbox}););\
         out tags bb;"
    );
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let mut last: Option<crate::Error> = None;
    for url in ENDPOINTS {
        match crate::http::body(
            client.post(url).form(&[("data", query.as_str())]),
            MAX_RESPONSE_BYTES,
        )
        .await
        {
            Ok(bytes) => match crate::http::parse_json(bytes).await {
                Ok(json) if json["elements"].is_array() => {
                    return crate::cpu::run(crate::cpu::Service::Features, move || {
                        Ok(parse(&json, cell))
                    })
                    .await;
                }
                Ok(_) => last = Some("unexpected Overpass response".into()),
                Err(error) => last = Some(error),
            },
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| "no Overpass endpoint".into()))
}

/// The names in an Overpass answer that lie in `cell` (a place spanning several cells
/// is kept by the cell holding its centre, so it is listed once).
fn parse(json: &serde_json::Value, cell: Cell) -> Vec<MapLabel> {
    json["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|element| {
            let tags = &element["tags"];
            let name = tags["name"]
                .as_str()
                .filter(|name| !name.trim().is_empty())?;
            let place = tags["place"].as_str()?;
            let (lat, lon, area_km2) = match (element["lat"].as_f64(), element["lon"].as_f64()) {
                (Some(lat), Some(lon)) => (lat, lon, None),
                _ => {
                    let bounds = &element["bounds"];
                    let (s, w) = (bounds["minlat"].as_f64()?, bounds["minlon"].as_f64()?);
                    let (n, e) = (bounds["maxlat"].as_f64()?, bounds["maxlon"].as_f64()?);
                    let lat = (s + n) / 2.0;
                    let area = (n - s)
                        * KM_PER_DEGREE
                        * (e - w)
                        * KM_PER_DEGREE
                        * lat.to_radians().cos().abs();
                    (lat, (w + e) / 2.0, Some(area))
                }
            };
            if !(lat.is_finite() && lon.is_finite()) || cell_of(lat, lon) != cell {
                return None;
            }
            let rank = match place {
                "city" | "town" | "archipelago" => PLACE_TOWN_RANK,
                "village" => PLACE_VILLAGE_RANK,
                "island" => match area_km2 {
                    Some(area) if area >= ISLAND_TOWN_KM2 => PLACE_TOWN_RANK,
                    Some(area) if area < ISLAND_VILLAGE_KM2 => PLACE_HAMLET_RANK,
                    _ => PLACE_VILLAGE_RANK,
                },
                _ => PLACE_HAMLET_RANK,
            };
            let named = |key: &str| {
                tags[key]
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(name)
                    .to_owned()
            };
            Some(MapLabel {
                lat,
                lon,
                en: named("name:en"),
                pt: named("name:pt"),
                kind: LabelKind::City,
                rank,
                big: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn towns_villages_and_islands_are_ranked_by_kind_and_size() {
        let json = serde_json::json!({"elements": [
            {"type": "node", "lat": 40.38, "lon": 71.78, "tags": {"place": "town", "name": "Fergana", "name:en": "Fergana"}},
            {"type": "node", "lat": 40.1, "lon": 71.6, "tags": {"place": "village", "name": "Qishloq"}},
            {"type": "node", "lat": 40.2, "lon": 71.7, "tags": {"place": "hamlet", "name": "Ovul"}},
            // Fais, about 2.8 km²: a village-sized island.
            {"type": "way", "bounds": {"minlat": 40.05, "minlon": 71.55, "maxlat": 40.065, "maxlon": 71.57},
             "tags": {"place": "island", "name": "Fais"}},
            {"type": "relation", "bounds": {"minlat": 40.0, "minlon": 71.5, "maxlat": 40.4, "maxlon": 71.9},
             "tags": {"place": "island", "name": "Big Island"}},
            // Its centre lies in the next cell, which lists it instead.
            {"type": "node", "lat": 40.6, "lon": 71.6, "tags": {"place": "town", "name": "Elsewhere"}},
            {"type": "node", "lat": 40.3, "lon": 71.6, "tags": {"place": "town"}}
        ]});
        let labels = parse(&json, cell_of(40.2, 71.7));
        let rank = |name: &str| labels.iter().find(|label| label.en == name).map(|l| l.rank);
        assert_eq!(rank("Fergana"), Some(PLACE_TOWN_RANK));
        assert_eq!(rank("Qishloq"), Some(PLACE_VILLAGE_RANK));
        assert_eq!(rank("Ovul"), Some(PLACE_HAMLET_RANK));
        assert_eq!(rank("Fais"), Some(PLACE_VILLAGE_RANK));
        assert_eq!(rank("Big Island"), Some(PLACE_TOWN_RANK));
        assert_eq!(rank("Elsewhere"), None, "listed by its own cell");
        assert_eq!(labels.len(), 5, "unnamed places are skipped");
        assert!(labels.iter().all(|label| !label.is_municipal()));
    }

    #[test]
    fn the_view_wants_its_own_cell_first_and_wraps_the_dateline() {
        let cells = wanted(PlaceView {
            enabled: true,
            lat: 9.76,
            lon: 179.9,
            altitude_km: 50.0,
        });
        assert_eq!(cells[0], cell_of(9.76, 179.9));
        assert!(cells.iter().any(|&(_, x)| x == cell_of(9.76, -179.9).1));
    }
}
