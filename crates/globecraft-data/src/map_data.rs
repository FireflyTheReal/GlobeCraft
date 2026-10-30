//! GlobeCraft's own map data: one `map-data` pre-release on GitHub whose
//! `manifest.json` names, for each set (borders, transport), the PMTiles archives
//! it is in (`.github/workflows/map-data.yml`). Readers ask for a set's template and
//! read tiles through it.
use crate::pmtiles::TileLocation;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MANIFEST_URL: &str =
    "https://github.com/FireflyTheReal/GlobeCraft/releases/download/map-data/manifest.json";
/// Templates naming PMTiles archives start with this, followed by the sets separated
/// by spaces, each set's parts separated by `|`.
pub(crate) const PMTILES_PREFIX: &str = "pmtiles:";
const MAX_MANIFEST_BYTES: u64 = 20_000;
/// One manifest serves every reader for this long.
const MANIFEST_REUSE: Duration = Duration::from_secs(10 * 60);

/// The `pmtiles:` template of manifest entry `set`.
pub(crate) async fn template(set: &str) -> Result<String, crate::Error> {
    let json = manifest().await?;
    manifest_template(&json[set])
        .ok_or_else(|| format!("map data manifest without the {set} set").into())
}

/// One set read from several manifest entries: `set` and its zoom bands (`bands`,
/// each built by a run of its own, e.g. the roads of zoom 13 and of zoom 14), as one
/// set of parts; each part's header says which zooms it holds. Bands not published
/// yet are left out.
pub(crate) async fn banded_template(set: &str, bands: &[&str]) -> Result<String, crate::Error> {
    let json = manifest().await?;
    let parts: Vec<String> = std::iter::once(set)
        .chain(bands.iter().copied())
        .filter_map(|key| manifest_template(&json[key]))
        .filter_map(|template| template.strip_prefix(PMTILES_PREFIX).map(str::to_owned))
        // A banded entry is one set; its parts join the others'.
        .map(|template| template.replace(' ', "|"))
        .collect();
    if parts.is_empty() {
        return Err(format!("map data manifest without the {set} set").into());
    }
    Ok(format!("{PMTILES_PREFIX}{}", parts.join("|")))
}

async fn manifest() -> Result<Arc<serde_json::Value>, crate::Error> {
    static LAST: std::sync::Mutex<Option<(Instant, Arc<serde_json::Value>)>> =
        std::sync::Mutex::new(None);
    if let Some((at, json)) = LAST.lock().expect("manifest cache").clone()
        && at.elapsed() < MANIFEST_REUSE
    {
        return Ok(json);
    }
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(client.get(MANIFEST_URL), MAX_MANIFEST_BYTES).await?;
    let json = Arc::new(crate::http::parse_json(bytes).await?);
    *LAST.lock().expect("manifest cache") = Some((Instant::now(), json.clone()));
    Ok(json)
}

/// The tile source a manifest describes: `pmtiles:` followed by its sets separated
/// by spaces, each set's parts separated by `|`. A set holds some of the layers
/// (borders and names, water); its parts split it under GitHub's 2 GiB asset limit
/// (by zoom and longitude) and are tried in order. `"archives"` lists sets of one part.
pub(crate) fn manifest_template(json: &serde_json::Value) -> Option<String> {
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

/// Tile `z/x/y` from every set of `archives` (a template without its prefix), each
/// still compressed, in the sets' order. A set without the tile adds nothing (open
/// sea, or nothing of that set there).
pub(crate) async fn tile_parts(
    z: u8,
    x: u32,
    y: u32,
    archives: &str,
) -> Result<Vec<Vec<u8>>, crate::Error> {
    let mut parts = tile_parts_many(&[(z, x, y)], archives).await?;
    Ok(parts.pop().unwrap_or_default())
}

/// Tiles from every set of `archives`, each still compressed, in the sets' order.
/// All parts of a set are located at the same time and exactly contiguous tile byte
/// ranges are read together: a view's tiles sit next to each other in the archive,
/// so tens of tile requests become a handful without transferring extra bytes.
pub(crate) async fn tile_parts_many(
    tiles: &[(u8, u32, u32)],
    archives: &str,
) -> Result<Vec<Vec<Vec<u8>>>, crate::Error> {
    if tiles.is_empty() {
        return Ok(Vec::new());
    }
    let sets: Vec<Vec<String>> = archives
        .split(' ')
        .map(|set| set.split('|').map(str::to_owned).collect())
        .collect();
    let mut locate_jobs = tokio::task::JoinSet::new();
    for (set_index, parts) in sets.iter().enumerate() {
        let parts = parts.clone();
        let tiles = tiles.to_vec();
        locate_jobs.spawn(async move {
            locate_set(&tiles, &parts)
                .await
                .map(|located| (set_index, located))
        });
    }
    let mut located: Vec<Vec<Option<(usize, TileLocation)>>> =
        vec![vec![None; tiles.len()]; sets.len()];
    while let Some(result) = locate_jobs.join_next().await {
        let (set_index, found) =
            result.map_err(|error| crate::Error::from(error.to_string()))??;
        located[set_index] = found;
    }

    let mut groups: std::collections::HashMap<String, Vec<(usize, usize, TileLocation)>> =
        std::collections::HashMap::new();
    for (set_index, found) in located.iter().enumerate() {
        for (tile_index, location) in found.iter().enumerate() {
            if let Some((part_index, location)) = location {
                groups
                    .entry(sets[set_index][*part_index].clone())
                    .or_default()
                    .push((tile_index, set_index, *location));
            }
        }
    }
    let mut parts: Vec<Vec<Vec<u8>>> = vec![vec![Vec::new(); sets.len()]; tiles.len()];
    let mut reads = tokio::task::JoinSet::new();
    for (url, members) in groups {
        reads.spawn(async move {
            let archive = archive(&url).await?;
            let ranges: Vec<(u64, u32)> = members
                .iter()
                .map(|(_, _, location)| (location.offset, location.length))
                .collect();
            let runs = coalesced_runs(&ranges);
            let mut run_bytes = Vec::with_capacity(runs.len());
            for &(start, end) in &runs {
                run_bytes.push(archive.range(start, end - start).await?);
            }
            let mut found = Vec::with_capacity(members.len());
            for (tile_index, set_index, location) in members {
                let run = runs
                    .partition_point(|(start, _)| *start <= location.offset)
                    .saturating_sub(1);
                let (start, end) = runs[run];
                let local = usize::try_from(location.offset - start)
                    .map_err(|_| crate::Error::from("bad PMTiles tile offset"))?;
                let length = location.length as usize;
                let window = run_bytes[run]
                    .get(local..local + length)
                    .ok_or_else(|| crate::Error::from("truncated PMTiles tile range"))?;
                debug_assert!(location.offset + u64::from(location.length) <= end);
                found.push((tile_index, set_index, window.to_vec()));
            }
            Ok::<_, crate::Error>(found)
        });
    }
    while let Some(result) = reads.join_next().await {
        let found = result.map_err(|error| crate::Error::from(error.to_string()))??;
        for (tile_index, set_index, bytes) in found {
            parts[tile_index][set_index] = bytes;
        }
    }
    Ok(parts)
}

/// Part locations for `tiles` in one set, opening and searching all its parts at the
/// same time. Bands can repeat a tile at their edge; the first part to report it wins
/// and the remaining probes are cancelled once every tile has a home.
async fn locate_set(
    tiles: &[(u8, u32, u32)],
    parts: &[String],
) -> Result<Vec<Option<(usize, TileLocation)>>, crate::Error> {
    let mut jobs = tokio::task::JoinSet::new();
    for (part_index, url) in parts.iter().enumerate() {
        let url = url.clone();
        let tiles = tiles.to_vec();
        jobs.spawn(async move {
            let archive = archive(&url).await?;
            let mut found = Vec::new();
            for (tile_index, (z, x, y)) in tiles.into_iter().enumerate() {
                if let Some(location) = archive.locate(z, x, y).await? {
                    found.push((tile_index, location));
                }
            }
            Ok::<_, crate::Error>((part_index, found))
        });
    }
    let mut located = vec![None; tiles.len()];
    let mut remaining = tiles.len();
    let mut first_error = None;
    while remaining > 0 {
        let Some(result) = jobs.join_next().await else {
            break;
        };
        match result {
            Ok(Ok((part_index, found))) => {
                for (tile_index, location) in found {
                    if located[tile_index].is_none() {
                        located[tile_index] = Some((part_index, location));
                        remaining -= 1;
                    }
                }
            }
            Ok(Err(error)) => {
                first_error.get_or_insert(error);
            }
            Err(error) => {
                first_error.get_or_insert_with(|| crate::Error::from(error.to_string()));
            }
        }
    }
    if remaining > 0 {
        jobs.abort_all();
        while jobs.join_next().await.is_some() {}
        // A failed probe may have held one of the tiles still missing; report it
        // instead of treating those tiles as absent.
        if let Some(error) = first_error {
            return Err(error);
        }
        // The rest were absent (open sea, or nothing of that set there).
    }
    Ok(located)
}

/// The largest one coalesced run may grow to. Tile groups are already bounded by the
/// stream's batch size; this only keeps a stray long run from becoming one huge read.
const MAX_COALESCED_RUN_BYTES: u64 = 8 * 1024 * 1024;

/// Exactly contiguous, sorted tile ranges merged into single reads. Duplicate ranges
/// (a run entry shared by several tiles) are kept once.
fn coalesced_runs(ranges: &[(u64, u32)]) -> Vec<(u64, u64)> {
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut runs: Vec<(u64, u64)> = Vec::new();
    for (offset, length) in sorted {
        let end = offset + u64::from(length);
        if let Some((start, run_end)) = runs.last_mut()
            && offset == *run_end
            && end - *start <= MAX_COALESCED_RUN_BYTES
        {
            *run_end = end;
            continue;
        }
        runs.push((offset, end));
    }
    runs
}

/// The archive at `url`, opened once per session (its header and root directory).
/// The map only guards the cache entries; each entry is initialized outside that
/// lock, so different archive parts open concurrently.
async fn archive(url: &str) -> Result<Arc<crate::pmtiles::Archive>, crate::Error> {
    type Cell = tokio::sync::OnceCell<Arc<crate::pmtiles::Archive>>;
    static OPEN: std::sync::OnceLock<
        tokio::sync::Mutex<std::collections::HashMap<String, Arc<Cell>>>,
    > = std::sync::OnceLock::new();
    let cell = {
        let mut open = OPEN.get_or_init(Default::default).lock().await;
        open.entry(url.to_owned()).or_default().clone()
    };
    cell.get_or_try_init(|| async {
        crate::pmtiles::Archive::open(url, crate::http::Policy::Boundary)
            .await
            .map(Arc::new)
    })
    .await
    .cloned()
}

/// Tiles are often stored gzip-compressed; servers that pass them on without a
/// `Content-Encoding` header leave the decompression to the client.
pub(crate) fn gunzip_if_needed(bytes: Vec<u8>) -> Result<Vec<u8>, crate::Error> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes);
    }
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod manifest_tests {
    use super::{PMTILES_PREFIX, coalesced_runs, manifest_template, tile_parts_many};

    #[test]
    fn contiguous_tile_ranges_share_one_read() {
        assert_eq!(
            coalesced_runs(&[(0, 10), (10, 5), (100, 1), (20, 1), (10, 5)]),
            vec![(0, 15), (20, 21), (100, 101)]
        );
        assert!(coalesced_runs(&[]).is_empty());
    }

    #[tokio::test]
    #[ignore = "network: reads GlobeCraft's published places tiles"]
    async fn a_places_view_batch_reads_its_tiles_together() {
        let template = super::template("poi").await.unwrap();
        let archives = template.strip_prefix(PMTILES_PREFIX).unwrap();
        let wanted = crate::boundaries::osm_tiles::pick::around(14, 48.14, 11.56, 0.08);
        let tiles: Vec<(u8, u32, u32)> = wanted.iter().map(|key| (key.z, key.x, key.y)).collect();
        let (requests_before, bytes_before) = crate::pmtiles::transferred();
        let started = std::time::Instant::now();
        let parts = tile_parts_many(&tiles, archives).await.unwrap();
        let elapsed = started.elapsed();
        let (requests_after, bytes_after) = crate::pmtiles::transferred();
        println!(
            "{} places tiles in {:?}: {} requests, {} bytes",
            tiles.len(),
            elapsed,
            requests_after - requests_before,
            bytes_after - bytes_before
        );
        assert_eq!(parts.len(), tiles.len());
        assert!(parts.iter().any(|parts| !parts.is_empty()));
    }

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
