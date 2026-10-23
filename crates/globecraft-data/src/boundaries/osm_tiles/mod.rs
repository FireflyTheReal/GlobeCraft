//! Borders and place names from OpenStreetMap vector tiles (OpenMapTiles schema,
//! served by OpenFreeMap), streamed like imagery: a zoom level for the altitude and the
//! tiles around the view.
//!
//! OpenStreetMap stores a border once, shared by the areas on both sides, and
//! OpenMapTiles gives each line the lowest administrative level it belongs to. So a
//! border is never drawn twice and never needs matching between two sources (the
//! municipal pipeline's job, see `docs/BORDER_PIPELINE.md`). Each zoom level is
//! generalised for its scale, so there is no handover between a coarse and a detailed
//! line either: the view switches tile levels like it does for imagery.
mod content;
mod mvt;
mod pick;

use super::{Layer, Line, MapLabel, Patch, StatusLog, log_status};
use content::{TileContent, TileKey};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// OpenFreeMap's TileJSON for the planet; it names the current tile set's URL.
const TILEJSON_URL: &str = "https://tiles.openfreemap.org/planet";
const MAX_TILE_BYTES: u64 = 4_000_000;
const MAX_TILEJSON_BYTES: u64 = 200_000;
/// Parallel tile requests (OpenFreeMap asks bulk clients for at most 4).
const MAX_INFLIGHT: usize = 4;
const MAX_STORED_TILES: usize = 600;
const FAILED_RETRY: Duration = Duration::from_secs(60);
/// Tiles at least this deep hold municipal lines (OpenMapTiles adds `admin_level` 7–8
/// at zoom 11).
pub const MUNICIPAL_MIN_ZOOM: u8 = 11;

/// Border lines from the tiles on show, by administrative level.
#[derive(Clone, Debug, Default)]
pub struct OsmBorderLines {
    /// `admin_level` 2.
    pub country: Vec<Line>,
    /// `admin_level` 3–4, of the big countries (see `BIG_COUNTRIES_A3`), sorted out when
    /// the lines reach the boundary data.
    pub state_big: Vec<Line>,
    /// `admin_level` 3–4 elsewhere.
    pub state: Vec<Line>,
    /// `admin_level` 5–8.
    pub municipal: Vec<Line>,
    /// The zoom level of the tiles wanted for the view.
    pub zoom: u8,
}

impl OsmBorderLines {
    pub fn is_empty(&self) -> bool {
        self.country.is_empty()
            && self.state.is_empty()
            && self.state_big.is_empty()
            && self.municipal.is_empty()
    }
}

/// What the tile layer should show, from the app once per frame.
#[derive(Clone, Copy, Debug)]
pub struct OsmTileView {
    pub enabled: bool,
    pub lat: f64,
    pub lon: f64,
    pub altitude_km: f64,
}

#[derive(Default)]
struct Store {
    /// The tile URL template, once the TileJSON arrived.
    template: Option<String>,
    template_loading: bool,
    template_failed: Option<Instant>,
    tiles: HashMap<TileKey, (Arc<TileContent>, Instant)>,
    inflight: HashSet<TileKey>,
    failed: HashMap<TileKey, Instant>,
    /// Bumped when a tile arrives, so the view is composed again.
    revision: u64,
}

pub(super) struct OsmTileStreamer {
    runtime: tokio::runtime::Handle,
    tx: mpsc::Sender<Patch>,
    log: StatusLog,
    repaint: Arc<dyn Fn() + Send + Sync>,
    store: Arc<Mutex<Store>>,
    /// What was published last: each shown tile and the tile it came from.
    published: Vec<(TileKey, TileKey)>,
    published_revision: u64,
    /// Ancestors cut to a missing tile, kept while they are shown.
    clipped: HashMap<(TileKey, TileKey), Arc<TileContent>>,
}

impl OsmTileStreamer {
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
            published: Vec::new(),
            published_revision: 0,
            clipped: HashMap::new(),
        }
    }

    /// Whether any tile has arrived: until then the other border layers stay on.
    pub(super) fn has_tiles(&self) -> bool {
        self.store.lock().is_ok_and(|store| !store.tiles.is_empty())
    }

    /// Follows the camera: loads the tiles around the view and publishes the borders
    /// and names of the best tiles available for each. Cheap when nothing changed.
    pub(super) fn update(&mut self, view: OsmTileView) {
        if !view.enabled || !view.altitude_km.is_finite() {
            if !self.published.is_empty() {
                self.published.clear();
                self.clipped.clear();
                self.send(OsmBorderLines::default(), Vec::new());
            }
            return;
        }
        let (zoom, wanted) = pick::wanted(view.lat, view.lon, view.altitude_km);
        let Ok(mut store) = self.store.lock() else {
            return;
        };
        let now = Instant::now();
        let Some(template) = store.template.clone() else {
            let retry = store
                .template_failed
                .is_none_or(|at| now.duration_since(at) >= FAILED_RETRY);
            if !store.template_loading && retry {
                store.template_loading = true;
                self.runtime.spawn(load_template(
                    self.store.clone(),
                    self.log.clone(),
                    self.repaint.clone(),
                ));
            }
            return;
        };
        for &key in &wanted {
            if store.inflight.len() >= MAX_INFLIGHT {
                break;
            }
            if store.tiles.contains_key(&key)
                || store.inflight.contains(&key)
                || store
                    .failed
                    .get(&key)
                    .is_some_and(|at| now.duration_since(*at) < FAILED_RETRY)
            {
                continue;
            }
            store.inflight.insert(key);
            self.runtime.spawn(load_tile(
                key,
                template.clone(),
                self.store.clone(),
                self.log.clone(),
                self.repaint.clone(),
            ));
        }
        for key in &wanted {
            if let Some(entry) = store.tiles.get_mut(key) {
                entry.1 = now;
            }
        }
        let shown = compose(&store.tiles, &wanted);
        if shown == self.published && store.revision == self.published_revision {
            return;
        }
        let revision = store.revision;
        let contents: Vec<Arc<TileContent>> = shown
            .iter()
            .filter_map(|&(tile, source)| {
                let (content, _) = store.tiles.get(&source)?;
                if tile == source {
                    return Some(content.clone());
                }
                Some(
                    self.clipped
                        .entry((tile, source))
                        .or_insert_with(|| Arc::new(content.clipped(tile.bounds())))
                        .clone(),
                )
            })
            .collect();
        drop(store);
        let shown_set: HashSet<(TileKey, TileKey)> = shown.iter().copied().collect();
        self.clipped.retain(|key, _| shown_set.contains(key));
        let mut lines = OsmBorderLines {
            zoom,
            ..Default::default()
        };
        let mut places = Vec::new();
        for content in contents {
            lines.country.extend(content.country.iter().cloned());
            lines.state.extend(content.state.iter().cloned());
            lines.municipal.extend(content.municipal.iter().cloned());
            places.extend(content.places.iter().cloned());
        }
        self.published = shown;
        self.published_revision = revision;
        self.send(lines, places);
    }

    fn send(&self, lines: OsmBorderLines, places: Vec<MapLabel>) {
        let patch = |layer, labels, osm| Patch {
            layer,
            pairs: Vec::new(),
            lines: Vec::new(),
            lines_big: Vec::new(),
            labels,
            regions: Vec::new(),
            replace: true,
            municipal: None,
            osm,
        };
        let _ = self
            .tx
            .send(patch(Layer::OsmBorders, Vec::new(), Some(lines)));
        let _ = self.tx.send(patch(Layer::Places, places, None));
        (self.repaint)();
    }
}

/// For each wanted tile, the tile whose content shows there: itself once loaded, else
/// its nearest loaded ancestor (cut to it), else its loaded children. So a view never
/// goes blank while a new zoom level loads, and no area is covered twice.
fn compose(
    tiles: &HashMap<TileKey, (Arc<TileContent>, Instant)>,
    wanted: &[TileKey],
) -> Vec<(TileKey, TileKey)> {
    let mut shown = Vec::new();
    for &key in wanted {
        if tiles.contains_key(&key) {
            shown.push((key, key));
            continue;
        }
        if let Some(ancestor) = std::iter::successors(key.parent(), |k| k.parent())
            .find(|ancestor| tiles.contains_key(ancestor))
        {
            shown.push((key, ancestor));
            continue;
        }
        shown.extend(
            key.children()
                .into_iter()
                .filter(|child| tiles.contains_key(child))
                .map(|child| (child, child)),
        );
    }
    shown.sort();
    shown
}

async fn load_template(
    store: Arc<Mutex<Store>>,
    log: StatusLog,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let result = fetch_template().await;
    if let Ok(mut store) = store.lock() {
        store.template_loading = false;
        match result {
            Ok(template) => {
                log_status(&log, format!("✔ OpenStreetMap border tiles: {template}"));
                store.template = Some(template);
            }
            Err(error) => {
                log_status(&log, format!("✖ OpenStreetMap border tiles: {error}"));
                store.template_failed = Some(Instant::now());
            }
        }
    }
    repaint();
}

async fn fetch_template() -> Result<String, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(client.get(TILEJSON_URL), MAX_TILEJSON_BYTES).await?;
    let json = crate::http::parse_json(bytes).await?;
    json["tiles"][0]
        .as_str()
        .filter(|url| url.contains("{z}") && url.contains("{x}") && url.contains("{y}"))
        .map(str::to_owned)
        .ok_or_else(|| "TileJSON without a tile URL".into())
}

async fn load_tile(
    key: TileKey,
    template: String,
    store: Arc<Mutex<Store>>,
    log: StatusLog,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let result = fetch_tile(key, &template).await;
    let Ok(mut store) = store.lock() else {
        return;
    };
    store.inflight.remove(&key);
    match result {
        Ok(content) => {
            store.tiles.insert(key, (Arc::new(content), Instant::now()));
            store.revision += 1;
            if store.tiles.len() > MAX_STORED_TILES {
                // Forget the tiles the view left longest ago.
                let mut by_age: Vec<(TileKey, Instant)> =
                    store.tiles.iter().map(|(k, (_, at))| (*k, *at)).collect();
                by_age.sort_by_key(|(_, at)| *at);
                let excess = store.tiles.len() - MAX_STORED_TILES;
                for (old, _) in by_age.into_iter().take(excess) {
                    store.tiles.remove(&old);
                }
            }
        }
        Err(error) => {
            log_status(
                &log,
                format!(
                    "✖ OpenStreetMap border tile {}/{}/{}: {error}",
                    key.z, key.x, key.y
                ),
            );
            store.failed.insert(key, Instant::now());
        }
    }
    drop(store);
    repaint();
}

async fn fetch_tile(key: TileKey, template: &str) -> Result<TileContent, crate::Error> {
    let url = template
        .replace("{z}", &key.z.to_string())
        .replace("{x}", &key.x.to_string())
        .replace("{y}", &key.y.to_string());
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = match crate::http::body(client.get(&url), MAX_TILE_BYTES).await {
        Ok(bytes) => bytes,
        // Tiles of empty ocean may simply not exist.
        Err(crate::Error::Http(error))
            if error.status() == Some(reqwest::StatusCode::NOT_FOUND) =>
        {
            Vec::new()
        }
        Err(error) => return Err(error),
    };
    crate::cpu::run(crate::cpu::Service::Features, move || {
        let bytes = gunzip_if_needed(bytes)?;
        TileContent::from_tile(key, &bytes)
            .ok_or_else(|| crate::Error::from(format!("malformed vector tile {url}")))
    })
    .await
}

/// Tiles are often stored gzip-compressed; servers that pass them on without a
/// `Content-Encoding` header leave the decompression to the client.
fn gunzip_if_needed(bytes: Vec<u8>) -> Result<Vec<u8>, crate::Error> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes);
    }
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(keys: &[TileKey]) -> HashMap<TileKey, (Arc<TileContent>, Instant)> {
        keys.iter()
            .map(|&k| (k, (Arc::default(), Instant::now())))
            .collect()
    }

    #[test]
    fn missing_tiles_borrow_from_an_ancestor_or_their_children_without_overlap() {
        let parent = TileKey {
            z: 9,
            x: 268,
            y: 175,
        };
        let [a, b, c, d] = parent.children();
        let tiles = loaded(&[parent, a, c.children()[0], c.children()[3]]);
        let shown = compose(&tiles, &[a, b, c, d]);
        // `a` is loaded; `b`, `c` and `d` fall back to the parent, cut to each.
        assert_eq!(shown, {
            let mut s = vec![(a, a), (b, parent), (c, parent), (d, parent)];
            s.sort();
            s
        });
        let without_parent = loaded(&[a, c.children()[0], c.children()[3]]);
        let shown = compose(&without_parent, &[a, b, c]);
        let mut expected = vec![
            (a, a),
            (c.children()[0], c.children()[0]),
            (c.children()[3], c.children()[3]),
        ];
        expected.sort();
        assert_eq!(shown, expected);
    }

    #[test]
    fn gzip_tiles_are_unpacked() {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"tile").unwrap();
        let packed = encoder.finish().unwrap();
        assert_eq!(gunzip_if_needed(packed).unwrap(), b"tile");
        assert_eq!(gunzip_if_needed(b"raw".to_vec()).unwrap(), b"raw");
    }
}
