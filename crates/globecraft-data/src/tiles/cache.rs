use super::{
    KeyBuild, Tile, TileKey,
    disk::{DiskCache, Validity, now_secs},
    downloads::Downloads,
    make_key, unkey,
};
use crate::paths::cache_dir;
use moka::{policy::EvictionPolicy, sync::Cache};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
/// Download tasks; each source's own `max_parallel` limits how many serve it at once.
/// Imagery this fine or finer colours the buildings: one texel at zoom 12 spans about
/// 40 m, a block or two.
pub const FINE_IMAGERY_ZOOM: u8 = 12;
const TILE_WORKER_COUNT: usize = 16;
/// Idle download tasks look at the queue this often even without a wake-up (a source's
/// throttling pause ending, a slot freed for a waiting source).
const WORKER_RECHECK: Duration = Duration::from_millis(250);
const FAILED_TILE_RETRY_DELAY: Duration = Duration::from_secs(15);
/// Best-effort decoded-pixel target; Moka may temporarily exceed it during eviction.
pub const DEFAULT_TILE_CACHE_BUDGET_BYTES: usize = 256 * 1024 * 1024;
const MAX_TILE_RESPONSE_BYTES: u64 = 8_000_000;
/// Stored tiles waiting to be asked for again (roadmap 1.3), at most.
const MAX_REVALIDATIONS_QUEUED: usize = 512;
/// Revalidations running at once: one, and only while no tile waits for a slot.
const MAX_REVALIDATIONS_RUNNING: usize = 1;
pub const MAX_INFLIGHT: usize = 48;
/// Expired failures leave the request map in passes, once it has doubled since the last
/// pass (and never below this size), not on every finished tile.
const REQUEST_PRUNE_MIN_ENTRIES: usize = 1024;
/// Tiles a source does not have are remembered for the session, up to about this many;
/// past it they are forgotten together and each is asked for once more.
const MAX_REMEMBERED_REQUESTS: usize = 100_000;

enum RequestState {
    /// The disk epoch at admission, and whether the tile is loaded ahead of need (a
    /// warmup) rather than for the view.
    Loading(u64, bool),
    Failed(Instant),
    /// The source answered that it has no tile here (Esri's "Map data not yet
    /// available"); never requested again this session.
    Unavailable,
}
impl RequestState {
    fn is_pending(&self) -> bool {
        match self {
            Self::Loading(..) | Self::Unavailable => true,
            Self::Failed(time) => time.elapsed() < FAILED_TILE_RETRY_DELAY,
        }
    }
}

/// Why a tile did not load.
pub(super) enum TileFailure {
    /// The source has no tile at this position and level: renderers use the
    /// deepest coarser level instead (overzoom).
    Unavailable,
    /// A network, disk or decoding error: retried after `FAILED_TILE_RETRY_DELAY`.
    Error,
    /// The server asked to slow down (429 or 503): its source pauses this long and the
    /// tile waits in the queue.
    Throttled(Duration),
    /// A stored tile asked for again is unchanged (304).
    NotModified,
}
impl<E: Into<crate::Error>> From<E> for TileFailure {
    fn from(_: E) -> Self {
        Self::Error
    }
}

/// Esri World Imagery's "Map data not yet available" placeholder: a normal
/// 256×256 JPEG, 2,521 bytes, FNV-1a 64 `0x484ab5da02f7864f`. Requests carry
/// `blankTile=false` so the server answers 404 instead, but tiles cached by
/// earlier versions may still hold it.
const ESRI_PLACEHOLDER_LEN: usize = 2_521;
const ESRI_PLACEHOLDER_FNV: u64 = 0x484a_b5da_02f7_864f;

pub(super) fn is_placeholder(bytes: &[u8]) -> bool {
    bytes.len() == ESRI_PLACEHOLDER_LEN
        && bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        }) == ESRI_PLACEHOLDER_FNV
}

pub enum Lookup {
    Ready(Arc<Tile>),
    Pending,
    Absent,
    /// The source has no tile here; use a coarser level and do not request it.
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheBudgets {
    pub decoded_bytes: usize,
    pub disk_bytes: u64,
}
impl Default for CacheBudgets {
    fn default() -> Self {
        Self {
            decoded_bytes: DEFAULT_TILE_CACHE_BUDGET_BYTES,
            disk_bytes: super::disk::DEFAULT_DISK_BUDGET_BYTES,
        }
    }
}

pub struct TileCache {
    ready: Cache<TileKey, Arc<Tile>, KeyBuild>,
    // Synchronizes request admission only; decoded-cache hits bypass this lock.
    requests: Mutex<HashMap<TileKey, RequestState, KeyBuild>>,
    // Always lock after requests; channel messages are wake tickets, not FIFO jobs.
    queue: Mutex<VecDeque<TileKey>>,
    budget: usize,
    disk: DiskCache,
    tx: tokio::sync::mpsc::Sender<TileKey>,
    generation: AtomicU64,
    /// Like `generation`, for elevation tiles only: what raises overlays onto the relief.
    elevation_generation: AtomicU64,
    /// Like `generation`, for imagery tiles of `FINE_IMAGERY_ZOOM` or finer only: what
    /// colours the buildings again.
    imagery_generation: AtomicU64,
    /// When the last elevation tile arrived.
    elevation_arrived: Mutex<Option<Instant>>,
    inflight: AtomicUsize,
    /// Benchmarks only: every tile comes from `{base}/{provider}/{z}/{x}/{y}/{release}`.
    fixture_source: Option<Arc<str>>,
    /// Requests a background still render (a high-quality screenshot) still needs: a new
    /// view's priorities do not retire them.
    reserved: Mutex<HashSet<TileKey, KeyBuild>>,
    /// Downloads running now, with the signal that stops one whose tile left the view.
    /// Lock after `requests` and `queue`.
    downloading: Mutex<Downloads>,
    /// Sources pausing after a throttling answer, until when (by source id).
    throttled: Mutex<HashMap<u64, Instant>>,
    /// Stored tiles past their source's `revalidate_days`, shown and to be asked for
    /// again with their validators. Lock after `downloading`.
    revalidations: Mutex<VecDeque<(TileKey, Validity)>>,
    revalidating: AtomicUsize,
    /// The request map's size at which the next prune pass runs (see `complete`).
    prune_above: AtomicUsize,
}

/// One tile source's downloads (`TileCache::diagnostics`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceQueue {
    pub provider: super::Provider,
    /// Waiting for a download slot.
    pub queued: usize,
    /// Downloading now.
    pub downloading: usize,
    /// Pausing after the server asked to slow down.
    pub paused: bool,
}

/// A download a task takes from the queue.
struct Job {
    key: TileKey,
    epoch: u64,
    ahead: bool,
    cancel: Arc<tokio::sync::Notify>,
    /// Ask again for a stored tile with these validators, instead of loading it.
    revalidate: Option<Validity>,
}

/// The width of an imagery tile, in pixels, as the services serve them.
const IMAGERY_TILE_PX: f64 = 256.0;

/// Web Mercator coordinates (0–1 across the world, `v` downward) of a point.
/// A loaded elevation tile: its level, coordinates and the tile.
type ElevationTile = (u8, u32, u32, Arc<Tile>);

/// `TileCache::loaded_elevation_m` for many points at one `zoom`, each tile of that
/// level looked up in the cache once: the points of a line or a line of sight mostly
/// share a tile, and each cache lookup tries the levels down to a loaded one (the
/// overlays of a view tilted 70° at 16 km made about 50,000 lookups a frame on an M1,
/// 2026-11-26). The same heights, for as long as the cache holds the same tiles.
pub struct LoadedElevation<'a> {
    cache: &'a TileCache,
    zoom: u8,
    /// Direct-mapped by the zoom tile's coordinates: the tile and what it read.
    memo: std::sync::Mutex<Vec<Option<MemoEntry>>>,
}
/// A zoom tile's coordinates and the loaded elevation tile its points read.
type MemoEntry = ((u32, u32), Option<ElevationTile>);

/// Entries of `LoadedElevation::memo`.
const LOADED_ELEVATION_MEMO: usize = 256;

impl<'a> LoadedElevation<'a> {
    pub fn new(cache: &'a TileCache, zoom: u8) -> Self {
        Self {
            cache,
            zoom: zoom.min(super::Provider::TerrainDem.max_zoom()),
            memo: std::sync::Mutex::new(vec![None; LOADED_ELEVATION_MEMO]),
        }
    }
    /// As `TileCache::loaded_elevation_m(lat, lon, zoom)`.
    pub fn height_m(&self, lat: f64, lon: f64) -> Option<f32> {
        let (u, v) = mercator(lat, lon);
        let n = f64::from(1u32 << self.zoom);
        let (x, y) = (
            (u * n).floor().min(n - 1.0) as u32,
            (v * n).floor().min(n - 1.0) as u32,
        );
        let slot = (x as usize).wrapping_mul(31).wrapping_add(y as usize) % LOADED_ELEVATION_MEMO;
        let found = {
            // Another thread on it (a layer projected in parallel): the cache itself.
            let Ok(mut memo) = self.memo.try_lock() else {
                return self.cache.loaded_elevation_m(lat, lon, self.zoom);
            };
            match &memo[slot] {
                Some((at, found)) if *at == (x, y) => found.clone(),
                _ => {
                    let found = self.cache.loaded_elevation_tile(self.zoom, x, y);
                    memo[slot] = Some(((x, y), found.clone()));
                    found
                }
            }
        };
        let (level, tx, ty, tile) = found?;
        let elevation = tile.elevation()?;
        let n = f64::from(1u32 << level);
        let size = elevation.size() as f64;
        Some(elevation.height_m(
            (u * n - f64::from(tx)) * size,
            (v * n - f64::from(ty)) * size,
        ))
    }
}

fn mercator(lat: f64, lon: f64) -> (f64, f64) {
    let lat = lat.clamp(-85.05, 85.05);
    let u = ((lon + 180.0) / 360.0).rem_euclid(1.0);
    let s = lat.to_radians().sin();
    let v = 0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * std::f64::consts::PI);
    (u, v)
}

impl TileCache {
    #[cfg(test)]
    fn with_budget(tx: tokio::sync::mpsc::Sender<TileKey>, budget: usize) -> Self {
        Self::with_budget_at(tx, budget, cache_dir())
    }
    #[cfg(test)]
    fn with_budget_at(
        tx: tokio::sync::mpsc::Sender<TileKey>,
        budget: usize,
        directory: PathBuf,
    ) -> Self {
        Self::with_budgets_at(
            tx,
            CacheBudgets {
                decoded_bytes: budget,
                ..CacheBudgets::default()
            },
            directory,
        )
    }
    fn with_budgets_at(
        tx: tokio::sync::mpsc::Sender<TileKey>,
        budgets: CacheBudgets,
        directory: PathBuf,
    ) -> Self {
        let budget = budgets.decoded_bytes;
        Self {
            ready: Cache::builder()
                .name("globecraft-decoded-tiles")
                .max_capacity(budget as u64)
                .weigher(|_: &TileKey, tile: &Arc<Tile>| {
                    u32::try_from(tile.bytes()).unwrap_or(u32::MAX)
                })
                // LRU favors imagery revisited during map browsing.
                .eviction_policy(EvictionPolicy::lru())
                .build_with_hasher(KeyBuild::default()),
            requests: Mutex::new(HashMap::default()),
            queue: Mutex::new(VecDeque::new()),
            budget,
            disk: DiskCache::with_budget(directory, budgets.disk_bytes),
            tx,
            generation: AtomicU64::new(0),
            elevation_generation: AtomicU64::new(0),
            imagery_generation: AtomicU64::new(0),
            elevation_arrived: Mutex::new(None),
            inflight: AtomicUsize::new(0),
            fixture_source: None,
            reserved: Mutex::new(HashSet::default()),
            downloading: Mutex::new(Downloads::default()),
            throttled: Mutex::new(HashMap::new()),
            revalidations: Mutex::new(VecDeque::new()),
            revalidating: AtomicUsize::new(0),
            prune_above: AtomicUsize::new(REQUEST_PRUNE_MIN_ENTRIES),
        }
    }
    pub fn new(
        repaint: impl Fn() + Send + Sync + 'static,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        Self::with_cache_dir(repaint, runtime, cache_dir())
    }

    /// Use an explicit imagery directory, including for isolated render fixtures.
    pub fn with_cache_dir(
        repaint: impl Fn() + Send + Sync + 'static,
        runtime: &tokio::runtime::Handle,
        directory: PathBuf,
    ) -> Arc<Self> {
        Self::with_budgets_in(repaint, runtime, directory, CacheBudgets::default())
    }
    pub fn with_budgets(
        repaint: impl Fn() + Send + Sync + 'static,
        runtime: &tokio::runtime::Handle,
        budgets: CacheBudgets,
    ) -> Arc<Self> {
        Self::with_budgets_in(repaint, runtime, cache_dir(), budgets)
    }
    /// A cache that downloads every tile from a local fixture server at `base`
    /// (`{base}/{provider code}/{z}/{x}/{y}/{release}`), for the streaming replay
    /// benchmark. Disk caching in `directory` works as in production.
    pub fn with_fixture_source(
        repaint: impl Fn() + Send + Sync + 'static,
        runtime: &tokio::runtime::Handle,
        directory: PathBuf,
        base: &str,
    ) -> Arc<Self> {
        Self::spawn(
            repaint,
            runtime,
            directory,
            CacheBudgets::default(),
            Some(base.trim_end_matches('/').into()),
        )
    }
    fn with_budgets_in(
        repaint: impl Fn() + Send + Sync + 'static,
        runtime: &tokio::runtime::Handle,
        directory: PathBuf,
        budgets: CacheBudgets,
    ) -> Arc<Self> {
        Self::spawn(repaint, runtime, directory, budgets, None)
    }
    fn spawn(
        repaint: impl Fn() + Send + Sync + 'static,
        runtime: &tokio::runtime::Handle,
        directory: PathBuf,
        budgets: CacheBudgets,
        fixture_source: Option<Arc<str>>,
    ) -> Arc<Self> {
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(repaint);
        let (tx, rx) = tokio::sync::mpsc::channel(MAX_INFLIGHT);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));
        let cache = Arc::new(Self {
            fixture_source,
            ..Self::with_budgets_at(tx, budgets, directory)
        });
        for _ in 0..TILE_WORKER_COUNT {
            let (c, r, x) = (cache.clone(), rx.clone(), repaint.clone());
            runtime.spawn(worker(Arc::downgrade(&c), r, x));
        }
        cache
    }

    pub fn budgets(&self) -> CacheBudgets {
        CacheBudgets {
            decoded_bytes: self.budget,
            disk_bytes: self.disk.budget_bytes(),
        }
    }
    /// Short-lived decoded handles for a renderer upload. Never retain this as a second RAM cache.
    pub fn ready_tiles(
        &self,
        provider: super::Provider,
        release: u16,
    ) -> Vec<(TileKey, Arc<Tile>)> {
        self.ready
            .iter()
            .filter_map(|(key, tile)| {
                let (p, _, _, _, r) = unkey(*key);
                (p == provider && r == release).then_some((*key, tile))
            })
            .collect()
    }

    /// `key`'s tile when it is loaded; nothing is requested and the request table's
    /// lock is not taken, so a frame's many lookups of loaded tiles stay cheap.
    pub fn ready_tile(&self, key: TileKey) -> Option<Arc<Tile>> {
        self.ready.get(&key)
    }

    pub fn lookup(&self, key: TileKey) -> Lookup {
        if let Some(tile) = self.ready.get(&key) {
            return Lookup::Ready(tile);
        }
        match self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            Some(RequestState::Unavailable) => Lookup::Unavailable,
            Some(state) if state.is_pending() => Lookup::Pending,
            _ => Lookup::Absent,
        }
    }
    /// The ground height in metres at `(lat, lon)` from the finest loaded elevation
    /// tile at or below `zoom`, requesting the `zoom` tile when it is missing. `None`
    /// until any elevation covers the point.
    pub fn elevation_m(&self, lat: f64, lon: f64, zoom: u8) -> Option<f32> {
        self.elevation_at(lat, lon, zoom, true)
    }
    /// `elevation_m` without requesting anything: overlays follow what the frames load.
    pub fn loaded_elevation_m(&self, lat: f64, lon: f64, zoom: u8) -> Option<f32> {
        self.elevation_at(lat, lon, zoom, false)
    }
    /// The loaded elevation tile `loaded_elevation_m` reads for a point in tile
    /// `(x, y)` of level `zoom` (every point of it reads the same one): its level, its
    /// coordinates and the tile.
    fn loaded_elevation_tile(&self, zoom: u8, x: u32, y: u32) -> Option<ElevationTile> {
        for level in (0..=zoom).rev() {
            let shift = zoom - level;
            let (tx, ty) = (x >> shift, y >> shift);
            let key = make_key(super::Provider::TerrainDem, level, tx, ty, 0);
            if let Some(tile) = self.ready.get(&key) {
                return tile.elevation().is_some().then_some((level, tx, ty, tile));
            }
        }
        None
    }
    /// The colour (RGB, 0–1) of `provider`'s imagery around `(lat, lon)`, averaged
    /// over about `span_deg` of longitude, from the finest loaded tile not coarser
    /// than `coarsest_zoom`. Nothing is requested: buildings take the colour of what
    /// the frames have loaded.
    pub fn loaded_colour(
        &self,
        provider: super::Provider,
        release: u16,
        (lat, lon): (f64, f64),
        span_deg: f64,
        coarsest_zoom: u8,
    ) -> Option<glam::Vec4> {
        let (u, v) = mercator(lat, lon);
        // Finer levels than one whose texels the span covers `MAX_SAMPLE_STRIDE`
        // times add nothing to the average, and each level tried is a lookup.
        let finest = (Tile::MAX_SAMPLE_STRIDE * 360.0 / (span_deg * IMAGERY_TILE_PX))
            .log2()
            .ceil()
            .clamp(f64::from(coarsest_zoom), f64::from(provider.max_zoom()))
            as u8;
        for level in (coarsest_zoom..=finest).rev() {
            let n = f64::from(1u32 << level);
            let (tx, ty) = ((u * n).floor().min(n - 1.0), (v * n).floor().min(n - 1.0));
            // Only what is ready: a miss must not wait on the request table's lock,
            // which the frames take for every tile they ask for.
            let Some(tile) = self
                .ready
                .get(&make_key(provider, level, tx as u32, ty as u32, release))
            else {
                continue;
            };
            if tile.elevation().is_some() {
                return None;
            }
            let (width, _, _) = tile.rgba();
            let texels = span_deg / 360.0 * n * width as f64;
            let stride = texels.clamp(1.0, Tile::MAX_SAMPLE_STRIDE) as f32;
            return Some(tile.sample(u * n - tx, v * n - ty, stride));
        }
        None
    }
    fn elevation_at(&self, lat: f64, lon: f64, zoom: u8, request: bool) -> Option<f32> {
        let (u, v) = mercator(lat, lon);
        for level in (0..=zoom.min(super::Provider::TerrainDem.max_zoom())).rev() {
            let n = f64::from(1u32 << level);
            let (tx, ty) = ((u * n).floor().min(n - 1.0), (v * n).floor().min(n - 1.0));
            let key = make_key(super::Provider::TerrainDem, level, tx as u32, ty as u32, 0);
            if let Some(tile) = self.ready.get(&key) {
                let elevation = tile.elevation()?;
                let size = elevation.size() as f64;
                return Some(elevation.height_m((u * n - tx) * size, (v * n - ty) * size));
            }
            // Only a lookup that may request asks the request table (behind a lock the
            // frames take for every tile they ask for): the overlays' terrain test
            // makes 16 lookups a point, each trying the levels down to a loaded one,
            // and in a view tilted 70° at 16 km on an M1 it took 36–77 ms a frame
            // (2026-11-26).
            if request && level == zoom && matches!(self.lookup(key), Lookup::Absent) {
                // Admitted like a load ahead of need: the view's own admission
                // (which may not sample this tile) must not stop it.
                self.request_under_limit(key, MAX_INFLIGHT - 1);
            }
        }
        None
    }
    /// Whether the last download of `key` failed (a network error, not a missing tile).
    pub fn failed(&self, key: TileKey) -> bool {
        matches!(
            self.requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&key),
            Some(RequestState::Failed(_))
        )
    }
    /// Estimated decoded-cache bytes; Moka maintenance may lag inserts and evictions.
    /// Renderer references and other allocations are not included in this metric.
    pub fn resident_bytes(&self) -> usize {
        usize::try_from(self.ready.weighted_size()).unwrap_or(usize::MAX)
    }
    pub fn request(&self, key: TileKey) -> bool {
        self.request_under_limit(key, MAX_INFLIGHT)
    }
    // Check under the admission lock, so background work cannot race past its limit.
    pub(super) fn request_under_limit(&self, key: TileKey, limit: usize) -> bool {
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.ready.contains_key(&key)
            || requests.get(&key).is_some_and(RequestState::is_pending)
            || !self.has_slot(key, limit)
        {
            return false;
        }
        let ahead = limit < MAX_INFLIGHT;
        requests.insert(key, RequestState::Loading(self.disk.epoch(), ahead));
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(key);
        self.inflight.fetch_add(1, Ordering::Relaxed);
        // If wake tickets are full, queued tickets already cover the bounded
        // job queue. Retiring a view must not accumulate unbounded stale tickets.
        if matches!(
            self.tx.try_send(key),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_))
        ) {
            self.queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|&queued| queued != key);
            requests.insert(key, RequestState::Failed(Instant::now()));
            self.inflight.fetch_sub(1, Ordering::Relaxed);
            return false;
        }
        true
    }
    /// Whether a request for `key` fits under `limit` requests in flight. Tiles waiting
    /// for a host that asked to slow down (429/503, up to two minutes) do not count
    /// against other hosts: one paused source must not hold every slot, or elevation,
    /// night lights and polar tiles stop loading while the imagery host pauses. A paused
    /// host itself still gets no more than `limit`. Call with `requests` locked (lock
    /// order: requests, queue, throttled).
    fn has_slot(&self, key: TileKey, limit: usize) -> bool {
        let limit = limit.min(MAX_INFLIGHT);
        let inflight = self.inflight.load(Ordering::Relaxed);
        if inflight < limit {
            return true;
        }
        let queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let throttled = self
            .throttled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let paused = |key: TileKey| {
            throttled
                .get(&unkey(key).0.host_group())
                .is_some_and(|until| *until > now)
        };
        if paused(key) {
            return false;
        }
        let waiting = queue.iter().filter(|&&queued| paused(queued)).count();
        inflight.saturating_sub(waiting) < limit
    }
    /// Whether `key`'s host is pausing after a throttling answer.
    fn host_paused(&self, key: TileKey) -> bool {
        self.throttled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&unkey(key).0.host_group())
            .is_some_and(|until| *until > Instant::now())
    }
    /// Requests `key` for background work (a screenshot), leaving a quarter of the
    /// download slots to the view on screen. False when nothing was requested.
    pub fn request_in_background(&self, key: TileKey) -> bool {
        self.inflight() < MAX_INFLIGHT * 3 / 4 && self.request(key)
    }
    /// Keeps `keys` queued whatever view is prioritized next, until `release_reserved`.
    pub fn reserve(&self, keys: &[TileKey]) {
        self.reserved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(keys.iter().copied());
    }
    pub fn release_reserved(&self) {
        self.reserved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
    /// Put the latest view first; retire old queued work while keeping active
    /// downloads and the small satellite overview queue. Keys arrive coarse-first.
    pub fn prioritize_view(&self, keys: &[TileKey]) {
        #[cfg(feature = "profiling")]
        let _profile = crate::profiling::Span::new("tiles.admission");
        let wanted: HashSet<_> = keys.iter().copied().collect();
        {
            #[cfg(feature = "profiling")]
            let waiting = std::time::Instant::now();
            let mut requests = self
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(feature = "profiling")]
            crate::profiling::Span::elapsed("tiles.admission_lock", waiting, Default::default());
            let mut queue = self
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let reserved = self
                .reserved
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue.retain(|key| {
                let (provider, zoom, _, _, _) = unkey(*key);
                let keep = wanted.contains(key)
                    || reserved.contains(key)
                    || (provider == super::Provider::Satellite && zoom <= 4);
                if !keep && matches!(requests.remove(key), Some(RequestState::Loading(..))) {
                    self.inflight.fetch_sub(1, Ordering::Relaxed);
                }
                keep
            });
            // Downloads of tiles the view left stop, freeing their slots: during a fast
            // zoom the levels passed through are not loaded to the end (roadmap 1.3).
            // Loads ahead of need and reserved tiles continue.
            self.downloading
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|key, cancel| {
                    let (provider, zoom, _, _, _) = unkey(key);
                    let keep = wanted.contains(&key)
                        || reserved.contains(&key)
                        || (provider == super::Provider::Satellite && zoom <= 4)
                        || matches!(requests.get(&key), Some(RequestState::Loading(_, true)));
                    if !keep {
                        if matches!(requests.remove(&key), Some(RequestState::Loading(..))) {
                            self.inflight.fetch_sub(1, Ordering::Relaxed);
                        }
                        cancel.notify_one();
                    }
                    keep
                });
            // A tile the view now wants is no longer loaded ahead.
            for key in keys {
                if let Some(RequestState::Loading(_, ahead)) = requests.get_mut(key) {
                    *ahead = false;
                }
            }
            let queued: HashSet<_> = queue.iter().copied().collect();
            let mut selected = HashSet::new();
            let mut ordered: VecDeque<_> = keys
                .iter()
                .copied()
                .filter(|key| queued.contains(key) && selected.insert(*key))
                .collect();
            ordered.extend(queue.iter().copied().filter(|key| !wanted.contains(key)));
            *queue = ordered;
        }
        for &key in keys {
            // Past a paused host's tiles, the other hosts may still have slots.
            if !self.request(key) && self.inflight() >= MAX_INFLIGHT && !self.host_paused(key) {
                break;
            }
        }
    }
    /// Schedule failed visible dependencies without rerasterizing the globe.
    /// Full admission relies on completion wakeups rather than a busy retry loop.
    pub fn retry_delay(&self, keys: &[TileKey]) -> Option<Duration> {
        let requests = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        keys.iter()
            .filter_map(|key| match requests.get(key) {
                Some(RequestState::Failed(time)) if time.elapsed() < FAILED_TILE_RETRY_DELAY => {
                    Some(FAILED_TILE_RETRY_DELAY.saturating_sub(time.elapsed()))
                }
                Some(RequestState::Loading(..) | RequestState::Unavailable) => None,
                // Never asked, or a failure old enough to be asked again.
                _ if self.has_slot(*key, MAX_INFLIGHT) && !self.ready.contains_key(key) => {
                    Some(Duration::ZERO)
                }
                _ => None,
            })
            .min()
    }

    /// The first queued tile whose source and host have a free download slot and are
    /// not pausing.
    fn next_job(&self) -> Option<Job> {
        let requests = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut downloading = self
            .downloading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let throttled = self
            .throttled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let mut index = 0;
        while index < queue.len() {
            let key = queue[index];
            let Some(&RequestState::Loading(epoch, ahead)) = requests.get(&key) else {
                // Retired or already finished: drop the stale entry.
                queue.remove(index);
                continue;
            };
            if downloading.contains(key) {
                queue.remove(index);
                continue;
            }
            let provider = unkey(key).0;
            let busy = downloading.of_source(provider)
                >= usize::from(provider.source().max_parallel)
                || downloading.of_host(provider) >= provider.host_limit()
                || throttled
                    .get(&provider.host_group())
                    .is_some_and(|until| *until > now);
            if busy {
                index += 1;
                continue;
            }
            queue.remove(index);
            let cancel = Arc::new(tokio::sync::Notify::new());
            downloading.insert(key, cancel.clone());
            return Some(Job {
                key,
                epoch,
                ahead,
                cancel,
                revalidate: None,
            });
        }
        // Stored tiles are asked for again only while nothing waits for a slot.
        if !queue.is_empty()
            || self.revalidating.load(Ordering::Relaxed) >= MAX_REVALIDATIONS_RUNNING
        {
            return None;
        }
        let mut revalidations = self
            .revalidations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = revalidations.iter().position(|(key, _)| {
            let provider = unkey(*key).0;
            !downloading.contains(*key)
                && !matches!(requests.get(key), Some(RequestState::Loading(..)))
                && downloading.of_source(provider) < usize::from(provider.source().max_parallel)
                && downloading.of_host(provider) < provider.host_limit()
                && !throttled
                    .get(&provider.host_group())
                    .is_some_and(|until| *until > now)
        })?;
        let (key, validity) = revalidations.remove(index)?;
        let cancel = Arc::new(tokio::sync::Notify::new());
        downloading.insert(key, cancel.clone());
        self.revalidating.fetch_add(1, Ordering::Relaxed);
        Some(Job {
            key,
            epoch: self.disk.epoch(),
            ahead: true,
            cancel,
            revalidate: Some(validity),
        })
    }

    /// Queues a stored tile to be asked for again (dropped when the queue is full).
    fn queue_revalidation(&self, key: TileKey, validity: Validity) {
        let mut revalidations = self
            .revalidations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if revalidations.len() < MAX_REVALIDATIONS_QUEUED
            && !revalidations.iter().any(|(queued, _)| *queued == key)
        {
            revalidations.push_back((key, validity));
        }
    }

    /// A throttled download goes back to the front of the queue and its source pauses.
    fn throttle(&self, key: TileKey, wait: Duration) {
        let requests = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(requests.get(&key), Some(RequestState::Loading(..))) {
            self.queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push_front(key);
        }
        let provider = unkey(key).0;
        let until = Instant::now() + wait;
        let mut throttled = self
            .throttled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The host pauses: its other sources would be answered the same.
        let entry = throttled.entry(provider.host_group()).or_insert(until);
        *entry = (*entry).max(until);
    }

    fn complete(&self, key: TileKey, result: Result<Tile, TileFailure>) {
        let unavailable = matches!(result, Err(TileFailure::Unavailable));
        let admitted = match result {
            Ok(tile) if tile.bytes() <= self.budget => {
                self.ready
                    .insert(key, Arc::new(tile.arriving(Instant::now())));
                true
            }
            _ => {
                self.ready.invalidate(&key);
                false
            }
        };
        // Publish the decoded tile before removing Loading to avoid duplicate requests.
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|&queued| queued != key);
        let was_loading = matches!(requests.remove(&key), Some(RequestState::Loading(..)));
        // The map also holds every tile its source lacks, so scanning it on each finished
        // tile grew slower through a session. Expired failures go in passes instead.
        if requests.len() >= self.prune_above.load(Ordering::Relaxed) {
            requests.retain(|_, state| state.is_pending());
            if requests.len() > MAX_REMEMBERED_REQUESTS {
                requests.retain(|_, state| !matches!(state, RequestState::Unavailable));
            }
            self.prune_above.store(
                (requests.len() * 2).max(REQUEST_PRUNE_MIN_ENTRIES),
                Ordering::Relaxed,
            );
        }
        if unavailable {
            requests.insert(key, RequestState::Unavailable);
        } else if !admitted {
            requests.insert(key, RequestState::Failed(Instant::now()));
        }
        if was_loading {
            self.inflight.fetch_sub(1, Ordering::Relaxed);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
        if admitted {
            self.note_fine_imagery(key);
        }
        if admitted && unkey(key).0 == super::Provider::TerrainDem {
            self.elevation_generation.fetch_add(1, Ordering::Relaxed);
            *self
                .elevation_arrived
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
        }
    }
    /// Waits up to `limit` for tile writes to the disk cache to finish (a quit that
    /// ends the process itself). Returns whether they did.
    pub fn wait_for_disk_writes(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while self.disk.writes_in_progress() > 0 {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
    /// Changes whenever a tile request finishes, including failures.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
    /// Changes whenever an imagery tile of `FINE_IMAGERY_ZOOM` or finer arrives: the
    /// buildings take their colours from these, and were coloured again for every
    /// tile that finished, labels, relief and failures too.
    pub fn imagery_generation(&self) -> u64 {
        self.imagery_generation.load(Ordering::Relaxed)
    }
    fn note_fine_imagery(&self, key: TileKey) {
        let (provider, zoom, ..) = unkey(key);
        if provider.is_imagery() && zoom >= FINE_IMAGERY_ZOOM {
            self.imagery_generation.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// While relief is growing in (`ELEVATION_FADE` after an elevation tile arrived):
    /// a step number that changes every `step`, so a view at rest is drawn again
    /// for each; zero once all relief has grown.
    pub fn elevation_fade_step(&self, step: Duration) -> u64 {
        let arrived = *self
            .elevation_arrived
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        arrived
            .map(|arrived| arrived.elapsed())
            .filter(|&age| age < super::ELEVATION_FADE + step)
            .map_or(0, |age| {
                1 + (age.as_millis() / step.as_millis().max(1)) as u64
            })
    }
    /// Changes whenever an elevation tile arrives. Overlays raised onto the relief
    /// follow this one: imagery tiles arriving (most of them) do not move them, and
    /// re-projecting every border, road and place on each of those cost the most.
    pub fn elevation_generation(&self) -> u64 {
        self.elevation_generation.load(Ordering::Relaxed)
    }
    pub fn inflight(&self) -> usize {
        self.inflight.load(Ordering::Relaxed)
    }
    /// Queued and running downloads per tile source, and which sources pause after a
    /// throttling answer: the streaming diagnostics overlay (roadmap 1.11). Takes the
    /// queue locks briefly; only called while the overlay shows.
    pub fn diagnostics(&self) -> Vec<SourceQueue> {
        let mut sources: Vec<SourceQueue> = super::Provider::all()
            .map(|provider| SourceQueue {
                provider,
                queued: 0,
                downloading: 0,
                paused: false,
            })
            .collect();
        let mut count = |key: TileKey, field: fn(&mut SourceQueue) -> &mut usize| {
            let provider = super::unkey(key).0;
            if let Some(source) = sources.iter_mut().find(|s| s.provider == provider) {
                *field(source) += 1;
            }
        };
        for &key in self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            count(key, |s| &mut s.queued);
        }
        for key in self
            .downloading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
        {
            count(key, |s| &mut s.downloading);
        }
        let now = Instant::now();
        for (&id, &until) in self
            .throttled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            for source in sources
                .iter_mut()
                .filter(|s| s.provider.host_group() == id && until > now)
            {
                source.paused = true;
            }
        }
        sources
    }
    /// Remove this instance's downloaded tile files while preserving decoded
    /// tiles and other application data. Old jobs cannot write back afterward.
    pub async fn clear_disk_cache(&self) -> Result<(), crate::Error> {
        self.disk.clear().await?;
        Ok(())
    }
    /// Snapshot of encoded file bytes in the managed provider directories.
    /// Filesystem allocation overhead and other application data are excluded.
    pub async fn disk_usage_bytes(&self) -> Result<u64, crate::Error> {
        self.disk.usage_bytes().await
    }
}

struct FetchedTile {
    tile: Tile,
    persistence: Option<(std::path::PathBuf, Vec<u8>, Validity)>,
    /// Shown from disk, but older than its source's `revalidate_days`.
    stale: Option<Validity>,
}

/// Whether a stored tile fetched at `validity` should be asked for again: `None` when
/// its source's tiles never change. A tile stored before validity was kept counts as
/// fetched now (recorded by the caller), so an old cache is not asked for all at once.
fn needs_revalidation(
    provider: super::Provider,
    validity: Option<&Validity>,
    now_secs: i64,
) -> Option<bool> {
    let days = provider.source().revalidate_days?;
    Some(
        validity.is_some_and(|validity| {
            now_secs - validity.fetched_secs > i64::from(days) * SECS_PER_DAY
        }),
    )
}
const SECS_PER_DAY: i64 = 86_400;
async fn decode(
    bytes: Vec<u8>,
    provider: super::Provider,
) -> Result<(Tile, Vec<u8>), crate::Error> {
    crate::cpu::run(crate::cpu::Service::Tiles, move || {
        if provider == super::Provider::TerrainDem {
            Tile::decode_elevation(&bytes)
        } else {
            Tile::decode(&bytes)
        }
        .map(|tile| (tile, bytes))
    })
    .await
}
/// Writes a source's first failed download to the run log, so a wrong address in
/// `sources.json` shows up without a debugger. Later failures stay quiet.
fn report_first_failure(provider: super::Provider, url: &str, reason: &str) {
    static REPORTED: std::sync::Mutex<u32> = std::sync::Mutex::new(0);
    let bit = 1u32 << (provider.id() as u32);
    if let Ok(mut reported) = REPORTED.lock()
        && *reported & bit == 0
    {
        *reported |= bit;
        crate::log_line!(
            "Tile source {}: first failed download ({reason}): {url}",
            provider.code()
        );
    }
}
/// Writes the session's first failed disk-cache write to the run log: a full or
/// read-only disk would otherwise go unnoticed while every tile downloads again.
fn report_disk_failure(result: Result<(), crate::Error>) {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if let Err(error) = result
        && !REPORTED.swap(true, Ordering::Relaxed)
    {
        crate::log_line!(
            "Tile disk cache: a write failed ({error}); later failures are not reported"
        );
    }
}
async fn fetch_tile(
    k: TileKey,
    disk: &DiskCache,
    fixture_source: Option<&str>,
    ahead: bool,
    revalidate: Option<Validity>,
) -> Result<FetchedTile, TileFailure> {
    let (provider, zoom, x, y, release) = unkey(k);
    let path = disk.path(k)?;
    if revalidate.is_none()
        && let Some((bytes, validity)) = disk.read(&path).await
    {
        if is_placeholder(&bytes) {
            let _ = disk.invalidate(&path).await;
            return Err(TileFailure::Unavailable);
        }
        let read = bytes.len();
        match decode(bytes, provider).await {
            Ok((tile, _)) => {
                crate::download_stats::record(
                    crate::download_stats::DownloadKind::ImageryFromDisk,
                    read,
                );
                let now = now_secs();
                let stale = match needs_revalidation(provider, validity.as_ref(), now) {
                    Some(true) => validity,
                    Some(false) if validity.is_none() => {
                        let first_seen = Validity {
                            fetched_secs: now,
                            ..Validity::default()
                        };
                        let _ = disk.set_validity(&path, first_seen, disk.epoch()).await;
                        None
                    }
                    _ => None,
                };
                return Ok(FetchedTile {
                    tile,
                    persistence: None,
                    stale,
                });
            }
            Err(_) => {
                let _ = disk.invalidate(&path).await;
            }
        }
    }
    let url = match fixture_source {
        Some(base) => format!("{base}/{}/{zoom}/{x}/{y}/{release}", provider.code()),
        None => provider.url(zoom, x, y, release),
    };
    if url.is_empty() {
        return Err(crate::Error::msg("empty tile URL").into());
    }
    let request = crate::http::client(crate::http::Policy::Tile)?.get(url.as_str());
    let request = match &revalidate {
        Some(validity) => conditional(request, validity),
        None => request,
    };
    let (bytes, etag, last_modified) =
        match crate::http::conditional_body(request, MAX_TILE_RESPONSE_BYTES).await {
            Ok(crate::http::Conditional::RetryAfter(wait)) => {
                report_first_failure(provider, &url, "throttled by the server");
                return Err(TileFailure::Throttled(wait));
            }
            Ok(crate::http::Conditional::NotModified) => {
                return Err(TileFailure::NotModified);
            }
            Ok(crate::http::Conditional::Body { bytes, .. }) if is_placeholder(&bytes) => {
                return Err(TileFailure::Unavailable);
            }
            Ok(crate::http::Conditional::Body {
                bytes,
                etag,
                last_modified,
            }) => {
                crate::download_stats::record(
                    if ahead {
                        crate::download_stats::DownloadKind::ImageryAhead
                    } else {
                        crate::download_stats::DownloadKind::Imagery
                    },
                    bytes.len(),
                );
                (bytes, etag, last_modified)
            }
            Err(crate::Error::Http(error))
                if error.status() == Some(reqwest::StatusCode::NOT_FOUND) =>
            {
                report_first_failure(provider, &url, "not found");
                return Err(TileFailure::Unavailable);
            }
            Err(error) => {
                report_first_failure(provider, &url, &error.to_string());
                return Err(error.into());
            }
        };
    let (tile, bytes) = decode(bytes, provider).await?;
    let validity = Validity {
        fetched_secs: now_secs(),
        etag,
        last_modified,
    };
    Ok(FetchedTile {
        tile,
        persistence: Some((path, bytes, validity)),
        stale: None,
    })
}

/// The request asking whether a stored tile changed: its `ETag`, else its date.
fn conditional(request: reqwest::RequestBuilder, validity: &Validity) -> reqwest::RequestBuilder {
    match (&validity.etag, &validity.last_modified) {
        (Some(etag), _) => request.header(reqwest::header::IF_NONE_MATCH, etag),
        (None, Some(date)) => request.header(reqwest::header::IF_MODIFIED_SINCE, date),
        (None, None) => request,
    }
}
async fn worker(
    cache: std::sync::Weak<TileCache>,
    rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TileKey>>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    loop {
        {
            let Some(cache) = cache.upgrade() else {
                return;
            };
            // Wake-up tickets only say that work exists: take jobs while any source
            // has a free slot.
            while let Some(job) = cache.next_job() {
                download(&cache, job, &repaint).await;
            }
        }
        let ticket =
            tokio::time::timeout(WORKER_RECHECK, async { rx.lock().await.recv().await }).await;
        if matches!(ticket, Ok(None)) {
            return;
        }
    }
}

/// A future whose panic becomes an error: a panicking download (as every TLS handshake on
/// Android did before its certificate verifier was set up) would otherwise end its worker
/// task with the tile's request slot still counted, and after `TILE_WORKER_COUNT` of them
/// nothing would ever download again.
struct CatchPanic<F>(std::pin::Pin<Box<F>>);
impl<F: std::future::Future> std::future::Future for CatchPanic<F> {
    type Output = Result<F::Output, String>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let future = self.0.as_mut();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future.poll(cx))) {
            Ok(std::task::Poll::Ready(output)) => std::task::Poll::Ready(Ok(output)),
            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
            Err(payload) => std::task::Poll::Ready(Err(payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic payload")
                .to_owned())),
        }
    }
}

async fn download(cache: &TileCache, job: Job, repaint: &Arc<dyn Fn() + Send + Sync>) {
    let revalidating = job.revalidate.clone();
    let fetch = CatchPanic(Box::pin(fetch_tile(
        job.key,
        &cache.disk,
        cache.fixture_source.as_deref(),
        job.ahead,
        revalidating.clone(),
    )));
    let outcome = tokio::select! {
        result = fetch => Some(result.unwrap_or_else(|panic| {
            crate::log_line!("Tile download panicked: {panic}");
            Err(TileFailure::Error)
        })),
        () = job.cancel.notified() => None,
    };
    cache
        .downloading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(job.key);
    if let Some(validity) = revalidating {
        cache.revalidating.fetch_sub(1, Ordering::Relaxed);
        finish_revalidation(cache, &job, validity, outcome, repaint).await;
        return;
    }
    match outcome {
        // The tile left the view; its request was already retired.
        None => crate::download_stats::record_cancelled(),
        Some(Err(TileFailure::Throttled(wait))) => cache.throttle(job.key, wait),
        Some(Ok(FetchedTile {
            tile,
            persistence,
            stale,
        })) => {
            // Publish before optional SQLite persistence, so writes/eviction do
            // not delay imagery readiness. Persistence keeps its original epoch.
            cache.complete(job.key, Ok(tile));
            repaint();
            if let Some((path, bytes, validity)) = persistence {
                report_disk_failure(
                    cache
                        .disk
                        .write_owned(&path, bytes, Some(validity), job.epoch)
                        .await,
                );
            }
            if let Some(validity) = stale {
                cache.queue_revalidation(job.key, validity);
            }
        }
        Some(Err(error)) => {
            cache.complete(job.key, Err(error));
            repaint();
        }
    }
}

/// A stored tile asked for again: unchanged, it is marked current; changed, the new
/// one replaces it on screen and on disk. Failures leave the stored tile as it is.
async fn finish_revalidation(
    cache: &TileCache,
    job: &Job,
    validity: Validity,
    outcome: Option<Result<FetchedTile, TileFailure>>,
    repaint: &Arc<dyn Fn() + Send + Sync>,
) {
    match outcome {
        Some(Err(TileFailure::NotModified)) => {
            if let Ok(path) = cache.disk.path(job.key) {
                let confirmed = Validity {
                    fetched_secs: now_secs(),
                    ..validity
                };
                let _ = cache.disk.set_validity(&path, confirmed, job.epoch).await;
            }
            crate::download_stats::record_not_modified();
        }
        Some(Ok(FetchedTile {
            tile,
            persistence: Some((path, bytes, fresh)),
            ..
        })) => {
            if tile.bytes() <= cache.budget {
                cache.ready.insert(job.key, Arc::new(tile));
                cache.generation.fetch_add(1, Ordering::Relaxed);
                cache.note_fine_imagery(job.key);
                repaint();
            }
            report_disk_failure(
                cache
                    .disk
                    .write_owned(&path, bytes, Some(fresh), job.epoch)
                    .await,
            );
        }
        Some(Err(TileFailure::Throttled(wait))) => {
            cache.throttle(job.key, wait);
            cache.queue_revalidation(job.key, validity);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod benchmarks;
