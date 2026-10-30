//! Streaming one of GlobeCraft's map data sets (`crate::map_data`) tile by tile for the
//! view: a zoom level for the altitude, the tiles around the view nearest first, a few
//! downloads at a time, decoded on the feature pool, kept in memory, and coarser loaded
//! tiles drawn while finer ones arrive. Used by the transport and road layers.
use crate::boundaries::osm_tiles::content::TileKey;
use crate::boundaries::osm_tiles::pick::{around, ideal_zoom, view_radius_deg};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tiles downloaded in one batch. A batch shares archive lookups and coalesced byte
/// ranges, so a view's 64 tiles take a few requests instead of one per tile.
const MAX_BATCH_TILES: usize = 24;
const FAILED_RETRY: Duration = Duration::from_secs(30);
const TEMPLATE_RETRY: Duration = Duration::from_secs(60);
/// Decoded vector tiles kept per set are bounded by both count and this estimated
/// heap budget. The estimate counts owned names and coordinate capacity, not the
/// allocator's overhead.
pub(crate) const MAX_VECTOR_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// How one set is streamed.
pub(crate) struct Spec<T> {
    /// The manifest entry.
    pub(crate) set: &'static str,
    /// Further entries holding more zooms of the same set (see
    /// `map_data::banded_template`).
    pub(crate) bands: &'static [&'static str],
    /// The coarsest and finest levels fetched.
    pub(crate) min_zoom: u8,
    pub(crate) max_zoom: u8,
    /// A level with nothing the next finer one lacks: never fetched.
    pub(crate) skip_zoom: Option<u8>,
    pub(crate) tiles_across_view: f64,
    /// More tiles than this go one level up instead.
    pub(crate) max_tiles: usize,
    /// The most tiles taken to stay at a caller's floor level.
    pub(crate) max_floor_tiles: usize,
    /// Decoded tiles kept in memory.
    pub(crate) max_cached_tiles: usize,
    /// Estimated decoded memory the tiles may occupy before the least recently used
    /// are dropped.
    pub(crate) max_cached_bytes: usize,
    /// Estimated heap size of one decoded tile.
    pub(crate) size_bytes: fn(&T) -> usize,
    /// A decompressed vector tile's content.
    pub(crate) decode: fn(TileKey, &[u8]) -> Option<T>,
}

/// The tiles to draw for the current view: wanted tiles that are loaded, and coarser
/// loaded tiles in place of those still loading.
#[derive(Debug)]
pub struct Snapshot<T> {
    pub tiles: Vec<Arc<T>>,
    /// The level the view wants.
    pub zoom: u8,
}

/// Where and how the view looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
    pub(crate) altitude_km: f64,
    pub(crate) tilt_deg: f64,
    /// Tiles at least this fine (e.g. the level a layer's features start at).
    pub(crate) floor: u8,
}

struct Shared<T> {
    template: Option<Arc<str>>,
    template_loading: bool,
    template_failed: Option<Instant>,
    template_error: Option<String>,
    tiles: HashMap<TileKey, Cached<T>>,
    inflight: HashSet<TileKey>,
    failed: HashMap<TileKey, Instant>,
    /// Bumped whenever a tile arrives.
    revision: u64,
}

struct Cached<T> {
    tile: Arc<T>,
    used: Instant,
    /// Estimated decoded heap size, see `Spec::size_bytes`.
    bytes: usize,
}

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Self {
            template: None,
            template_loading: false,
            template_failed: None,
            template_error: None,
            tiles: HashMap::new(),
            inflight: HashSet::new(),
            failed: HashMap::new(),
            revision: 0,
        }
    }
}

/// One set streamed for the view: call [`Self::update`] every frame it is shown.
pub(crate) struct TileStream<T: 'static> {
    spec: &'static Spec<T>,
    shared: Arc<Mutex<Shared<T>>>,
    snapshot: Option<Arc<Snapshot<T>>>,
    /// What the snapshot was built from: the wanted tiles and the revision.
    built: Option<(Vec<TileKey>, u64)>,
}

impl<T: Send + Sync + 'static> TileStream<T> {
    pub(crate) fn new(spec: &'static Spec<T>) -> Self {
        Self {
            spec,
            shared: Arc::default(),
            snapshot: None,
            built: None,
        }
    }

    /// The tiles to draw, when something is loaded.
    pub(crate) fn shared_snapshot(&self) -> Option<Arc<Snapshot<T>>> {
        self.snapshot
            .clone()
            .filter(|snapshot| !snapshot.tiles.is_empty())
    }

    /// Why nothing shows yet, if something is wrong.
    pub(crate) fn error(&self) -> Option<String> {
        self.shared.lock().ok()?.template_error.clone()
    }

    /// Whether downloads are running.
    pub(crate) fn loading(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|shared| !shared.inflight.is_empty())
    }

    /// Forgets the view (the layer is off or out of range).
    pub(crate) fn hide(&mut self) {
        self.snapshot = None;
        self.built = None;
    }

    pub(crate) fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        view: View,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        let spec = self.spec;
        let (zoom, wanted) = wanted(spec, view);
        let now = Instant::now();
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let Some(template) = shared.template.clone() else {
            let due = shared
                .template_failed
                .is_none_or(|at| now.duration_since(at) >= TEMPLATE_RETRY);
            if !shared.template_loading && due {
                shared.template_loading = true;
                let state = self.shared.clone();
                runtime.spawn(async move {
                    let result = crate::map_data::banded_template(spec.set, spec.bands).await;
                    if let Ok(mut shared) = state.lock() {
                        shared.template_loading = false;
                        match result {
                            Ok(template) => {
                                shared.template = Some(template.into());
                                shared.template_error = None;
                            }
                            Err(error) => {
                                shared.template_failed = Some(Instant::now());
                                shared.template_error = Some(error.to_string());
                            }
                        }
                    }
                    repaint();
                });
            }
            return;
        };
        // Downloads for missing tiles, nearest first. One batch of tiles shares its
        // archive lookups and coalesced byte ranges instead of one request each.
        let mut batch = Vec::new();
        for key in &wanted {
            // `inflight` already holds the batch's keys, so it bounds the whole batch.
            if shared.inflight.len() >= MAX_BATCH_TILES {
                break;
            }
            let retry = shared
                .failed
                .get(key)
                .is_none_or(|at| now.duration_since(*at) >= FAILED_RETRY);
            if shared.tiles.contains_key(key) || shared.inflight.contains(key) || !retry {
                continue;
            }
            shared.inflight.insert(*key);
            batch.push(*key);
        }
        if !batch.is_empty() {
            let (state, template, repaint) =
                (self.shared.clone(), template.clone(), repaint.clone());
            runtime.spawn(async move {
                let result = fetch_many(spec, &batch, &template).await;
                if let Ok(mut shared) = state.lock() {
                    let mut remaining: HashSet<TileKey> = batch.iter().copied().collect();
                    let mut arrived = false;
                    if let Ok(tiles) = result {
                        for (key, tile) in tiles {
                            remaining.remove(&key);
                            shared.inflight.remove(&key);
                            match tile {
                                Ok(tile) => {
                                    shared.failed.remove(&key);
                                    let bytes = (spec.size_bytes)(&tile);
                                    shared.tiles.insert(
                                        key,
                                        Cached {
                                            tile: Arc::new(tile),
                                            used: Instant::now(),
                                            bytes,
                                        },
                                    );
                                    arrived = true;
                                }
                                Err(_) => {
                                    shared.failed.insert(key, Instant::now());
                                }
                            }
                        }
                    }
                    for key in remaining {
                        shared.inflight.remove(&key);
                        shared.failed.insert(key, Instant::now());
                    }
                    if arrived {
                        shared.revision += 1;
                        evict(
                            &mut shared.tiles,
                            spec.max_cached_tiles,
                            spec.max_cached_bytes,
                        );
                    }
                }
                repaint();
            });
        }
        if self
            .built
            .as_ref()
            .is_some_and(|(keys, revision)| *keys == wanted && *revision == shared.revision)
        {
            return;
        }
        // Each wanted tile, or its nearest loaded ancestor while it loads.
        let mut chosen: Vec<TileKey> = Vec::new();
        for key in &wanted {
            let mut candidate = Some(*key);
            while let Some(at) = candidate {
                if shared.tiles.contains_key(&at) {
                    if !chosen.contains(&at) {
                        chosen.push(at);
                    }
                    break;
                }
                candidate = at.parent().filter(|parent| parent.z >= spec.min_zoom);
            }
        }
        let tiles = chosen
            .iter()
            .filter_map(|key| {
                let entry = shared.tiles.get_mut(key)?;
                entry.used = now;
                Some(entry.tile.clone())
            })
            .collect();
        self.built = Some((wanted, shared.revision));
        self.snapshot = Some(Arc::new(Snapshot { tiles, zoom }));
    }
}

/// The zoom level and the tiles around the view, nearest first.
pub(crate) fn wanted<T>(spec: &Spec<T>, view: View) -> (u8, Vec<TileKey>) {
    let floor = view.floor.clamp(spec.min_zoom, spec.max_zoom);
    let skip = |zoom: u8| spec.skip_zoom == Some(zoom);
    let mut zoom = ideal_zoom(view.latitude, view.altitude_km, spec.tiles_across_view)
        .round()
        .clamp(f64::from(floor), f64::from(spec.max_zoom)) as u8;
    if skip(zoom) && zoom < spec.max_zoom {
        zoom += 1;
    }
    let radius = view_radius_deg(view.altitude_km, view.tilt_deg);
    loop {
        let tiles = around(zoom, view.latitude, view.longitude, radius);
        let limit = if zoom <= floor {
            spec.max_floor_tiles
        } else {
            spec.max_tiles
        };
        if tiles.len() <= limit || zoom <= spec.min_zoom {
            return (zoom, tiles.into_iter().take(spec.max_floor_tiles).collect());
        }
        zoom -= 1;
        if skip(zoom) && zoom > spec.min_zoom {
            zoom -= 1;
        }
    }
}

/// Drops the tiles used longest ago until both the count and the estimated heap
/// budget fit. Tiles are never evicted below one, even when a single tile exceeds the
/// byte target, so a view can still draw.
fn evict<T>(tiles: &mut HashMap<TileKey, Cached<T>>, limit: usize, max_bytes: usize) {
    let mut total_bytes: usize = tiles.values().map(|cached| cached.bytes).sum();
    if tiles.len() <= limit && total_bytes <= max_bytes {
        return;
    }
    let mut ages: Vec<(Instant, TileKey)> = tiles
        .iter()
        .map(|(key, cached)| (cached.used, *key))
        .collect();
    ages.sort();
    for (_, key) in ages {
        if tiles.len() <= 1 || (tiles.len() <= limit && total_bytes <= max_bytes) {
            break;
        }
        if let Some(cached) = tiles.remove(&key) {
            total_bytes = total_bytes.saturating_sub(cached.bytes);
        }
    }
}

/// Fetches and decodes several tiles at once. Their archive locations and exactly
/// contiguous byte ranges are read in as few requests as possible; decoding stays on
/// the feature pool. One failed tile does not discard the others.
pub(crate) async fn fetch_many<T: Send + 'static>(
    spec: &'static Spec<T>,
    keys: &[TileKey],
    template: &str,
) -> Result<Vec<(TileKey, Result<T, crate::Error>)>, crate::Error> {
    let archives = template
        .strip_prefix(crate::map_data::PMTILES_PREFIX)
        .ok_or("map data tiles need GlobeCraft's archives")?
        .to_owned();
    let tiles: Vec<(u8, u32, u32)> = keys.iter().map(|key| (key.z, key.x, key.y)).collect();
    let parts = crate::map_data::tile_parts_many(&tiles, &archives).await?;
    let keys = keys.to_vec();
    crate::cpu::run(crate::cpu::Service::Features, move || {
        Ok::<_, crate::Error>(
            keys.into_iter()
                .zip(parts)
                .map(|(key, parts)| {
                    let result = (|| {
                        let mut bytes = Vec::new();
                        for part in parts {
                            bytes.extend(crate::map_data::gunzip_if_needed(part)?);
                        }
                        (spec.decode)(key, &bytes).ok_or_else(|| {
                            crate::Error::from(format!(
                                "malformed {} tile {}/{}/{}",
                                spec.set, key.z, key.x, key.y
                            ))
                        })
                    })();
                    (key, result)
                })
                .collect(),
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached(used: Instant, bytes: usize) -> Cached<()> {
        Cached {
            tile: Arc::new(()),
            used,
            bytes,
        }
    }

    #[test]
    fn eviction_drops_oldest_tiles_until_count_and_bytes_fit() {
        let now = Instant::now();
        let mut tiles = HashMap::new();
        for x in 0..4 {
            tiles.insert(
                TileKey { z: 4, x, y: 0 },
                cached(now + Duration::from_secs(u64::from(x)), 10),
            );
        }
        // Count still fits but 40 estimated bytes do not: the two oldest go.
        evict(&mut tiles, 4, 25);
        assert_eq!(tiles.len(), 2);
        assert!(!tiles.contains_key(&TileKey { z: 4, x: 0, y: 0 }));
        assert!(!tiles.contains_key(&TileKey { z: 4, x: 1, y: 0 }));
        assert!(tiles.contains_key(&TileKey { z: 4, x: 2, y: 0 }));
        // A single tile above the target is kept so the view can still draw.
        let mut one = HashMap::new();
        one.insert(TileKey { z: 4, x: 0, y: 0 }, cached(now, 100));
        evict(&mut one, 10, 50);
        assert_eq!(one.len(), 1);
    }
}
