//! Reading single tiles from a PMTiles (version 3) archive over HTTP range requests:
//! one file holds a whole tile pyramid, and a client fetches only the header, the
//! directories it needs and the tiles themselves. See
//! <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The header is 127 bytes; the root directory must lie within the first 16 KiB, so
/// one request reads both.
const HEADER_BYTES: usize = 127;
const FIRST_READ_BYTES: u64 = 16_384;
/// The largest directory read (a leaf directory is typically a few KiB to 100 KiB).
const MAX_DIRECTORY_BYTES: u64 = 4_000_000;
/// The largest tile read.
const MAX_TILE_BYTES: u64 = 4_000_000;
/// Leaf directories kept in memory.
const MAX_CACHED_LEAVES: usize = 64;
/// How long a redirect target (a signed download link) is reused before the
/// archive's own address is asked again. GitHub's signed release links expire after
/// 5 minutes (then answer 618 `jwt:expired`).
const REDIRECT_REUSE: Duration = Duration::from_secs(4 * 60);

/// Range requests made and bytes received by all archives (headers, directories and
/// tiles), for measurements.
static REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RECEIVED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Requests and bytes over the wire since the start, see [`REQUESTS`].
#[cfg(test)]
pub(crate) fn transferred() -> (u64, u64) {
    use std::sync::atomic::Ordering::Relaxed;
    (REQUESTS.load(Relaxed), RECEIVED.load(Relaxed))
}
/// Leaf directories nest at most this deep in a valid archive.
const MAX_DIRECTORY_DEPTH: usize = 4;

/// Compression of directories or tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Compression {
    Unknown,
    None,
    Gzip,
    Brotli,
    Zstd,
}

impl Compression {
    fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Self::None,
            2 => Self::Gzip,
            3 => Self::Brotli,
            4 => Self::Zstd,
            _ => Self::Unknown,
        }
    }
}

/// The parts of the header a reader needs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Header {
    root_offset: u64,
    root_length: u64,
    leaf_offset: u64,
    tile_offset: u64,
    internal_compression: Compression,
    tile_compression: Compression,
    min_zoom: u8,
    max_zoom: u8,
    /// West and east edges in degrees: GlobeCraft's border archives are split into
    /// longitude bands to stay under GitHub's 2 GiB asset limit, and a tile outside
    /// a band is not looked up there. `None` when the header leaves them unset.
    longitudes: Option<(f64, f64)>,
}

impl Header {
    /// Whether the archive's bounds can hold tile `z/x` (any latitude).
    fn may_hold(&self, z: u8, x: u32) -> bool {
        let Some((west, east)) = self.longitudes else {
            return true;
        };
        let width = 360.0 / f64::from(1u32 << z);
        let tile_west = -180.0 + f64::from(x) * width;
        // Strictly overlapping: a tile only touching a band's edge belongs next door.
        tile_west < east && tile_west + width > west
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, crate::Error> {
        if bytes.len() < HEADER_BYTES || &bytes[..7] != b"PMTiles" {
            return Err("not a PMTiles archive".into());
        }
        if bytes[7] != 3 {
            return Err(format!("PMTiles version {} is not supported", bytes[7]).into());
        }
        let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
        let header = Self {
            root_offset: u64_at(8),
            root_length: u64_at(16),
            leaf_offset: u64_at(40),
            tile_offset: u64_at(56),
            internal_compression: Compression::from_byte(bytes[97]),
            tile_compression: Compression::from_byte(bytes[98]),
            min_zoom: bytes[100],
            max_zoom: bytes[101],
            longitudes: {
                let degrees = |at: usize| {
                    f64::from(i32::from_le_bytes(
                        bytes[at..at + 4].try_into().expect("4 bytes"),
                    )) / 1e7
                };
                let (west, east) = (degrees(102), degrees(110));
                (west < east).then_some((west, east))
            },
        };
        if !matches!(
            header.internal_compression,
            Compression::None | Compression::Gzip
        ) {
            return Err("PMTiles directories must be uncompressed or gzip".into());
        }
        Ok(header)
    }
}

/// One directory entry: a run of tiles (`run_length` ≥ 1) or a leaf directory
/// (`run_length` 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    tile_id: u64,
    offset: u64,
    length: u32,
    run_length: u32,
}

/// The Hilbert-curve tile id of `z/x/y`: tiles of lower zooms first, each zoom along
/// the curve.
pub(crate) fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let lower_zooms = ((1u64 << (2 * u32::from(z))) - 1) / 3;
    let (mut x, mut y) = (u64::from(x), u64::from(y));
    let mut d = 0u64;
    let mut s = (1u64 << z) / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = s - 1 - (x & (s - 1));
                y = s - 1 - (y & (s - 1));
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    lower_zooms + d
}

fn varint(bytes: &[u8], at: &mut usize) -> Result<u64, crate::Error> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at).ok_or("truncated PMTiles directory")?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("overlong varint in a PMTiles directory".into())
}

/// Decodes an (already decompressed) directory.
pub(crate) fn parse_directory(bytes: &[u8]) -> Result<Vec<Entry>, crate::Error> {
    let mut at = 0;
    let count = usize::try_from(varint(bytes, &mut at)?).map_err(|_| "huge directory")?;
    // Every entry takes at least four bytes: a larger count is corrupt.
    if count > bytes.len() {
        return Err("corrupt PMTiles directory".into());
    }
    let mut entries = vec![
        Entry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run_length: 0,
        };
        count
    ];
    let mut last = 0u64;
    for entry in &mut entries {
        last += varint(bytes, &mut at)?;
        entry.tile_id = last;
    }
    for entry in &mut entries {
        entry.run_length = u32::try_from(varint(bytes, &mut at)?).map_err(|_| "huge run")?;
    }
    for entry in &mut entries {
        entry.length = u32::try_from(varint(bytes, &mut at)?).map_err(|_| "huge entry")?;
    }
    for i in 0..count {
        let value = varint(bytes, &mut at)?;
        entries[i].offset = if value == 0 && i > 0 {
            entries[i - 1].offset + u64::from(entries[i - 1].length)
        } else {
            value.checked_sub(1).ok_or("corrupt PMTiles offset")?
        };
    }
    Ok(entries)
}

/// The entry covering `id`: the last entry at or before it, if its run (or leaf)
/// can hold it.
pub(crate) fn find(entries: &[Entry], id: u64) -> Option<Entry> {
    let index = entries
        .partition_point(|entry| entry.tile_id <= id)
        .checked_sub(1)?;
    let entry = entries[index];
    (entry.run_length == 0 || id < entry.tile_id + u64::from(entry.run_length)).then_some(entry)
}

fn decompress(bytes: Vec<u8>, compression: Compression) -> Result<Vec<u8>, crate::Error> {
    match compression {
        Compression::None => Ok(bytes),
        Compression::Gzip => {
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut out)?;
            Ok(out)
        }
        other => Err(format!("unsupported PMTiles compression {other:?}").into()),
    }
}

/// The archive offset and stored length of one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TileLocation {
    pub(crate) offset: u64,
    pub(crate) length: u32,
}

struct CachedLeaf {
    entries: std::sync::Arc<Vec<Entry>>,
    used: Instant,
}

/// A remote archive: its header and root directory, read once, and recently used
/// leaf directories.
pub(crate) struct Archive {
    url: String,
    policy: crate::http::Policy,
    header: Header,
    root: Vec<Entry>,
    /// Recently used leaf directories, by their offset in the archive.
    leaves: Mutex<HashMap<u64, CachedLeaf>>,
    /// Where the archive's address last redirected to (a hosted file's signed link).
    resolved: Mutex<Option<(String, Instant)>>,
}

impl Archive {
    /// Reads the header and root directory of the archive at `url`.
    pub(crate) async fn open(url: &str, policy: crate::http::Policy) -> Result<Self, crate::Error> {
        let mut archive = Self {
            url: url.to_owned(),
            policy,
            header: Header {
                root_offset: 0,
                root_length: 0,
                leaf_offset: 0,
                tile_offset: 0,
                internal_compression: Compression::None,
                tile_compression: Compression::None,
                min_zoom: 0,
                max_zoom: 0,
                longitudes: None,
            },
            root: Vec::new(),
            leaves: Mutex::new(HashMap::new()),
            resolved: Mutex::new(None),
        };
        let first = archive.range(0, FIRST_READ_BYTES).await?;
        let header = Header::parse(&first)?;
        let start = usize::try_from(header.root_offset).map_err(|_| "bad root offset")?;
        let end = start + usize::try_from(header.root_length).map_err(|_| "bad root length")?;
        let root_bytes = match first.get(start..end) {
            Some(bytes) => bytes.to_vec(),
            None => {
                archive
                    .range(header.root_offset, header.root_length)
                    .await?
            }
        };
        archive.root = parse_directory(&decompress(root_bytes, header.internal_compression)?)?;
        archive.header = header;
        Ok(archive)
    }

    /// Where the tile's bytes are, without reading them. A batch of tiles can group
    /// these ranges into fewer requests.
    pub(crate) async fn locate(
        &self,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<TileLocation>, crate::Error> {
        if z < self.header.min_zoom || z > self.header.max_zoom || !self.header.may_hold(z, x) {
            return Ok(None);
        }
        let id = tile_id(z, x, y);
        let mut directory = None;
        for _ in 0..MAX_DIRECTORY_DEPTH {
            let entries = directory.as_deref().unwrap_or(&self.root);
            let Some(entry) = find(entries, id) else {
                return Ok(None);
            };
            if entry.run_length > 0 {
                if u64::from(entry.length) > MAX_TILE_BYTES {
                    return Err("PMTiles tile too large".into());
                }
                return Ok(Some(TileLocation {
                    offset: self.header.tile_offset + entry.offset,
                    length: entry.length,
                }));
            }
            directory = Some(self.leaf(entry).await?);
        }
        Err("PMTiles directories nest too deep".into())
    }

    async fn leaf(&self, entry: Entry) -> Result<std::sync::Arc<Vec<Entry>>, crate::Error> {
        let offset = self.header.leaf_offset + entry.offset;
        if let Some(cached) = self.leaves.lock().expect("leaf cache").get_mut(&offset) {
            cached.used = Instant::now();
            return Ok(cached.entries.clone());
        }
        if u64::from(entry.length) > MAX_DIRECTORY_BYTES {
            return Err("PMTiles directory too large".into());
        }
        let bytes = self.range(offset, u64::from(entry.length)).await?;
        let compression = self.header.internal_compression;
        let leaf = std::sync::Arc::new(
            crate::cpu::run(crate::cpu::Service::Features, move || {
                parse_directory(&decompress(bytes, compression)?)
            })
            .await?,
        );
        let mut leaves = self.leaves.lock().expect("leaf cache");
        if leaves.len() >= MAX_CACHED_LEAVES
            && let Some(oldest) = leaves
                .iter()
                .min_by_key(|(_, cached)| cached.used)
                .map(|(key, _)| *key)
        {
            leaves.remove(&oldest);
        }
        leaves.insert(
            offset,
            CachedLeaf {
                entries: leaf.clone(),
                used: Instant::now(),
            },
        );
        Ok(leaf)
    }

    /// Bytes `offset..offset + length` of the archive. A server that ignores the range
    /// is an error rather than a download of the whole file.
    pub(crate) async fn range(&self, offset: u64, length: u64) -> Result<Vec<u8>, crate::Error> {
        let _permit = match crate::http::range_limit(self.policy) {
            Some(limit) => Some(
                limit
                    .acquire()
                    .await
                    .map_err(|_| crate::Error::from("HTTP range limiter closed"))?,
            ),
            None => None,
        };
        let client = crate::http::client(self.policy)?;
        let reused = self
            .resolved
            .lock()
            .expect("redirect cache")
            .clone()
            .filter(|(_, at)| at.elapsed() < REDIRECT_REUSE)
            .map(|(url, _)| url);
        for url in reused.into_iter().chain(std::iter::once(self.url.clone())) {
            let response = client
                .get(&url)
                .header(
                    reqwest::header::RANGE,
                    format!("bytes={offset}-{}", offset + length.max(1) - 1),
                )
                .send()
                .await?;
            REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let status = response.status();
            // An expired signed link: ask the archive's own address again.
            if url != self.url && !status.is_success() {
                *self.resolved.lock().expect("redirect cache") = None;
                continue;
            }
            let response = response.error_for_status()?;
            if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                return Err(format!("{} does not serve byte ranges", self.url).into());
            }
            if url == self.url && response.url().as_str() != self.url {
                *self.resolved.lock().expect("redirect cache") =
                    Some((response.url().to_string(), Instant::now()));
            }
            let mut response = response;
            let mut bytes = Vec::with_capacity(length as usize);
            while let Some(chunk) = response.chunk().await? {
                bytes.extend_from_slice(&chunk);
                if bytes.len() as u64 > length {
                    bytes.truncate(length as usize);
                    break;
                }
            }
            RECEIVED.fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed);
            // Directories count too: the session's border download total is what
            // crossed the network.
            if self.policy == crate::http::Policy::Boundary {
                crate::download_stats::record(
                    crate::download_stats::DownloadKind::BorderTiles,
                    bytes.len(),
                );
            }
            return Ok(bytes);
        }
        Err(format!("{} could not be read", self.url).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_follow_the_specification() {
        // The specification's examples.
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        assert_eq!(tile_id(12, 3423, 1763), 19_078_479);
        // Against the reference implementation (`pmtiles` 3.4.1 for Python, seed 7).
        for line in include_str!("pmtiles/tile_ids.txt").lines() {
            let v: Vec<u64> = line.split(' ').map(|n| n.parse().unwrap()).collect();
            assert_eq!(
                tile_id(v[0] as u8, v[1] as u32, v[2] as u32),
                v[3],
                "{line}"
            );
        }
        // Every tile of a zoom gets its own id, in that zoom's block.
        let mut ids: Vec<u64> = (0..8)
            .flat_map(|x| (0..8).map(move |y| tile_id(3, x, y)))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids, (21..85).collect::<Vec<_>>());
    }

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn encode(entries: &[Entry]) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, entries.len() as u64);
        let mut last = 0;
        for entry in entries {
            put_varint(&mut out, entry.tile_id - last);
            last = entry.tile_id;
        }
        for entry in entries {
            put_varint(&mut out, u64::from(entry.run_length));
        }
        for entry in entries {
            put_varint(&mut out, u64::from(entry.length));
        }
        for (i, entry) in entries.iter().enumerate() {
            let contiguous =
                i > 0 && entry.offset == entries[i - 1].offset + u64::from(entries[i - 1].length);
            put_varint(&mut out, if contiguous { 0 } else { entry.offset + 1 });
        }
        out
    }

    #[test]
    fn directories_round_trip_and_find_runs_and_leaves() {
        let entries = vec![
            Entry {
                tile_id: 0,
                offset: 0,
                length: 100,
                run_length: 1,
            },
            // Contiguous with the previous entry, so its offset is written as 0.
            Entry {
                tile_id: 5,
                offset: 100,
                length: 30,
                run_length: 3,
            },
            Entry {
                tile_id: 40,
                offset: 7,
                length: 500,
                run_length: 0,
            },
        ];
        let parsed = parse_directory(&encode(&entries)).unwrap();
        assert_eq!(parsed, entries);
        assert_eq!(find(&parsed, 0), Some(entries[0]));
        assert_eq!(find(&parsed, 3), None);
        // A run covers ids 5, 6 and 7 with the same bytes.
        assert_eq!(find(&parsed, 7), Some(entries[1]));
        assert_eq!(find(&parsed, 8), None);
        // Past a leaf entry: the leaf directory holds the rest.
        assert_eq!(find(&parsed, 1_000), Some(entries[2]));
        assert!(parse_directory(&[200, 1]).is_err());
    }

    #[test]
    fn headers_are_checked() {
        let mut bytes = vec![0u8; HEADER_BYTES];
        bytes[..7].copy_from_slice(b"PMTiles");
        bytes[7] = 3;
        bytes[8..16].copy_from_slice(&127u64.to_le_bytes());
        bytes[16..24].copy_from_slice(&40u64.to_le_bytes());
        bytes[97] = 2;
        bytes[98] = 2;
        bytes[101] = 12;
        let header = Header::parse(&bytes).unwrap();
        assert_eq!(header.root_offset, 127);
        assert_eq!(header.tile_compression, Compression::Gzip);
        assert_eq!((header.min_zoom, header.max_zoom), (0, 12));
        // Unset bounds hold everything; a band holds the tiles overlapping it.
        assert!(header.may_hold(12, 0) && header.may_hold(12, 4095));
        bytes[102..106].copy_from_slice(&(-1_800_000_000i32).to_le_bytes());
        bytes[110..114].copy_from_slice(&0i32.to_le_bytes());
        let west = Header::parse(&bytes).unwrap();
        assert!(west.may_hold(12, 0) && west.may_hold(12, 2047));
        assert!(!west.may_hold(12, 2048) && !west.may_hold(1, 1));
        assert!(west.may_hold(0, 0));
        bytes[97] = 3;
        assert!(Header::parse(&bytes).is_err());
        assert!(Header::parse(b"MBTiles").is_err());
    }

    /// Serves `bytes` over HTTP on a local port, answering byte ranges with 206 (or
    /// ignoring them when `ranges` is false).
    fn serve(bytes: Vec<u8>, ranges: bool) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { return };
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") && socket.read_exact(&mut byte).is_ok() {
                    request.push(byte[0]);
                }
                let request = String::from_utf8_lossy(&request).to_lowercase();
                let range = request.lines().find_map(|line| {
                    let spec = line.strip_prefix("range: bytes=")?;
                    let (start, end) = spec.split_once('-')?;
                    Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                });
                let reply = match range {
                    Some((start, end)) if ranges => {
                        let end = end.min(bytes.len() - 1);
                        let mut head = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                            end + 1 - start,
                            bytes.len()
                        )
                        .into_bytes();
                        head.extend_from_slice(&bytes[start..=end]);
                        head
                    }
                    _ => {
                        let mut head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        )
                        .into_bytes();
                        head.extend_from_slice(&bytes);
                        head
                    }
                };
                let _ = socket.write_all(&reply);
            }
        });
        format!("http://{address}/archive.pmtiles")
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, bytes).unwrap();
        encoder.finish().unwrap()
    }

    /// An archive with tiles 0/0/0 and 1/1/0 in the root directory and 2/0/0 behind a
    /// leaf directory; gzip-compressed directories.
    fn archive_bytes() -> Vec<u8> {
        let tiles: [&[u8]; 3] = [b"world", b"quarter", b"sixteenth"];
        let leaf = gzip(&encode(&[Entry {
            tile_id: tile_id(2, 0, 0),
            offset: 12,
            length: 9,
            run_length: 1,
        }]));
        let root = gzip(&encode(&[
            Entry {
                tile_id: 0,
                offset: 0,
                length: 5,
                run_length: 1,
            },
            Entry {
                tile_id: tile_id(1, 1, 0),
                offset: 5,
                length: 7,
                run_length: 1,
            },
            Entry {
                tile_id: tile_id(2, 0, 0),
                offset: 0,
                length: leaf.len() as u32,
                run_length: 0,
            },
        ]));
        let root_offset = HEADER_BYTES as u64;
        let leaf_offset = root_offset + root.len() as u64;
        let tile_offset = leaf_offset + leaf.len() as u64;
        let mut out = vec![0u8; HEADER_BYTES];
        out[..7].copy_from_slice(b"PMTiles");
        out[7] = 3;
        for (at, value) in [
            (8, root_offset),
            (16, root.len() as u64),
            (40, leaf_offset),
            (48, leaf.len() as u64),
            (56, tile_offset),
            (64, 21),
        ] {
            out[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        out[97] = 2;
        out[98] = 1;
        out[99] = 1;
        out[101] = 2;
        out.extend_from_slice(&root);
        out.extend_from_slice(&leaf);
        for tile in tiles {
            out.extend_from_slice(tile);
        }
        out
    }

    #[test]
    fn tiles_are_read_by_range_through_the_root_and_leaf_directories() {
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let url = serve(archive_bytes(), true);
        runtime.handle().block_on(async {
            let archive = Archive::open(&url, crate::http::Policy::Boundary)
                .await
                .unwrap();
            for (z, x, y, expected) in [
                (0, 0, 0, &b"world"[..]),
                (1, 1, 0, &b"quarter"[..]),
                (2, 0, 0, &b"sixteenth"[..]),
            ] {
                let location = archive.locate(z, x, y).await.unwrap().unwrap();
                let bytes = archive
                    .range(location.offset, u64::from(location.length))
                    .await
                    .unwrap();
                assert_eq!(bytes, expected);
            }
            // Missing tiles and zooms outside the archive.
            assert_eq!(archive.locate(1, 0, 0).await.unwrap(), None);
            assert_eq!(archive.locate(3, 0, 0).await.unwrap(), None);
        });
    }

    #[test]
    fn a_server_without_ranges_is_refused() {
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let url = serve(archive_bytes(), false);
        let opened = runtime
            .handle()
            .block_on(Archive::open(&url, crate::http::Policy::Boundary));
        let Err(error) = opened else {
            panic!("a server ignoring ranges must not open")
        };
        assert!(error.to_string().contains("byte ranges"), "{error}");
    }
}
