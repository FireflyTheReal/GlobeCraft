//! Background loads of the tile layer: the manifest, tiles from the disk cache and
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
/// Templates naming PMTiles archives start with this, followed by the archives'
/// addresses separated by spaces; each tile is the union of their layers.
const PMTILES_PREFIX: &str = "pmtiles:";
const MAX_MANIFEST_BYTES: u64 = 20_000;
const MAX_TILE_BYTES: u64 = 4_000_000;

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
        // Only GlobeCraft's own archives: a template from before (OpenFreeMap's) is
        // not used again.
        if store.template.is_none()
            && let Ok(Some(template)) = cached
            && template.starts_with(PMTILES_PREFIX)
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

/// Border tiles come only from GlobeCraft's own archives on GitHub. Without the
/// manifest the request is retried; meanwhile the last manifest from the disk cache
/// is used, or the bundled Natural Earth borders are drawn.
async fn fetch_template() -> Result<String, crate::Error> {
    fetch_manifest().await
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
    // The sets (borders and names, water) are read at the same time: one after the
    // other doubled the wait for every tile.
    let mut reads = tokio::task::JoinSet::new();
    for (index, set) in archives.split(' ').enumerate() {
        let set = set.to_owned();
        reads.spawn(async move { (index, set_tile(key, &set).await) });
    }
    let mut found = Vec::new();
    while let Some(read) = reads.join_next().await {
        let (index, bytes) = read.map_err(|error| crate::Error::from(error.to_string()))?;
        if let Some(bytes) = bytes? {
            found.push((index, bytes));
        }
    }
    found.sort_by_key(|(index, _)| *index);
    let joined: Vec<Vec<u8>> = found.into_iter().map(|(_, bytes)| bytes).collect();
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

/// Tile `key` from one set: its parts are disjoint but for tiles on a band's edge,
/// which are the same in both, so the first part holding the tile gives it.
async fn set_tile(key: TileKey, set: &str) -> Result<Option<Vec<u8>>, crate::Error> {
    for url in set.split('|') {
        if let Some(bytes) = archive(url).await?.tile(key.z, key.x, key.y).await? {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
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

/// GlobeCraft's archives on GitHub against OpenFreeMap's full tiles, the source used
/// before (roadmap 3.4): the tiles a session of typical views wants, fetched four at a
/// time as the app does, with bytes over the wire, requests, time and content.
/// `GLOBECRAFT_BORDER_BENCH_OPENFREEMAP=0` skips OpenFreeMap (it is not reachable
/// from every network). Run on a CI runner by `.github/workflows/border-bench.yml`.
#[cfg(test)]
mod source_benchmark {
    use super::super::pick::wanted;
    use super::{Filtered, TileKey, fetch_archived_tile, fetch_manifest, gunzip_if_needed};
    use std::sync::Arc;
    use std::time::Instant;

    const PARALLEL: usize = 4;
    const VIEWS: [(&str, f64, f64, f64, f64); 10] = [
        ("globe", 20.0, 10.0, 14_000.0, 0.0),
        ("Europe", 50.0, 10.0, 3_000.0, 0.0),
        ("Germany", 51.0, 10.0, 900.0, 0.0),
        ("Bavaria", 48.5, 11.5, 250.0, 0.0),
        ("Munich", 48.14, 11.58, 30.0, 0.0),
        ("Alps tilted", 47.3, 11.4, 20.0, 60.0),
        ("USA", 39.0, -98.0, 2_500.0, 0.0),
        ("Brazil", -15.0, -50.0, 2_500.0, 0.0),
        ("India", 22.0, 79.0, 1_500.0, 30.0),
        ("Tokyo tilted", 35.68, 139.7, 40.0, 45.0),
    ];

    /// What one source returned for one tile.
    #[derive(Clone, Copy, Default)]
    struct Content {
        lines: usize,
        points: usize,
        places: usize,
    }
    fn content(tile: &Filtered) -> Content {
        Content {
            lines: tile.lines.len(),
            points: tile.lines.iter().map(|line| line.points.len()).sum(),
            places: tile.places.len(),
        }
    }

    /// Fetches `keys` with `PARALLEL` at a time; `fetch` returns the tile and the bytes
    /// it moved when the source does not count them itself.
    async fn fetch_all<F, Fut>(keys: &[TileKey], fetch: F) -> (f64, Vec<Option<Content>>, u64)
    where
        F: Fn(TileKey) -> Fut,
        Fut: std::future::Future<Output = Result<(Filtered, u64), crate::Error>> + Send + 'static,
    {
        let start = Instant::now();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(PARALLEL));
        let mut tasks = tokio::task::JoinSet::new();
        for (index, key) in keys.iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let future = fetch(*key);
            tasks.spawn(async move {
                let result = future.await;
                drop(permit);
                (index, result)
            });
        }
        let mut contents = vec![None; keys.len()];
        let mut bytes = 0;
        while let Some(done) = tasks.join_next().await {
            let (index, result) = done.unwrap();
            match result {
                Ok((tile, moved)) => {
                    contents[index] = Some(content(&tile));
                    bytes += moved;
                }
                Err(error) => println!("  tile {:?} failed: {error}", keys[index]),
            }
        }
        (start.elapsed().as_secs_f64() * 1000.0, contents, bytes)
    }

    async fn openfreemap_template() -> String {
        let json: serde_json::Value = reqwest::get("https://tiles.openfreemap.org/planet")
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        json["tiles"][0].as_str().unwrap().to_owned()
    }

    /// One full OpenFreeMap tile, as transferred (no transparent decompression).
    async fn openfreemap_tile(
        client: reqwest::Client,
        template: String,
        key: TileKey,
    ) -> Result<(Filtered, u64), crate::Error> {
        let url = template
            .replace("{z}", &key.z.to_string())
            .replace("{x}", &key.x.to_string())
            .replace("{y}", &key.y.to_string());
        let response = client.get(&url).send().await?;
        let bytes = if response.status() == reqwest::StatusCode::NOT_FOUND {
            Vec::new()
        } else {
            response.error_for_status()?.bytes().await?.to_vec()
        };
        let moved = bytes.len() as u64;
        let tile = Filtered::from_mvt(&gunzip_if_needed(bytes)?).ok_or("malformed tile")?;
        Ok((tile, moved))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "network: compares the border tile sources"]
    async fn border_tile_sources() {
        let with_openfreemap =
            std::env::var("GLOBECRAFT_BORDER_BENCH_OPENFREEMAP").as_deref() != Ok("0");
        let template = fetch_manifest().await.unwrap();
        let archives: Arc<str> = template.strip_prefix(super::PMTILES_PREFIX).unwrap().into();
        let openfreemap = if with_openfreemap {
            Some(openfreemap_template().await)
        } else {
            None
        };
        let plain = reqwest::Client::builder().no_gzip().build().unwrap();
        println!(
            "| View | Tiles | GitHub cold KiB | of it tiles | requests | ms | GitHub warm ms | OpenFreeMap KiB | requests | ms | Tiles differing |"
        );
        println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
        let mut totals = [0f64; 7];
        let mut differing_total = 0;
        let mut empty_on_github = Vec::new();
        for (name, lat, lon, altitude, tilt) in VIEWS {
            let (_, keys) = wanted(lat, lon, altitude, tilt, true);
            let (requests_before, bytes_before) = crate::pmtiles::transferred();
            let fetch = |key| {
                let archives = archives.clone();
                async move {
                    fetch_archived_tile(key, &archives)
                        .await
                        .map(|tile| (tile, 0))
                }
            };
            let (cold_ms, github, _) = fetch_all(&keys, fetch).await;
            let (requests_after, bytes_after) = crate::pmtiles::transferred();
            // Warm: the directories are cached, so what moves is the tiles alone.
            let (warm_ms, _, _) = fetch_all(&keys, fetch).await;
            let tile_kib = (crate::pmtiles::transferred().1 - bytes_after) as f64 / 1024.0;
            let github_requests = requests_after - requests_before;
            let github_kib = (bytes_after - bytes_before) as f64 / 1024.0;
            let (mut ofm_kib, mut ofm_requests, mut ofm_ms, mut differing) =
                (f64::NAN, 0usize, f64::NAN, 0usize);
            if let Some(template) = &openfreemap {
                let fetch = |key| openfreemap_tile(plain.clone(), template.clone(), key);
                let (ms, contents, bytes) = fetch_all(&keys, fetch).await;
                (ofm_kib, ofm_requests, ofm_ms) = (bytes as f64 / 1024.0, keys.len(), ms);
                for ((key, ours), theirs) in keys.iter().zip(&github).zip(&contents) {
                    let (Some(ours), Some(theirs)) = (ours, theirs) else {
                        continue;
                    };
                    if ours.lines != theirs.lines || ours.places != theirs.places {
                        differing += 1;
                    }
                    if ours.lines == 0 && theirs.lines > 0 {
                        empty_on_github.push((*key, theirs.lines, theirs.points));
                    }
                }
            }
            println!(
                "| {name} | {} | {github_kib:.0} | {tile_kib:.0} | {github_requests} | {cold_ms:.0} | {warm_ms:.0} | {ofm_kib:.0} | {ofm_requests} | {ofm_ms:.0} | {differing} |",
                keys.len()
            );
            for (total, value) in totals.iter_mut().zip([
                keys.len() as f64,
                github_kib,
                github_requests as f64,
                cold_ms,
                ofm_kib,
                ofm_requests as f64,
                ofm_ms,
            ]) {
                *total += value;
            }
            differing_total += differing;
            assert!(
                github.iter().all(Option::is_some),
                "{name}: a GitHub tile failed"
            );
        }
        println!(
            "| total | {:.0} | {:.0} | | {:.0} | {:.0} | | {:.0} | {:.0} | {:.0} | {differing_total} |",
            totals[0], totals[1], totals[2], totals[3], totals[4], totals[5], totals[6]
        );
        for (key, lines, points) in &empty_on_github {
            println!(
                "GAP: {}/{}/{} has {lines} lines ({points} points) on OpenFreeMap, none on GitHub",
                key.z, key.x, key.y
            );
        }
    }
}
