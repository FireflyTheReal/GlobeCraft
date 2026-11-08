//! Streaming one of GlobeCraft's map data sets (`crate::map_data`) tile by tile for the
//! view: a zoom level for the altitude, the tiles around the view nearest first, a few
//! downloads at a time, decoded on the feature pool, kept in memory, and coarser loaded
//! tiles drawn while finer ones arrive. Used by the transport, road, place name and
//! point of interest layers.
//!
//! Tiles are kept on disk too (roadmap 3.4), one SQLite file per set in the cache
//! folder, as downloaded, under the limit `set_disk_cache_limit` sets (the border tile
//! cache's, in Settings -> Cache). A tile on disk shows at once; one of an older
//! build of the set shows too and is downloaded again.
use crate::boundaries::osm_tiles::content::TileKey;
use crate::boundaries::osm_tiles::pick::{around, ideal_zoom, view_radius_deg};
use crate::boundaries::osm_tiles::store::DiskStore;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the disk rows hold: a vector tile, gzip-compressed (rows of another format are
/// ignored, so a change of storage starts the files afresh).
const DISK_FORMAT: u8 = 1;
/// Until the app sets it: the border tile cache's default.
const DEFAULT_DISK_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
static DISK_LIMIT_BYTES: AtomicU64 = AtomicU64::new(DEFAULT_DISK_LIMIT_BYTES);

/// Each vector set's disk cache stays under this many bytes (applied at the next
/// download, and at once to a smaller limit).
pub fn set_disk_cache_limit(bytes: u64) {
    DISK_LIMIT_BYTES.store(bytes, Ordering::Relaxed);
}

/// The vector sets cached on disk.
const DISK_SETS: [&str; 4] = ["transport", "roads", "poi", "places"];

fn disk_path(set: &str) -> std::path::PathBuf {
    crate::paths::cache_dir().join(format!("map_data_{set}.sqlite"))
}

/// The bytes the vector sets' disk caches hold now (their files' sizes).
pub fn disk_cache_usage() -> u64 {
    DISK_SETS
        .iter()
        .filter_map(|set| std::fs::metadata(disk_path(set)).ok())
        .map(|meta| meta.len())
        .sum()
}

/// Tiles downloaded in one batch. A batch shares archive lookups and coalesced byte
/// ranges, so a view's 64 tiles take a few requests instead of one per tile.
const MAX_BATCH_TILES: usize = 8;
/// Tiles downloading at once, in several batches: the nearest tiles appear as soon as
/// their batch is in, instead of waiting for one large batch.
const MAX_INFLIGHT_TILES: usize = 24;
/// A tile that could not be decoded is tried again after this long.
const FAILED_RETRY: Duration = Duration::from_secs(30);
/// A batch that failed as a whole (a network error, an archive that did not answer)
/// is tried again soon: its tiles are most likely fine.
const BATCH_RETRY: Duration = Duration::from_secs(4);
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
    /// A level never fetched (the next finer one is used): one with nothing the
    /// finer one lacks, or whose data is too thinned out to show.
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

/// How long a tile takes to fade in once it is shown.
const FADE_IN: Duration = Duration::from_millis(450);
/// A replaced tile stays fully drawn this long (its replacement fades in over it),
/// then fades out over `FADE_OUT`, as the borders exchange their sets.
const FADE_OUT_DELAY: Duration = Duration::from_millis(250);
const FADE_OUT: Duration = Duration::from_millis(350);

/// When a tile was shown and, once replaced, when it was: its drawing fades in after
/// the first and out after the second, wherever the view is meanwhile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TileFade {
    /// `None`: shown long enough ago to be fully drawn.
    pub shown: Option<Instant>,
    pub retired: Option<Instant>,
}

impl TileFade {
    /// The tile's opacity at `now`.
    pub fn opacity(&self, now: Instant) -> f32 {
        let smooth = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let since = |at: Instant| now.saturating_duration_since(at).as_secs_f32();
        let fade_in = self
            .shown
            .map_or(1.0, |at| smooth(since(at) / FADE_IN.as_secs_f32()));
        let fade_out = self.retired.map_or(1.0, |at| {
            1.0 - smooth((since(at) - FADE_OUT_DELAY.as_secs_f32()) / FADE_OUT.as_secs_f32())
        });
        fade_in * fade_out
    }
    /// Whether the opacity still changes.
    pub fn animating(&self, now: Instant) -> bool {
        self.shown
            .is_some_and(|at| now.saturating_duration_since(at) < FADE_IN)
            || self.retired.is_some()
    }
    /// Whether a replaced tile has faded out completely.
    fn gone(&self, now: Instant) -> bool {
        self.retired
            .is_some_and(|at| now.saturating_duration_since(at) >= FADE_OUT_DELAY + FADE_OUT)
    }
}

/// The tiles to draw for the current view: wanted tiles that are loaded, coarser
/// loaded tiles in place of those still loading, and replaced tiles fading out.
#[derive(Debug)]
pub struct Snapshot<T> {
    pub tiles: Vec<Arc<T>>,
    /// The level the view wants.
    pub zoom: u8,
    /// Each tile's fade, in the order of `tiles` (missing: fully drawn).
    pub fades: Vec<TileFade>,
}

impl<T> Snapshot<T> {
    /// The fade of tile `index`.
    pub fn fade(&self, index: usize) -> TileFade {
        self.fades.get(index).copied().unwrap_or_default()
    }
}

/// A tile on show, for the fades.
struct Shown<T> {
    tile: Arc<T>,
    fade: TileFade,
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
    /// When a failed tile may be requested again.
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
    disk: Arc<DiskStore>,
    /// The disk limit last applied, to shrink the file when it gets smaller.
    disk_limit: u64,
    snapshot: Option<Arc<Snapshot<T>>>,
    /// What the snapshot was built from: the wanted tiles and the revision.
    built: Option<(Vec<TileKey>, u64)>,
    /// The tiles on show, current and fading out.
    shown: HashMap<TileKey, Shown<T>>,
}

impl<T: Send + Sync + 'static> TileStream<T> {
    pub(crate) fn new(spec: &'static Spec<T>) -> Self {
        debug_assert!(
            DISK_SETS.contains(&spec.set),
            "{} has no disk file",
            spec.set
        );
        Self::with_disk(spec, DiskStore::at(disk_path(spec.set), DISK_FORMAT))
    }

    fn with_disk(spec: &'static Spec<T>, disk: DiskStore) -> Self {
        Self {
            spec,
            shared: Arc::default(),
            disk: Arc::new(disk),
            disk_limit: DISK_LIMIT_BYTES.load(Ordering::Relaxed),
            snapshot: None,
            built: None,
            shown: HashMap::new(),
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
        self.shown.clear();
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
        let limit = DISK_LIMIT_BYTES.load(Ordering::Relaxed);
        if limit < self.disk_limit {
            let disk = self.disk.clone();
            runtime.spawn(async move {
                let _ = disk.shrink(limit).await;
            });
        }
        self.disk_limit = limit;
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let Some(template) = shared.template.clone() else {
            let due = shared
                .template_failed
                .is_none_or(|at| now.duration_since(at) >= TEMPLATE_RETRY);
            if !shared.template_loading && due {
                shared.template_loading = true;
                let (state, disk) = (self.shared.clone(), self.disk.clone());
                runtime.spawn(async move {
                    let result = crate::map_data::banded_template(spec.set, spec.bands).await;
                    // Offline, the set last read still shows what the disk holds.
                    let result = match result {
                        Ok(template) => {
                            let _ = disk.set_template(template.clone()).await;
                            Ok(template)
                        }
                        Err(error) => match disk.template().await {
                            Ok(Some(template)) => Ok(template),
                            _ => Err(error),
                        },
                    };
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
        // Downloads for missing tiles, nearest first, in batches that share their
        // archive lookups and coalesced byte ranges.
        let mut batch = Vec::new();
        for key in &wanted {
            if shared.inflight.len() >= MAX_INFLIGHT_TILES {
                break;
            }
            let retry = shared.failed.get(key).is_none_or(|at| now >= *at);
            if shared.tiles.contains_key(key) || shared.inflight.contains(key) || !retry {
                continue;
            }
            shared.inflight.insert(*key);
            batch.push(*key);
            if batch.len() == MAX_BATCH_TILES {
                self.spawn_batch(runtime, std::mem::take(&mut batch), &template, &repaint);
            }
        }
        if !batch.is_empty() {
            self.spawn_batch(runtime, batch, &template, &repaint);
        }
        let expired = self.shown.values().any(|shown| shown.fade.gone(now));
        if !expired
            && self
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
        // New tiles fade in; tiles no longer chosen fade out, then go.
        for key in &chosen {
            let Some(entry) = shared.tiles.get_mut(key) else {
                continue;
            };
            entry.used = now;
            let tile = entry.tile.clone();
            match self.shown.get_mut(key) {
                Some(shown) => {
                    shown.tile = tile;
                    if shown.fade.retired.take().is_some() {
                        // Back before it had gone: shown in full again.
                        shown.fade.shown = None;
                    }
                }
                None => {
                    self.shown.insert(
                        *key,
                        Shown {
                            tile,
                            fade: TileFade {
                                shown: Some(now),
                                retired: None,
                            },
                        },
                    );
                }
            }
        }
        for (key, shown) in &mut self.shown {
            if !chosen.contains(key) && shown.fade.retired.is_none() {
                shown.fade.retired = Some(now);
            }
            // Fully faded in: no longer animating.
            if shown
                .fade
                .shown
                .is_some_and(|at| now.saturating_duration_since(at) >= FADE_IN)
            {
                shown.fade.shown = None;
            }
        }
        self.shown.retain(|_, shown| !shown.fade.gone(now));
        let retiring = self
            .shown
            .iter()
            .filter(|(key, _)| !chosen.contains(key))
            .map(|(_, shown)| (shown.tile.clone(), shown.fade));
        let (tiles, fades) = chosen
            .iter()
            .filter_map(|key| self.shown.get(key).map(|s| (s.tile.clone(), s.fade)))
            .chain(retiring)
            .unzip();
        self.built = Some((wanted, shared.revision));
        self.snapshot = Some(Arc::new(Snapshot { tiles, zoom, fades }));
    }
}

impl<T: Send + Sync + 'static> TileStream<T> {
    /// Loads a batch: from disk what is there, then from the network what was not
    /// (or was of an older build), stored to disk as it arrives.
    fn spawn_batch(
        &self,
        runtime: &crate::runtime::ServiceRuntime,
        batch: Vec<TileKey>,
        template: &Arc<str>,
        repaint: &(impl Fn() + Send + Sync + Clone + 'static),
    ) {
        let spec = self.spec;
        let (state, disk, template, repaint) = (
            self.shared.clone(),
            self.disk.clone(),
            template.clone(),
            repaint.clone(),
        );
        runtime.spawn(async move {
            let mut stored = Vec::new();
            let mut download = Vec::new();
            for &key in &batch {
                match disk.read(key, template.to_string()).await {
                    Ok(Some(tile)) => {
                        if !tile.current {
                            download.push(key);
                        }
                        stored.push((key, tile.bytes));
                    }
                    _ => download.push(key),
                }
            }
            if !stored.is_empty() {
                let decoded = crate::cpu::run(crate::cpu::Service::Features, move || {
                    Ok::<_, crate::Error>(
                        stored
                            .into_iter()
                            .filter_map(|(key, bytes)| {
                                let bytes = crate::map_data::gunzip_if_needed(bytes).ok()?;
                                Some((key, (spec.decode)(key, &bytes)?))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .await
                .unwrap_or_default();
                if let Ok(mut shared) = state.lock() {
                    let now = Instant::now();
                    for (key, tile) in decoded {
                        if !download.contains(&key) {
                            shared.inflight.remove(&key);
                        }
                        shared.failed.remove(&key);
                        insert(&mut shared, spec, key, tile, now);
                    }
                    shared.revision += 1;
                    evict(
                        &mut shared.tiles,
                        spec.max_cached_tiles,
                        spec.max_cached_bytes,
                    );
                }
                repaint();
            }
            // A stored tile that did not decode is downloaded again.
            if let Ok(shared) = state.lock() {
                for key in &batch {
                    if !download.contains(key) && !shared.tiles.contains_key(key) {
                        download.push(*key);
                    }
                }
                download.retain(|key| shared.inflight.contains(key));
            }
            if download.is_empty() {
                return;
            }
            let result = fetch_many_stored(spec, &download, &template).await;
            let mut to_store = Vec::new();
            if let Ok(mut shared) = state.lock() {
                let now = Instant::now();
                let mut arrived = false;
                // Every key of the batch is settled here, whatever came back.
                for key in &download {
                    shared.inflight.remove(key);
                }
                match result {
                    Ok(tiles) => {
                        for (key, tile) in tiles {
                            match tile {
                                Ok((tile, bytes)) => {
                                    shared.failed.remove(&key);
                                    insert(&mut shared, spec, key, tile, now);
                                    to_store.push((key, bytes));
                                    arrived = true;
                                }
                                // A tile shown from disk stays; it is tried again later.
                                Err(_) if shared.tiles.contains_key(&key) => {}
                                Err(_) => {
                                    shared.failed.insert(key, now + FAILED_RETRY);
                                }
                            }
                        }
                    }
                    Err(_) => {
                        for key in &download {
                            if !shared.tiles.contains_key(key) {
                                shared.failed.insert(*key, now + BATCH_RETRY);
                            }
                        }
                    }
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
            let limit = DISK_LIMIT_BYTES.load(Ordering::Relaxed);
            for (key, bytes) in to_store {
                let _ = disk.write(key, template.to_string(), bytes, limit).await;
            }
        });
    }
}

fn insert<T>(shared: &mut Shared<T>, spec: &Spec<T>, key: TileKey, tile: T, now: Instant) {
    let bytes = (spec.size_bytes)(&tile);
    shared.tiles.insert(
        key,
        Cached {
            tile: Arc::new(tile),
            used: now,
            bytes,
        },
    );
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
#[cfg(test)]
pub(crate) async fn fetch_many<T: Send + 'static>(
    spec: &'static Spec<T>,
    keys: &[TileKey],
    template: &str,
) -> Result<Vec<(TileKey, Result<T, crate::Error>)>, crate::Error> {
    Ok(fetch_many_stored(spec, keys, template)
        .await?
        .into_iter()
        .map(|(key, tile)| (key, tile.map(|(tile, _)| tile)))
        .collect())
}

/// [`fetch_many`], with each tile's bytes as the disk cache keeps them (gzip).
async fn fetch_many_stored<T: Send + 'static>(
    spec: &'static Spec<T>,
    keys: &[TileKey],
    template: &str,
) -> Result<Vec<(TileKey, Result<(T, Vec<u8>), crate::Error>)>, crate::Error> {
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
                        let tile = (spec.decode)(key, &bytes).ok_or_else(|| {
                            crate::Error::from(format!(
                                "malformed {} tile {}/{}/{}",
                                spec.set, key.z, key.x, key.y
                            ))
                        })?;
                        Ok((tile, gzip(&bytes)?))
                    })();
                    (key, result)
                })
                .collect(),
        )
    })
    .await
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, crate::Error> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(
        Vec::with_capacity(bytes.len() / 3),
        flate2::Compression::fast(),
    );
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
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

    /// A tile's content in the test set: the bytes it was decoded from.
    static TEST_SPEC: Spec<Vec<u8>> = Spec {
        set: "test",
        bands: &[],
        min_zoom: 4,
        max_zoom: 10,
        skip_zoom: None,
        tiles_across_view: 3.0,
        max_tiles: 64,
        max_floor_tiles: 64,
        max_cached_tiles: 600,
        max_cached_bytes: MAX_VECTOR_CACHE_BYTES,
        size_bytes: Vec::len,
        decode: |_, bytes| Some(bytes.to_vec()),
    };

    /// Tiles on disk show without a download: the stream's first update after the set
    /// is known reads them (the template is an address no request could reach).
    #[test]
    fn tiles_on_disk_show_without_downloading() {
        let dir =
            std::env::temp_dir().join(format!("globecraft-vector-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let view = View {
            latitude: 48.1,
            longitude: 11.6,
            altitude_km: 400.0,
            tilt_deg: 0.0,
            floor: 4,
        };
        let (_, keys) = wanted(&TEST_SPEC, view);
        let template = "pmtiles:http://127.0.0.1:9/unreachable-{z}.pmtiles";
        let disk = DiskStore::at(dir.join("test.sqlite"), DISK_FORMAT);
        runtime.handle().block_on(async {
            for key in &keys {
                let bytes = gzip(&[key.z, key.x as u8, key.y as u8]).unwrap();
                disk.write(*key, template.to_owned(), bytes, u64::MAX)
                    .await
                    .unwrap();
            }
        });
        let mut stream = TileStream::with_disk(&TEST_SPEC, disk);
        stream.shared.lock().unwrap().template = Some(template.into());
        let deadline = Instant::now() + Duration::from_secs(10);
        let shown = loop {
            stream.update(&runtime, view, || {});
            let count = stream
                .shared_snapshot()
                .map_or(0, |snapshot| snapshot.tiles.len());
            if count == keys.len() || Instant::now() > deadline {
                break count;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(shown, keys.len());
        assert!(!stream.loading(), "nothing was left to download");
        let snapshot = stream.shared_snapshot().unwrap();
        let first = &keys[0];
        assert!(
            snapshot
                .tiles
                .iter()
                .any(|tile| **tile == [first.z, first.x as u8, first.y as u8])
        );
        drop(stream);
        drop(runtime);
        let _ = std::fs::remove_dir_all(dir);
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
