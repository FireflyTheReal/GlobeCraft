//! Bytes downloaded this session per kind of data, so the app can show where the
//! bandwidth goes (Settings → Cache) and log it for diagnosis. Counts response bodies
//! as received (before decoding images or unpacking gzip-stored vector tiles).
use std::sync::atomic::{AtomicU64, Ordering};

/// A kind of download counted here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadKind {
    /// Imagery and terrain tiles.
    Imagery,
    /// OpenStreetMap vector tiles for borders and names.
    BorderTiles,
}

struct Counter {
    bytes: AtomicU64,
    responses: AtomicU64,
}
impl Counter {
    const fn new() -> Self {
        Self {
            bytes: AtomicU64::new(0),
            responses: AtomicU64::new(0),
        }
    }
}

static IMAGERY: Counter = Counter::new();
static BORDER_TILES: Counter = Counter::new();

fn counter(kind: DownloadKind) -> &'static Counter {
    match kind {
        DownloadKind::Imagery => &IMAGERY,
        DownloadKind::BorderTiles => &BORDER_TILES,
    }
}

pub(crate) fn record(kind: DownloadKind, bytes: usize) {
    let counter = counter(kind);
    counter.bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    counter.responses.fetch_add(1, Ordering::Relaxed);
}

/// Bytes and responses of one kind so far this session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadTotals {
    pub bytes: u64,
    pub responses: u64,
}

pub fn totals(kind: DownloadKind) -> DownloadTotals {
    let counter = counter(kind);
    DownloadTotals {
        bytes: counter.bytes.load(Ordering::Relaxed),
        responses: counter.responses.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_add_up_per_kind() {
        let before = totals(DownloadKind::BorderTiles);
        record(DownloadKind::BorderTiles, 1_000);
        record(DownloadKind::BorderTiles, 500);
        let after = totals(DownloadKind::BorderTiles);
        assert!(after.bytes >= before.bytes + 1_500);
        assert!(after.responses >= before.responses + 2);
    }
}
