//! Background loads of the tile layer: the TileJSON, tiles from the disk cache and
//! tiles from the network. Each load updates the shared store and wakes the UI.
use super::compact::Filtered;
use super::content::{TileContent, TileKey};
use super::store::DiskStore;
use super::{Entry, Store};
use crate::boundaries::{StatusLog, log_status};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// GlobeCraft's own borders-and-names tile set: a manifest naming PMTiles archives
/// cut from the OpenFreeMap planet (only the layers drawn here, a few KB per tile
/// instead of 100-500 KB; `.github/workflows/border-tiles.yml`).
const MANIFEST_URL: &str =
    "https://github.com/FireflyTheReal/GlobeCraft/releases/download/tiles-borders/borders.json";
/// OpenFreeMap's TileJSON for the planet; it names the current tile set's URL. Used
/// when GlobeCraft's own tile set cannot be reached.
const TILEJSON_URL: &str = "https://tiles.openfreemap.org/planet";
/// Templates naming PMTiles archives start with this, followed by the archives'
/// addresses separated by spaces; each tile is the union of their layers.
const PMTILES_PREFIX: &str = "pmtiles:";
const MAX_MANIFEST_BYTES: u64 = 20_000;
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
    match fetch_manifest().await {
        Ok(template) => Ok(template),
        // OpenFreeMap's full tiles carry the same layers.
        Err(_) => fetch_tilejson().await,
    }
}

/// The archives GlobeCraft's manifest names, as a `pmtiles:` template.
async fn fetch_manifest() -> Result<String, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(client.get(MANIFEST_URL), MAX_MANIFEST_BYTES).await?;
    let json = crate::http::parse_json(bytes).await?;
    manifest_template(&json).ok_or_else(|| "border tile manifest without archives".into())
}

/// The tile source a manifest describes: `pmtiles:` followed by its sets separated
/// by spaces, each set's parts separated by `|`. A set holds some of the layers
/// (borders and names, water); its parts split it under GitHub's 2 GiB asset limit
/// (by zoom and longitude) and are tried in order. `"archives"` lists sets of one part.
fn manifest_template(json: &serde_json::Value) -> Option<String> {
    let urls = |list: &serde_json::Value| -> Option<Vec<String>> {
        let urls: Vec<String> = list
            .as_array()?
            .iter()
            .map(|url| url.as_str().map(str::to_owned))
            .collect::<Option<_>>()?;
        let valid = !urls.is_empty()
            && urls
                .iter()
                .all(|url| url.starts_with("https://") && !url.contains(' ') && !url.contains('|'));
        valid.then_some(urls)
    };
    let sets: Vec<Vec<String>> = match json.get("sets") {
        Some(sets) => sets.as_array()?.iter().map(urls).collect::<Option<_>>()?,
        None => urls(json.get("archives")?)?
            .into_iter()
            .map(|url| vec![url])
            .collect(),
    };
    (!sets.is_empty()).then(|| {
        let sets: Vec<String> = sets.iter().map(|parts| parts.join("|")).collect();
        format!("{PMTILES_PREFIX}{}", sets.join(" "))
    })
}

async fn fetch_tilejson() -> Result<String, crate::Error> {
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
    if let Some(archives) = template.strip_prefix(PMTILES_PREFIX) {
        return fetch_archived_tile(key, archives).await;
    }
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

/// A tile from GlobeCraft's PMTiles archives: each holds some of the layers, and a
/// vector tile's layers are a repeated field, so the decompressed tiles simply
/// concatenate. An archive without the tile adds nothing (open sea).
async fn fetch_archived_tile(key: TileKey, archives: &str) -> Result<Filtered, crate::Error> {
    let mut joined = Vec::new();
    for set in archives.split(' ') {
        // The parts of a set are disjoint but for tiles on a band's edge, which are
        // the same in both: the first part holding the tile gives it.
        for url in set.split('|') {
            let archive = archive(url).await?;
            if let Some(bytes) = archive.tile(key.z, key.x, key.y).await? {
                crate::download_stats::record(
                    crate::download_stats::DownloadKind::BorderTiles,
                    bytes.len(),
                );
                joined.push(bytes);
                break;
            }
        }
    }
    crate::cpu::run(crate::cpu::Service::Features, move || {
        let mut tile = Vec::new();
        for bytes in joined {
            tile.extend(gunzip_if_needed(bytes)?);
        }
        Filtered::from_mvt(&tile).ok_or_else(|| {
            crate::Error::from(format!(
                "malformed vector tile {}/{}/{}",
                key.z, key.x, key.y
            ))
        })
    })
    .await
}

/// The archive at `url`, opened once per session (its header and root directory).
async fn archive(url: &str) -> Result<Arc<crate::pmtiles::Archive>, crate::Error> {
    static OPEN: std::sync::OnceLock<
        tokio::sync::Mutex<std::collections::HashMap<String, Arc<crate::pmtiles::Archive>>>,
    > = std::sync::OnceLock::new();
    let mut open = OPEN.get_or_init(Default::default).lock().await;
    if let Some(archive) = open.get(url) {
        return Ok(archive.clone());
    }
    let archive =
        Arc::new(crate::pmtiles::Archive::open(url, crate::http::Policy::Boundary).await?);
    open.insert(url.to_owned(), archive.clone());
    Ok(archive)
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

#[cfg(test)]
mod manifest_tests {
    use super::manifest_template;

    #[test]
    fn manifests_name_sets_of_archive_parts() {
        let split = serde_json::json!({
            "version": "1",
            "sets": [["https://a/b0.pmtiles", "https://a/b1.pmtiles"], ["https://a/w.pmtiles"]],
        });
        assert_eq!(
            manifest_template(&split).as_deref(),
            Some("pmtiles:https://a/b0.pmtiles|https://a/b1.pmtiles https://a/w.pmtiles")
        );
        let plain = serde_json::json!({"archives": ["https://a/b.pmtiles", "https://a/w.pmtiles"]});
        assert_eq!(
            manifest_template(&plain).as_deref(),
            Some("pmtiles:https://a/b.pmtiles https://a/w.pmtiles")
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"sets": []}),
            serde_json::json!({"sets": [[]]}),
            serde_json::json!({"sets": [["http://a/b.pmtiles"]]}),
            serde_json::json!({"archives": ["https://a/b|c.pmtiles"]}),
        ] {
            assert_eq!(manifest_template(&bad), None, "{bad}");
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::{TileKey, fetch_archived_tile, fetch_manifest};

    fn key(z: u8, lat: f64, lon: f64) -> TileKey {
        let n = f64::from(1u32 << z);
        let x = ((lon + 180.0) / 360.0 * n) as u32;
        let s = lat.to_radians();
        let y = ((1.0 - (s.tan() + 1.0 / s.cos()).ln() / std::f64::consts::PI) / 2.0 * n) as u32;
        TileKey { z, x, y }
    }

    /// The published border archives, read as the app reads them: every part of the
    /// split set answers tiles on a border.
    #[tokio::test]
    #[ignore = "network: reads GlobeCraft's published border tile archives"]
    async fn published_border_parts_serve_border_tiles() {
        let template = fetch_manifest().await.unwrap();
        let archives = template
            .strip_prefix(super::PMTILES_PREFIX)
            .unwrap()
            .to_owned();
        // Germany–Austria near Salzburg (east of 0°) and Kansas–Missouri (west):
        // zoom 5 and 10 from the low part, 11 from its own, 12 from either half.
        for (z, lat, lon) in [
            (5, 47.7, 13.0),
            (10, 47.7, 13.0),
            (11, 47.7, 13.0),
            (12, 47.7, 13.0),
            (12, 39.0, -94.61),
        ] {
            let tile = fetch_archived_tile(key(z, lat, lon), &archives)
                .await
                .unwrap();
            println!(
                "z{z} {lat},{lon}: {} lines, {} places",
                tile.lines.len(),
                tile.places.len()
            );
            assert!(!tile.lines.is_empty(), "z{z} {lat},{lon}");
        }
    }
}
