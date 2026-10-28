//! GlobeCraft's own map data: one `map-data` pre-release on GitHub whose
//! `manifest.json` names, for each set (borders, transport), the PMTiles archives
//! it is in (`.github/workflows/map-data.yml`). Readers ask for a set's template and
//! read tiles through it.
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
/// still compressed, in the sets' order. The sets are read at the same time: one
/// after the other doubled the wait for every tile. A set without the tile adds
/// nothing (open sea, or nothing of that set there).
pub(crate) async fn tile_parts(
    z: u8,
    x: u32,
    y: u32,
    archives: &str,
) -> Result<Vec<Vec<u8>>, crate::Error> {
    let mut reads = tokio::task::JoinSet::new();
    for (index, set) in archives.split(' ').enumerate() {
        let set = set.to_owned();
        reads.spawn(async move { (index, set_tile(z, x, y, &set).await) });
    }
    let mut found = Vec::new();
    while let Some(read) = reads.join_next().await {
        let (index, bytes) = read.map_err(|error| crate::Error::from(error.to_string()))?;
        if let Some(bytes) = bytes? {
            found.push((index, bytes));
        }
    }
    found.sort_by_key(|(index, _)| *index);
    Ok(found.into_iter().map(|(_, bytes)| bytes).collect())
}

/// Tile `z/x/y` from one set: its parts are disjoint but for tiles on a band's edge,
/// which are the same in both, so the first part holding the tile gives it.
async fn set_tile(z: u8, x: u32, y: u32, set: &str) -> Result<Option<Vec<u8>>, crate::Error> {
    for url in set.split('|') {
        if let Some(bytes) = archive(url).await?.tile(z, x, y).await? {
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
