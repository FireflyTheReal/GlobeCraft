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
//!
//! Tiles come from a small disk cache first (`store`, in the compact format of
//! `compact`), and from the network only when missing or from an older tile set.
mod compact;
mod content;
mod join;
mod loading;
mod mvt;
mod pick;
mod store;

use super::{Layer, Line, MapLabel, Patch, StatusLog};
use content::{TileContent, TileKey};
use loading::Context;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Parallel tile downloads (OpenFreeMap asks bulk clients for at most 4).
const MAX_NETWORK_INFLIGHT: usize = 4;
/// Parallel reads from the disk cache.
const MAX_DISK_INFLIGHT: usize = 8;
/// Decoded tiles kept in memory, counted in vertices (16 bytes each as lines).
const MAX_STORED_POINTS: usize = 2_000_000;
const FAILED_RETRY: Duration = Duration::from_secs(60);
/// Levels above the view's whose surrounding ring is loaded ahead too.
const RING_PARENT_LEVELS: usize = 2;
/// The whole world is loaded ahead down to this zoom (1 + 4 + 16 + 64 tiles).
const WORLD_PRELOAD_MAX_ZOOM: u8 = 3;
/// Tiles of this zoom and above stay in memory: few and small, and what a quick zoom
/// out needs at once.
const PINNED_MAX_ZOOM: u8 = 6;
/// Tiles at least this deep hold municipal lines (OpenMapTiles adds `admin_level` 7–8
/// at zoom 11).
pub const MUNICIPAL_MIN_ZOOM: u8 = 11;

/// Border lines from the tiles on show, by administrative level.
#[derive(Clone, Debug, Default)]
pub struct OsmBorderLines {
    /// `admin_level` 2.
    pub country: Vec<Line>,
    /// `admin_level` 2 lines OpenMapTiles marks disputed (the neutral line), and with
    /// `OsmTileView::claims` each country's claimed line too.
    pub disputed: Vec<Line>,
    /// `admin_level` 3–4, of the big countries (see `BIG_COUNTRIES_A3`), sorted out when
    /// the lines reach the boundary data.
    pub state_big: Vec<Line>,
    /// `admin_level` 3–4 elsewhere.
    pub state: Vec<Line>,
    /// `admin_level` 5–8.
    pub municipal: Vec<Line>,
    /// The zoom level of the tiles wanted for the view.
    pub zoom: u8,
    /// Below this altitude (at the view's latitude) the tiles hold municipal lines;
    /// they and the village names fade in from here.
    pub municipal_max_km: f64,
}

impl OsmBorderLines {
    pub fn is_empty(&self) -> bool {
        self.country.is_empty()
            && self.disputed.is_empty()
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
    /// The disk cache's size limit (Settings → Cache).
    pub cache_limit_bytes: u64,
    /// Draw each country's claimed line of disputed borders too (Settings → Map style).
    pub claims: bool,
}

/// Ancestors cut to the missing tile they stand in for, by (shown tile, source tile).
type ClippedCache = Arc<Mutex<HashMap<(TileKey, TileKey), Arc<TileContent>>>>;

/// A decoded tile in memory.
struct Entry {
    content: Arc<TileContent>,
    used: Instant,
    /// The tile set it came from; empty for a cached tile of an older set.
    version: Arc<str>,
}

#[derive(Default)]
struct Store {
    /// The current tile set's URL template (from the network, or cached on disk).
    template: Option<Arc<str>>,
    /// The network confirmed the template this session.
    template_confirmed: bool,
    template_loading: bool,
    template_failed: Option<Instant>,
    cached_template_requested: bool,
    tiles: HashMap<TileKey, Entry>,
    points: usize,
    disk_inflight: HashSet<TileKey>,
    net_inflight: HashSet<TileKey>,
    /// Tiles already looked up on disk this session (found or not).
    disk_checked: HashSet<TileKey>,
    failed: HashMap<TileKey, Instant>,
    /// Bumped when the shown content may change.
    revision: u64,
}

impl Store {
    fn insert(&mut self, key: TileKey, entry: Entry) {
        self.points += entry.content.points;
        if let Some(old) = self.tiles.insert(key, entry) {
            self.points -= old.content.points;
        }
        self.revision += 1;
        if self.points <= MAX_STORED_POINTS {
            return;
        }
        // Forget the tiles the view left longest ago; the disk still has them.
        let mut by_age: Vec<(TileKey, Instant)> =
            self.tiles.iter().map(|(k, e)| (*k, e.used)).collect();
        by_age.sort_by_key(|(_, used)| *used);
        for (old, _) in by_age {
            if self.points <= MAX_STORED_POINTS * 3 / 4 {
                break;
            }
            if old == key || old.z <= PINNED_MAX_ZOOM {
                continue;
            }
            if let Some(entry) = self.tiles.remove(&old) {
                self.points -= entry.content.points;
                self.disk_checked.remove(&old);
            }
        }
    }

    fn is_current(&self, key: &TileKey) -> bool {
        match (self.tiles.get(key), &self.template) {
            (Some(entry), Some(template)) => entry.version == *template,
            _ => false,
        }
    }
}

pub(super) struct OsmTileStreamer {
    runtime: tokio::runtime::Handle,
    tx: mpsc::Sender<Patch>,
    context: Context,
    /// What was published last: each shown tile and the tile it came from.
    published: Vec<(TileKey, TileKey)>,
    published_revision: u64,
    published_claims: bool,
    /// The latest publication; older ones still being built are dropped.
    generation: Arc<AtomicU64>,
    /// Ancestors cut to a missing tile, shared with the publication jobs.
    clipped: ClippedCache,
    limit_bytes: u64,
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
            context: Context {
                store: Arc::default(),
                disk: Arc::new(store::DiskStore::in_data_folder()),
                log,
                repaint,
            },
            published: Vec::new(),
            published_revision: 0,
            published_claims: false,
            generation: Arc::default(),
            clipped: Arc::default(),
            limit_bytes: 0,
        }
    }

    /// Whether any tile is loaded: until then the other border layers stay on.
    pub(super) fn has_tiles(&self) -> bool {
        self.context
            .store
            .lock()
            .is_ok_and(|store| !store.tiles.is_empty())
    }

    /// Bytes the disk cache uses.
    pub(super) fn cache_usage(&self) -> u64 {
        self.context.disk.usage()
    }

    /// Follows the camera: loads the tiles around the view (disk first) and publishes
    /// the borders and names of the best tiles available. Cheap when nothing changed.
    pub(super) fn update(&mut self, view: OsmTileView) {
        if !view.enabled || !view.altitude_km.is_finite() {
            if !self.published.is_empty() {
                self.published.clear();
                self.generation.fetch_add(1, Ordering::AcqRel);
                send(&self.tx, OsmBorderLines::default(), Vec::new());
                (self.context.repaint)();
            }
            return;
        }
        if view.cache_limit_bytes < self.limit_bytes {
            let disk = self.context.disk.clone();
            let limit = view.cache_limit_bytes;
            self.runtime.spawn(async move {
                let _ = disk.shrink(limit).await;
            });
        }
        self.limit_bytes = view.cache_limit_bytes;
        let (zoom, wanted) = pick::wanted(view.lat, view.lon, view.altitude_km);
        let Ok(mut store) = self.context.store.lock() else {
            return;
        };
        let now = Instant::now();
        self.request_template(&mut store, now);
        let Some(template) = store.template.clone() else {
            return;
        };
        let ready = self.request_tiles(&mut store, &wanted, &template, now, true);
        if ready {
            let prefetch = prefetch(&wanted);
            self.request_tiles(&mut store, &prefetch, &template, now, false);
        }
        for key in &wanted {
            if let Some(entry) = store.tiles.get_mut(key) {
                entry.used = now;
            }
        }
        let shown = compose(&store.tiles, &wanted);
        if shown == self.published
            && store.revision == self.published_revision
            && view.claims == self.published_claims
        {
            return;
        }
        self.published_claims = view.claims;
        let contents: Vec<(TileKey, TileKey, Arc<TileContent>)> = shown
            .iter()
            .filter_map(|&(tile, source)| {
                Some((tile, source, store.tiles.get(&source)?.content.clone()))
            })
            .collect();
        self.published_revision = store.revision;
        drop(store);
        self.published = shown;
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let job = Publication {
            contents,
            zoom,
            municipal_max_km: pick::max_altitude_km_for_zoom(view.lat, MUNICIPAL_MIN_ZOOM),
            claims: view.claims,
            generation,
            latest: self.generation.clone(),
            clipped: self.clipped.clone(),
            tx: self.tx.clone(),
            repaint: self.context.repaint.clone(),
        };
        self.runtime.spawn(async move {
            let _ = crate::cpu::run(crate::cpu::Service::Features, move || {
                job.run();
                Ok(())
            })
            .await;
        });
    }

    /// The tile set's URL: the cached one at once, the network's when it answers.
    fn request_template(&self, store: &mut Store, now: Instant) {
        if !store.cached_template_requested {
            store.cached_template_requested = true;
            self.runtime
                .spawn(loading::cached_template(self.context.clone()));
        }
        let retry = store
            .template_failed
            .is_none_or(|at| now.duration_since(at) >= FAILED_RETRY);
        if !store.template_confirmed && !store.template_loading && retry {
            store.template_loading = true;
            self.runtime
                .spawn(loading::network_template(self.context.clone()));
        }
    }

    /// Starts loads for `keys` in order: a disk read for tiles not looked up yet, then
    /// a download for tiles missing or of an older set (`refresh`). Returns whether
    /// every key has some content to show.
    fn request_tiles(
        &self,
        store: &mut Store,
        keys: &[TileKey],
        template: &Arc<str>,
        now: Instant,
        refresh: bool,
    ) -> bool {
        let mut ready = true;
        // Missing tiles first, then stale ones.
        for stale_pass in [false, true] {
            for &key in keys {
                let present = store.tiles.contains_key(&key);
                if stale_pass != present {
                    continue;
                }
                ready &= present;
                if present && (!refresh || store.is_current(&key)) {
                    continue;
                }
                if store.disk_inflight.contains(&key) || store.net_inflight.contains(&key) {
                    continue;
                }
                if !store.disk_checked.contains(&key) {
                    if store.disk_inflight.len() < MAX_DISK_INFLIGHT {
                        store.disk_inflight.insert(key);
                        self.runtime.spawn(loading::from_disk(
                            self.context.clone(),
                            key,
                            template.clone(),
                        ));
                    }
                    continue;
                }
                let failed_recently = store
                    .failed
                    .get(&key)
                    .is_some_and(|at| now.duration_since(*at) < FAILED_RETRY);
                if failed_recently || store.net_inflight.len() >= MAX_NETWORK_INFLIGHT {
                    continue;
                }
                store.net_inflight.insert(key);
                self.runtime.spawn(loading::from_network(
                    self.context.clone(),
                    key,
                    template.clone(),
                    self.limit_bytes,
                ));
            }
        }
        ready
    }
}

/// Tiles worth loading ahead of need, most useful first:
/// - every level above the view, so a quick zoom out finds its tiles ready;
/// - the ring around the view and around its next `RING_PARENT_LEVELS` levels up, for
///   panning and for the wider area a zoom out shows;
/// - the whole world down to `WORLD_PRELOAD_MAX_ZOOM` (85 small tiles, kept on disk
///   after the first time), so the globe never waits for borders from far out.
fn prefetch(wanted: &[TileKey]) -> Vec<TileKey> {
    let set: HashSet<TileKey> = wanted.iter().copied().collect();
    let mut out: Vec<TileKey> = Vec::new();
    let mut seen = set.clone();
    let mut push = |key: TileKey, out: &mut Vec<TileKey>| {
        if seen.insert(key) {
            out.push(key);
        }
    };
    let mut levels: Vec<Vec<TileKey>> = vec![wanted.to_vec()];
    loop {
        let mut parents: Vec<TileKey> = Vec::new();
        for parent in levels
            .last()
            .into_iter()
            .flatten()
            .filter_map(|k| k.parent())
        {
            if !parents.contains(&parent) {
                parents.push(parent);
            }
        }
        if parents.is_empty() {
            break;
        }
        for &parent in &parents {
            push(parent, &mut out);
        }
        levels.push(parents);
    }
    for level in levels.iter().take(1 + RING_PARENT_LEVELS) {
        for key in level {
            let n = TileKey::tiles_per_side(key.z) as i64;
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let y = key.y as i64 + dy;
                if !(0..n).contains(&y) {
                    continue;
                }
                let neighbour = TileKey {
                    z: key.z,
                    x: (key.x as i64 + dx).rem_euclid(n) as u32,
                    y: y as u32,
                };
                push(neighbour, &mut out);
            }
        }
    }
    for z in 0..=WORLD_PRELOAD_MAX_ZOOM {
        let n = TileKey::tiles_per_side(z);
        for y in 0..n {
            for x in 0..n {
                push(TileKey { z, x, y }, &mut out);
            }
        }
    }
    out
}

/// Builds and sends one view's borders and names, off the UI thread.
struct Publication {
    contents: Vec<(TileKey, TileKey, Arc<TileContent>)>,
    zoom: u8,
    municipal_max_km: f64,
    claims: bool,
    generation: u64,
    latest: Arc<AtomicU64>,
    clipped: ClippedCache,
    tx: mpsc::Sender<Patch>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}

impl Publication {
    fn run(self) {
        let stale = || self.latest.load(Ordering::Acquire) != self.generation;
        let mut shown_pairs = HashSet::new();
        let contents: Vec<Arc<TileContent>> = self
            .contents
            .iter()
            .map(|(tile, source, content)| {
                if tile == source {
                    return content.clone();
                }
                shown_pairs.insert((*tile, *source));
                let cached = self
                    .clipped
                    .lock()
                    .ok()
                    .and_then(|c| c.get(&(*tile, *source)).cloned());
                cached.unwrap_or_else(|| {
                    let cut = Arc::new(content.clipped(tile.bounds()));
                    if let Ok(mut cache) = self.clipped.lock() {
                        cache.insert((*tile, *source), cut.clone());
                    }
                    cut
                })
            })
            .collect();
        if let Ok(mut cache) = self.clipped.lock() {
            cache.retain(|key, _| shown_pairs.contains(key));
        }
        if stale() {
            return;
        }
        let tolerance = join::tolerance_deg(self.zoom);
        let gather = |pick: fn(&TileContent) -> &Vec<Line>| -> Vec<Line> {
            let lines: Vec<Line> = contents
                .iter()
                .flat_map(|c| pick(c).iter().cloned())
                .collect();
            join::join(lines, tolerance)
        };
        let lines = OsmBorderLines {
            country: gather(|c| &c.country),
            disputed: {
                let mut lines: Vec<Line> = contents
                    .iter()
                    .flat_map(|c| c.disputed.iter().cloned())
                    .collect();
                if self.claims {
                    lines.extend(contents.iter().flat_map(|c| c.claims.iter().cloned()));
                }
                join::join(lines, tolerance)
            },
            state: gather(|c| &c.state),
            municipal: gather(|c| &c.municipal),
            state_big: Vec::new(),
            zoom: self.zoom,
            municipal_max_km: self.municipal_max_km,
        };
        let places: Vec<MapLabel> = contents
            .iter()
            .flat_map(|c| c.places.iter().cloned())
            .collect();
        if stale() {
            return;
        }
        send(&self.tx, lines, places);
        (self.repaint)();
    }
}

fn send(tx: &mpsc::Sender<Patch>, lines: OsmBorderLines, places: Vec<MapLabel>) {
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
    let _ = tx.send(patch(Layer::OsmBorders, Vec::new(), Some(lines)));
    let _ = tx.send(patch(Layer::Places, places, None));
}

/// For each wanted tile, the tile whose content shows there: itself once loaded, else
/// its nearest loaded ancestor (cut to it), else its loaded children. So a view never
/// goes blank while a new zoom level loads, and no area is covered twice.
fn compose(tiles: &HashMap<TileKey, Entry>, wanted: &[TileKey]) -> Vec<(TileKey, TileKey)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(keys: &[TileKey]) -> HashMap<TileKey, Entry> {
        keys.iter()
            .map(|&k| {
                let entry = Entry {
                    content: Arc::default(),
                    used: Instant::now(),
                    version: Arc::from("v1"),
                };
                (k, entry)
            })
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
    fn prefetch_covers_parents_and_the_ring_around_the_view() {
        let centre = TileKey {
            z: 10,
            x: 535,
            y: 350,
        };
        let around = prefetch(&[centre]);
        // Every level above, the world overview, and parents before neighbours.
        let mut ancestor = centre;
        while let Some(parent) = ancestor.parent() {
            assert!(around.contains(&parent), "{parent:?}");
            ancestor = parent;
        }
        assert!(around.contains(&TileKey { z: 3, x: 7, y: 7 }));
        let position = |key| around.iter().position(|k| *k == key).unwrap();
        assert!(position(centre.parent().unwrap()) < position(TileKey { x: 536, ..centre }));
        assert!(around.contains(&TileKey { x: 536, ..centre }));
        assert!(!around.contains(&centre));
        // Across the date line.
        let east = TileKey { z: 3, x: 7, y: 3 };
        assert!(prefetch(&[east]).contains(&TileKey { x: 0, ..east }));
    }

    #[test]
    fn memory_holds_a_vertex_budget_and_forgets_the_oldest_tiles() {
        let mut store = Store::default();
        let heavy = |x: u32| {
            let content = TileContent {
                points: MAX_STORED_POINTS / 4,
                ..Default::default()
            };
            (
                TileKey { z: 12, x, y: 1 },
                Entry {
                    content: Arc::new(content),
                    used: Instant::now(),
                    version: Arc::from("v1"),
                },
            )
        };
        for x in 0..10 {
            let (key, entry) = heavy(x);
            store.disk_checked.insert(key);
            store.insert(key, entry);
        }
        assert!(store.points <= MAX_STORED_POINTS, "{}", store.points);
        assert!(store.tiles.contains_key(&TileKey { z: 12, x: 9, y: 1 }));
        assert!(!store.tiles.contains_key(&TileKey { z: 12, x: 0, y: 1 }));
        // An evicted tile is read from disk again when needed.
        assert!(!store.disk_checked.contains(&TileKey { z: 12, x: 0, y: 1 }));
    }

    #[test]
    fn gzip_tiles_are_unpacked() {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"tile").unwrap();
        let packed = encoder.finish().unwrap();
        assert_eq!(loading::gunzip_if_needed(packed).unwrap(), b"tile");
        assert_eq!(loading::gunzip_if_needed(b"raw".to_vec()).unwrap(), b"raw");
    }
}
