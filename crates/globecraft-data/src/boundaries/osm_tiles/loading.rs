//! Background loads of the tile layer: the TileJSON, tiles from the disk cache and
//! tiles from the network. Each load updates the shared store and wakes the UI.
use super::compact::Filtered;
use super::content::{TileContent, TileKey};
use super::store::DiskStore;
use super::{Entry, Store};
use crate::boundaries::{StatusLog, log_status};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// OpenFreeMap's TileJSON for the planet; it names the current tile set's URL.
const TILEJSON_URL: &str = "https://tiles.openfreemap.org/planet";
const MAX_TILE_BYTES: u64 = 4_000_000;
const MAX_TILEJSON_BYTES: u64 = 200_000;

/// What a background load needs.
#[derive(Clone)]
pub(super) struct Context {
    pub(super) store: Arc<Mutex<Store>>,
    pub(super) disk: Arc<DiskStore>,
    pub(super) log: StatusLog,
    pub(super) repaint: Arc<dyn Fn() + Send + Sync>,
}

impl Context {
    fn finish(&self, update: impl FnOnce(&mut Store)) {
        if let Ok(mut store) = self.store.lock() {
            update(&mut store);
        }
        (self.repaint)();
    }
}

/// The tile set last used, from the disk cache, so an offline start still has one.
pub(super) async fn cached_template(context: Context) {
    let cached = context.disk.template().await;
    context.finish(|store| {
        if store.template.is_none()
            && let Ok(Some(template)) = cached
        {
            store.template = Some(template.into());
            store.revision += 1;
        }
    });
}

/// The current tile set from the network. A new set makes the shown tiles stale; they
/// stay on screen and are refetched as the view needs them.
pub(super) async fn network_template(context: Context) {
    let result = fetch_template().await;
    if let Ok(template) = &result {
        // Optional persistence: a failed write only costs the offline start.
        let _ = context.disk.set_template(template.clone()).await;
    }
    let log = context.log.clone();
    context.finish(|store| {
        store.template_loading = false;
        match result {
            Ok(template) => {
                if store.template.as_deref() != Some(template.as_str()) {
                    log_status(&log, format!("✔ OpenStreetMap border tiles: {template}"));
                    store.template = Some(template.into());
                    store.revision += 1;
                }
                store.template_confirmed = true;
            }
            Err(error) => {
                log_status(&log, format!("✖ OpenStreetMap border tiles: {error}"));
                store.template_failed = Some(Instant::now());
            }
        }
    });
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

/// A tile from the disk cache. A miss (or a damaged row) leaves it to the network.
pub(super) async fn from_disk(context: Context, key: TileKey, version: Arc<str>) {
    let stored = context.disk.read(key, version.to_string()).await;
    let decoded = match stored {
        Ok(Some(stored)) => {
            let current = stored.current;
            crate::cpu::run(crate::cpu::Service::Features, move || {
                Ok(Filtered::decode(&stored.bytes).map(|f| (f, current)))
            })
            .await
            .ok()
            .flatten()
        }
        _ => None,
    };
    context.finish(|store| {
        store.disk_inflight.remove(&key);
        store.disk_checked.insert(key);
        if let Some((filtered, current)) = decoded
            && !store.tiles.contains_key(&key)
        {
            // A tile of an older set is shown until the current one arrives.
            let version = if current { version } else { Arc::from("") };
            store.insert(key, entry(key, &filtered, version));
        }
    });
}

/// A tile from the network, kept on disk in the compact format.
pub(super) async fn from_network(context: Context, key: TileKey, version: Arc<str>, limit: u64) {
    let result = fetch_tile(key, &version).await;
    let result = match result {
        Ok(filtered) => {
            let encoded = filtered.encode();
            if let Err(error) = context
                .disk
                .write(key, version.to_string(), encoded, limit)
                .await
            {
                log_status(&context.log, format!("✖ Border tile cache: {error}"));
            }
            Ok(filtered)
        }
        Err(error) => Err(error),
    };
    let log = context.log.clone();
    context.finish(|store| {
        store.net_inflight.remove(&key);
        match result {
            Ok(filtered) => store.insert(key, entry(key, &filtered, version)),
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
    });
}

fn entry(key: TileKey, filtered: &Filtered, version: Arc<str>) -> Entry {
    Entry {
        content: Arc::new(TileContent::from_filtered(key, filtered)),
        used: Instant::now(),
        version,
    }
}

async fn fetch_tile(key: TileKey, template: &str) -> Result<Filtered, crate::Error> {
    let url = template
        .replace("{z}", &key.z.to_string())
        .replace("{x}", &key.x.to_string())
        .replace("{y}", &key.y.to_string());
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = match crate::http::body(client.get(&url), MAX_TILE_BYTES).await {
        Ok(bytes) => {
            crate::download_stats::record(
                crate::download_stats::DownloadKind::BorderTiles,
                bytes.len(),
            );
            bytes
        }
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
        Filtered::from_mvt(&bytes)
            .ok_or_else(|| crate::Error::from(format!("malformed vector tile {url}")))
    })
    .await
}

/// Tiles are often stored gzip-compressed; servers that pass them on without a
/// `Content-Encoding` header leave the decompression to the client.
pub(super) fn gunzip_if_needed(bytes: Vec<u8>) -> Result<Vec<u8>, crate::Error> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes);
    }
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut out)?;
    Ok(out)
}
