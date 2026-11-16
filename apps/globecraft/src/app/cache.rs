//! Cache commands and completion polling; filesystem ownership stays in data.
use super::App;
use eframe::egui;
use globecraft_core::language::Lang;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const DISK_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const CACHE_CLEAR_NOTICE_DURATION: Duration = Duration::from_secs(5);
const BYTES_PER_KIBIBYTE: u64 = 1024;
type CacheResult = Result<(), globecraft_data::Error>;
type UsageResult = Result<u64, globecraft_data::Error>;

struct CacheClearNotice {
    message: String,
    expires_at: Instant,
}

/// The imagery tiles (roadmap 10.2: one of the feature states split out of `App`):
/// the cache every renderer and overlay reads, its controls in Settings, and the
/// tiles loaded ahead of the view.
pub(super) struct TilesState {
    pub(super) cache: std::sync::Arc<globecraft_data::tiles::TileCache>,
    pub(super) controls: CacheControls,
    /// The overview loaded at startup.
    pub(super) warmup: globecraft_data::tiles::TileWarmup,
    /// Imagery loaded ahead of a zoom out, for the view (state key) it was made for.
    pub(super) zoom_out_warmup: Option<(u64, globecraft_data::tiles::TileWarmup)>,
    /// The imagery a running flight lands on, loaded while it flies (roadmap 1.4),
    /// with the destination it was listed for.
    pub(super) arrival_warmup: Option<([u64; 5], globecraft_data::tiles::TileWarmup)>,
}

impl TilesState {
    pub(super) fn new(cache: std::sync::Arc<globecraft_data::tiles::TileCache>) -> Self {
        Self {
            cache,
            controls: CacheControls::default(),
            warmup: globecraft_data::tiles::TileWarmup::startup_satellite(),
            zoom_out_warmup: None,
            arrival_warmup: None,
        }
    }
}

#[derive(Default)]
pub(super) struct CacheControls {
    clear_rx: Option<mpsc::Receiver<CacheResult>>,
    usage_rx: Option<mpsc::Receiver<UsageResult>>,
    pub(super) usage: Option<Result<u64, String>>,
    last_usage_scan: Option<Instant>,
    clear_notice: Option<CacheClearNotice>,
}
impl CacheControls {
    pub(super) fn clearing(&self) -> bool {
        self.clear_rx.is_some()
    }

    fn poll_clear_notice(
        &mut self,
        status: &mut String,
        ready: &str,
        now: Instant,
    ) -> Option<Duration> {
        let notice = self.clear_notice.as_ref()?;
        if *status != notice.message {
            self.clear_notice = None;
        } else if now >= notice.expires_at {
            *status = ready.into();
            self.clear_notice = None;
        } else {
            return Some(notice.expires_at - now);
        }
        None
    }
}

pub(super) fn cache_size_text(bytes: u64) -> String {
    for (unit, scale) in [
        ("GiB", BYTES_PER_KIBIBYTE.pow(3)),
        ("MiB", BYTES_PER_KIBIBYTE.pow(2)),
        ("KiB", BYTES_PER_KIBIBYTE),
    ] {
        if bytes >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale as f64);
        }
    }
    format!("{bytes} B")
}

impl App {
    pub(super) fn clear_disk_cache(&mut self, ctx: &egui::Context) {
        if self.tiles.controls.clearing() {
            return;
        }
        self.tiles.warmup.cancel();
        self.tiles.zoom_out_warmup = None;
        self.tiles.arrival_warmup = None;
        let (tx, rx) = mpsc::channel();
        self.tiles.controls.clear_rx = Some(rx);
        self.tiles.controls.usage_rx = None;
        ctx.request_repaint();
        let (cache, ctx) = (self.tiles.cache.clone(), ctx.clone());
        self.runtime.spawn(async move {
            let _ = tx.send(cache.clear_disk_cache().await);
            ctx.request_repaint();
        });
    }
    pub(super) fn poll_cache_clear(&mut self, ctx: &egui::Context) {
        if let Some(remaining) = self.tiles.controls.poll_clear_notice(
            &mut self.status,
            self.lang.text("app-ready-status"),
            Instant::now(),
        ) {
            ctx.request_repaint_after(remaining);
        }
        self.poll_disk_usage();
        let Some(rx) = self.tiles.controls.clear_rx.take() else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                self.tiles.controls.clear_rx = Some(rx);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(globecraft_data::Error::msg("cache clearing task stopped"))
            }
        };
        self.tiles.controls.last_usage_scan = None;
        self.tiles.controls.usage = None;
        self.tiles.controls.clear_notice = None;
        match result {
            Ok(()) => {
                self.status = self.lang.text("settings-cache-cleared-status").into();
                self.tiles.controls.clear_notice = Some(CacheClearNotice {
                    message: self.status.clone(),
                    expires_at: Instant::now() + CACHE_CLEAR_NOTICE_DURATION,
                });
                ctx.request_repaint_after(CACHE_CLEAR_NOTICE_DURATION);
            }
            Err(error) => {
                self.status = Lang::En.format(
                    "settings-cache-clear-error",
                    &[("error", error.to_string().into())],
                );
            }
        }
    }

    // Invoked by Settings only; scans are asynchronous and throttled, never per frame.
    pub(super) fn refresh_disk_usage(&mut self, ctx: &egui::Context) {
        if self.tiles.controls.clearing() || self.tiles.controls.usage_rx.is_some() {
            return;
        }
        if self
            .tiles
            .controls
            .last_usage_scan
            .is_some_and(|last| last.elapsed() < DISK_USAGE_REFRESH_INTERVAL)
        {
            ctx.request_repaint_after(DISK_USAGE_REFRESH_INTERVAL);
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.tiles.controls.usage_rx = Some(rx);
        self.tiles.controls.last_usage_scan = Some(Instant::now());
        let (cache, ctx) = (self.tiles.cache.clone(), ctx.clone());
        self.runtime.spawn(async move {
            let _ = tx.send(cache.disk_usage_bytes().await);
            ctx.request_repaint();
        });
    }

    fn poll_disk_usage(&mut self) {
        let Some(rx) = self.tiles.controls.usage_rx.take() else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(mpsc::TryRecvError::Empty) => {
                self.tiles.controls.usage_rx = Some(rx);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("cache measurement task stopped".to_owned())
            }
        };
        self.tiles.controls.usage = Some(result);
    }
}

/// How often the session's download totals go to the run log (stderr), when they changed.
const DOWNLOAD_LOG_INTERVAL: Duration = Duration::from_secs(60);

impl App {
    /// Writes where the bandwidth went to the run log, at most once a minute and only
    /// after new downloads: a bounded diagnostic, not a timing log.
    pub(super) fn log_downloads(&mut self) {
        use globecraft_data::download_stats::{DownloadKind, totals};
        let logged_bytes = self.frame.download_log.bytes;
        if self.frame.download_log.at.elapsed() < DOWNLOAD_LOG_INTERVAL {
            return;
        }
        let imagery = totals(DownloadKind::Imagery);
        let ahead = totals(DownloadKind::ImageryAhead);
        let borders = totals(DownloadKind::BorderTiles);
        let disk = totals(DownloadKind::ImageryFromDisk);
        let bytes = imagery.bytes + ahead.bytes + borders.bytes + disk.bytes;
        self.frame.download_log = super::frame::DownloadLog {
            at: Instant::now(),
            bytes,
        };
        if bytes != logged_bytes {
            globecraft_data::log_line!(
                "Downloaded this session: imagery for the view {} in {} tiles, imagery ahead {} in {} tiles, border tiles {} in {} tiles; imagery from disk {} in {} tiles ({} confirmed unchanged by the server); {} downloads stopped (tile left the view)",
                cache_size_text(imagery.bytes),
                imagery.responses,
                cache_size_text(ahead.bytes),
                ahead.responses,
                cache_size_text(borders.bytes),
                borders.responses,
                cache_size_text(disk.bytes),
                disk.responses,
                globecraft_data::download_stats::not_modified(),
                globecraft_data::download_stats::cancelled()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_clear_success_expires_at_deadline_in_both_languages() {
        let now = Instant::now();
        for lang in Lang::ALL {
            let mut status = lang.text("settings-cache-cleared-status").to_owned();
            let ready = lang.text("app-ready-status");
            let mut controls = CacheControls {
                clear_notice: Some(CacheClearNotice {
                    message: status.clone(),
                    expires_at: now + CACHE_CLEAR_NOTICE_DURATION,
                }),
                ..Default::default()
            };
            assert_eq!(
                controls.poll_clear_notice(&mut status, ready, now),
                Some(CACHE_CLEAR_NOTICE_DURATION)
            );
            assert_eq!(status, lang.text("settings-cache-cleared-status"));
            assert_eq!(
                controls.poll_clear_notice(&mut status, ready, now + CACHE_CLEAR_NOTICE_DURATION),
                None
            );
            assert_eq!(status, ready);
        }
    }

    #[test]
    fn notice_expiration_preserves_newer_messages_and_errors() {
        let now = Instant::now();
        for newer in ["Search in progress", "Settings persistence failed: fixture"] {
            let mut status = newer.to_owned();
            let mut controls = CacheControls {
                clear_notice: Some(CacheClearNotice {
                    message: Lang::En.text("settings-cache-cleared-status").into(),
                    expires_at: now,
                }),
                ..Default::default()
            };
            assert_eq!(controls.poll_clear_notice(&mut status, "Ready", now), None);
            assert_eq!(status, newer);
            assert!(controls.clear_notice.is_none());
        }
    }
}
