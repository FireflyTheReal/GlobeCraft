use super::{KeyBuild, Tile, TileKey, disk::DiskCache, make_key, unkey};
use crate::paths::cache_dir;
use moka::{policy::EvictionPolicy, sync::Cache};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
/// Download tasks; each source's own `max_parallel` limits how many serve it at once.
const TILE_WORKER_COUNT: usize = 16;
/// Idle download tasks look at the queue this often even without a wake-up (a source's
/// throttling pause ending, a slot freed for a waiting source).
const WORKER_RECHECK: Duration = Duration::from_millis(250);
const FAILED_TILE_RETRY_DELAY: Duration = Duration::from_secs(15);
/// Best-effort decoded-pixel target; Moka may temporarily exceed it during eviction.
pub const DEFAULT_TILE_CACHE_BUDGET_BYTES: usize = 256 * 1024 * 1024;
const MAX_TILE_RESPONSE_BYTES: u64 = 8_000_000;
pub const MAX_INFLIGHT: usize = 48;

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
    inflight: AtomicUsize,
    /// Benchmarks only: every tile comes from `{base}/{provider}/{z}/{x}/{y}/{release}`.
    fixture_source: Option<Arc<str>>,
    /// Requests a background still render (a high-quality screenshot) still needs: a new
    /// view's priorities do not retire them.
    reserved: Mutex<HashSet<TileKey, KeyBuild>>,
    /// Downloads running now, with the signal that stops one whose tile left the view.
    /// Lock after `requests` and `queue`.
    downloading: Mutex<HashMap<TileKey, Arc<tokio::sync::Notify>, KeyBuild>>,
    /// Sources pausing after a throttling answer, until when (by source id).
    throttled: Mutex<HashMap<u64, Instant>>,
}

/// A download a task takes from the queue.
struct Job {
    key: TileKey,
    epoch: u64,
    ahead: bool,
    cancel: Arc<tokio::sync::Notify>,
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
            inflight: AtomicUsize::new(0),
            fixture_source: None,
            reserved: Mutex::new(HashSet::default()),
            downloading: Mutex::new(HashMap::default()),
            throttled: Mutex::new(HashMap::new()),
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

    pub fn lookup(&self, key: TileKey) -> Lookup {
        if let Some(tile) = self.ready.get(&key) {
            return Lookup::Ready(tile);
        }
        match self.requests.lock().unwrap().get(&key) {
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
    fn elevation_at(&self, lat: f64, lon: f64, zoom: u8, request: bool) -> Option<f32> {
        let lat = lat.clamp(-85.05, 85.05);
        let u = ((lon + 180.0) / 360.0).rem_euclid(1.0);
        let s = lat.to_radians().sin();
        let v = 0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * std::f64::consts::PI);
        for level in (0..=zoom.min(super::Provider::TerrainDem.max_zoom())).rev() {
            let n = f64::from(1u32 << level);
            let (tx, ty) = ((u * n).floor().min(n - 1.0), (v * n).floor().min(n - 1.0));
            let key = make_key(super::Provider::TerrainDem, level, tx as u32, ty as u32, 0);
            match self.lookup(key) {
                Lookup::Ready(tile) => {
                    let elevation = tile.elevation()?;
                    let size = elevation.size() as f64;
                    return Some(elevation.height_m((u * n - tx) * size, (v * n - ty) * size));
                }
                Lookup::Absent if request && level == zoom => {
                    // Admitted like a load ahead of need: the view's own admission
                    // (which may not sample this tile) must not stop it.
                    self.request_under_limit(key, MAX_INFLIGHT - 1);
                }
                _ => {}
            }
        }
        None
    }
    /// Whether the last download of `key` failed (a network error, not a missing tile).
    pub fn failed(&self, key: TileKey) -> bool {
        matches!(
            self.requests.lock().unwrap().get(&key),
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
        let mut requests = self.requests.lock().unwrap();
        if self.ready.contains_key(&key)
            || requests.get(&key).is_some_and(RequestState::is_pending)
            || self.inflight.load(Ordering::Relaxed) >= limit.min(MAX_INFLIGHT)
        {
            return false;
        }
        let ahead = limit < MAX_INFLIGHT;
        requests.insert(key, RequestState::Loading(self.disk.epoch(), ahead));
        self.queue.lock().unwrap().push_back(key);
        self.inflight.fetch_add(1, Ordering::Relaxed);
        // If wake tickets are full, queued tickets already cover the bounded
        // job queue. Retiring a view must not accumulate unbounded stale tickets.
        if matches!(
            self.tx.try_send(key),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_))
        ) {
            self.queue.lock().unwrap().retain(|&queued| queued != key);
            requests.insert(key, RequestState::Failed(Instant::now()));
            self.inflight.fetch_sub(1, Ordering::Relaxed);
            return false;
        }
        true
    }
    /// Requests `key` for background work (a screenshot), leaving a quarter of the
    /// download slots to the view on screen. False when nothing was requested.
    pub fn request_in_background(&self, key: TileKey) -> bool {
        self.inflight() < MAX_INFLIGHT * 3 / 4 && self.request(key)
    }
    /// Keeps `keys` queued whatever view is prioritized next, until `release_reserved`.
    pub fn reserve(&self, keys: &[TileKey]) {
        if let Ok(mut reserved) = self.reserved.lock() {
            reserved.extend(keys.iter().copied());
        }
    }
    pub fn release_reserved(&self) {
        if let Ok(mut reserved) = self.reserved.lock() {
            reserved.clear();
        }
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
            let mut requests = self.requests.lock().unwrap();
            #[cfg(feature = "profiling")]
            crate::profiling::Span::elapsed("tiles.admission_lock", waiting, Default::default());
            let mut queue = self.queue.lock().unwrap();
            let reserved = self.reserved.lock().map(|r| r.clone()).unwrap_or_default();
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
            self.downloading.lock().unwrap().retain(|key, cancel| {
                let (provider, zoom, _, _, _) = unkey(*key);
                let keep = wanted.contains(key)
                    || reserved.contains(key)
                    || (provider == super::Provider::Satellite && zoom <= 4)
                    || matches!(requests.get(key), Some(RequestState::Loading(_, true)));
                if !keep {
                    if matches!(requests.remove(key), Some(RequestState::Loading(..))) {
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
            if !self.request(key) && self.inflight() >= MAX_INFLIGHT {
                break;
            }
        }
    }
    /// Schedule failed visible dependencies without rerasterizing the globe.
    /// Full admission relies on completion wakeups rather than a busy retry loop.
    pub fn retry_delay(&self, keys: &[TileKey]) -> Option<Duration> {
        let requests = self.requests.lock().unwrap();
        keys.iter()
            .filter_map(|key| match requests.get(key) {
                Some(RequestState::Failed(time)) => {
                    Some(FAILED_TILE_RETRY_DELAY.saturating_sub(time.elapsed()))
                }
                Some(RequestState::Loading(..) | RequestState::Unavailable) => None,
                None if self.inflight() < MAX_INFLIGHT && !self.ready.contains_key(key) => {
                    Some(Duration::ZERO)
                }
                None => None,
            })
            .min()
    }

    /// The first queued tile whose source has a free download slot and is not pausing.
    fn next_job(&self) -> Option<Job> {
        let requests = self.requests.lock().unwrap();
        let mut queue = self.queue.lock().unwrap();
        let mut downloading = self.downloading.lock().unwrap();
        let throttled = self.throttled.lock().unwrap();
        let now = Instant::now();
        let mut active: HashMap<u64, usize> = HashMap::new();
        for key in downloading.keys() {
            *active.entry(unkey(*key).0.id()).or_default() += 1;
        }
        let mut index = 0;
        while index < queue.len() {
            let key = queue[index];
            let Some(&RequestState::Loading(epoch, ahead)) = requests.get(&key) else {
                // Retired or already finished: drop the stale entry.
                queue.remove(index);
                continue;
            };
            if downloading.contains_key(&key) {
                queue.remove(index);
                continue;
            }
            let provider = unkey(key).0;
            let busy = active.get(&provider.id()).copied().unwrap_or(0)
                >= usize::from(provider.source().max_parallel)
                || throttled
                    .get(&provider.id())
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
            });
        }
        None
    }

    /// A throttled download goes back to the front of the queue and its source pauses.
    fn throttle(&self, key: TileKey, wait: Duration) {
        let requests = self.requests.lock().unwrap();
        if matches!(requests.get(&key), Some(RequestState::Loading(..))) {
            self.queue.lock().unwrap().push_front(key);
        }
        let provider = unkey(key).0;
        let until = Instant::now() + wait;
        let mut throttled = self.throttled.lock().unwrap();
        let entry = throttled.entry(provider.id()).or_insert(until);
        *entry = (*entry).max(until);
    }

    fn complete(&self, key: TileKey, result: Result<Tile, TileFailure>) {
        let unavailable = matches!(result, Err(TileFailure::Unavailable));
        let admitted = match result {
            Ok(tile) if tile.bytes() <= self.budget => {
                self.ready.insert(key, Arc::new(tile));
                true
            }
            _ => {
                self.ready.invalidate(&key);
                false
            }
        };
        // Publish the decoded tile before removing Loading to avoid duplicate requests.
        let mut requests = self.requests.lock().unwrap();
        self.queue.lock().unwrap().retain(|&queued| queued != key);
        let was_loading = matches!(requests.remove(&key), Some(RequestState::Loading(..)));
        requests.retain(|_, state| state.is_pending());
        if unavailable {
            requests.insert(key, RequestState::Unavailable);
        } else if !admitted {
            requests.insert(key, RequestState::Failed(Instant::now()));
        }
        if was_loading {
            self.inflight.fetch_sub(1, Ordering::Relaxed);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
    /// Changes whenever a tile request finishes, including failures.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
    pub fn inflight(&self) -> usize {
        self.inflight.load(Ordering::Relaxed)
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
    persistence: Option<(std::path::PathBuf, Vec<u8>)>,
}
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
        eprintln!(
            "Tile source {}: first failed download ({reason}): {url}",
            provider.code()
        );
    }
}
async fn fetch_tile(
    k: TileKey,
    disk: &DiskCache,
    fixture_source: Option<&str>,
    ahead: bool,
) -> Result<FetchedTile, TileFailure> {
    let (provider, zoom, x, y, release) = unkey(k);
    let path = disk.path(k)?;
    if let Some(bytes) = disk.read(&path).await {
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
                return Ok(FetchedTile {
                    tile,
                    persistence: None,
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
        return Err(crate::Error::from("empty tile URL").into());
    }
    let bytes = match crate::http::body_or_retry_after(
        crate::http::client(crate::http::Policy::Tile)?.get(url.as_str()),
        MAX_TILE_RESPONSE_BYTES,
    )
    .await
    {
        Ok(crate::http::Fetched::RetryAfter(wait)) => {
            report_first_failure(provider, &url, "throttled by the server");
            return Err(TileFailure::Throttled(wait));
        }
        Ok(crate::http::Fetched::Body(bytes)) if is_placeholder(&bytes) => {
            return Err(TileFailure::Unavailable);
        }
        Ok(crate::http::Fetched::Body(bytes)) => {
            crate::download_stats::record(
                if ahead {
                    crate::download_stats::DownloadKind::ImageryAhead
                } else {
                    crate::download_stats::DownloadKind::Imagery
                },
                bytes.len(),
            );
            bytes
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
    Ok(FetchedTile {
        tile,
        persistence: Some((path, bytes)),
    })
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

async fn download(cache: &TileCache, job: Job, repaint: &Arc<dyn Fn() + Send + Sync>) {
    let fetch = fetch_tile(
        job.key,
        &cache.disk,
        cache.fixture_source.as_deref(),
        job.ahead,
    );
    let outcome = tokio::select! {
        result = fetch => Some(result),
        () = job.cancel.notified() => None,
    };
    cache.downloading.lock().unwrap().remove(&job.key);
    match outcome {
        // The tile left the view; its request was already retired.
        None => crate::download_stats::record_cancelled(),
        Some(Err(TileFailure::Throttled(wait))) => cache.throttle(job.key, wait),
        Some(Ok(FetchedTile { tile, persistence })) => {
            // Publish before optional SQLite persistence, so writes/eviction do
            // not delay imagery readiness. Persistence keeps its original epoch.
            cache.complete(job.key, Ok(tile));
            repaint();
            if let Some((path, bytes)) = persistence {
                let _ = cache.disk.write_owned(&path, bytes, job.epoch).await;
            }
        }
        Some(Err(error)) => {
            cache.complete(job.key, Err(error));
            repaint();
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod benchmarks;
