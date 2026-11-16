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
/// The most a gzip-compressed directory or tile may unpack to: eight times the
/// largest stored size, where real directories and vector tiles pack two to five
/// times smaller.
const MAX_UNPACKED_BYTES: u64 = 32_000_000;
/// The longest range read in one request: a run of neighbouring tiles
/// (`map_data` groups up to 8 MiB) with room to spare.
const MAX_RANGE_BYTES: u64 = 16 * 1024 * 1024;
/// The deepest zoom of a tile id: a level has 4^z tiles, and the ids of all levels
/// up to 31 fit 64 bits. (Published archives stop near zoom 20.)
pub(crate) const MAX_ZOOM: u8 = 31;
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
    /// The archive's suggested view: zoom, longitude, latitude.
    center: (u8, f64, f64),
    /// Whether the tiles are vector tiles (Mapbox Vector Tiles), not images.
    vector_tiles: bool,
}

impl Header {
    /// The zoom levels the archive holds.
    pub(crate) fn zooms(&self) -> (u8, u8) {
        (self.min_zoom, self.max_zoom)
    }
    /// The archive's suggested view: zoom, longitude, latitude.
    pub(crate) fn center(&self) -> (u8, f64, f64) {
        self.center
    }
    /// The root directory's bytes when `first` (the archive's first bytes) holds all of
    /// them; otherwise they have to be read.
    pub(crate) fn root_in<'a>(&self, first: &'a [u8]) -> Result<Option<&'a [u8]>, crate::Error> {
        if self.root_length > MAX_DIRECTORY_BYTES {
            return Err(crate::Error::msg("PMTiles directory too large"));
        }
        let end = self
            .root_offset
            .checked_add(self.root_length)
            .ok_or_else(|| crate::Error::msg("corrupt PMTiles header"))?;
        // Offsets beyond `first` (or beyond what this computer can address) are not
        // in it.
        let (Ok(start), Ok(end)) = (usize::try_from(self.root_offset), usize::try_from(end)) else {
            return Ok(None);
        };
        Ok(first.get(start..end))
    }
    /// Where the root directory is in the archive, for reading it.
    fn root_location(&self) -> (u64, u64) {
        (self.root_offset, self.root_length)
    }
    /// Where the tile of a directory entry (one with a run) is in the archive.
    pub(crate) fn tile_location(&self, entry: Entry) -> Result<TileLocation, crate::Error> {
        if u64::from(entry.length) > MAX_TILE_BYTES {
            return Err(crate::Error::msg("PMTiles tile too large"));
        }
        TileLocation::new(self.tile_offset.checked_add(entry.offset), entry.length)
    }
    /// Where the leaf directory of an entry (one without a run) is in the archive.
    pub(crate) fn leaf_location(&self, entry: Entry) -> Result<TileLocation, crate::Error> {
        if u64::from(entry.length) > MAX_DIRECTORY_BYTES {
            return Err(crate::Error::msg("PMTiles directory too large"));
        }
        TileLocation::new(self.leaf_offset.checked_add(entry.offset), entry.length)
    }
    /// Whether the archive's bounds can hold tile `z/x` (any latitude).
    pub(crate) fn may_hold(&self, z: u8, x: u32) -> bool {
        let Some((west, east)) = self.longitudes else {
            return true;
        };
        // A power of two as a float: any zoom byte is safe here.
        let width = 360.0 / 2f64.powi(i32::from(z));
        let tile_west = -180.0 + f64::from(x) * width;
        // Strictly overlapping: a tile only touching a band's edge belongs next door.
        tile_west < east && tile_west + width > west
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, crate::Error> {
        if bytes.len() < HEADER_BYTES || &bytes[..7] != b"PMTiles" {
            return Err(crate::Error::msg("not a PMTiles archive"));
        }
        if bytes[7] != 3 {
            return Err(crate::Error::Unsupported(format!(
                "PMTiles version {} is not supported",
                bytes[7]
            )));
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
            center: {
                let degrees = |at: usize| {
                    f64::from(i32::from_le_bytes(
                        bytes[at..at + 4].try_into().expect("4 bytes"),
                    )) / 1e7
                };
                (bytes[118], degrees(119), degrees(123))
            },
            vector_tiles: bytes[99] == 1,
        };
        if header.min_zoom > header.max_zoom || header.max_zoom > MAX_ZOOM {
            return Err(crate::Error::msg(format!(
                "corrupt PMTiles header: zoom levels {} to {}",
                header.min_zoom, header.max_zoom
            )));
        }
        if !matches!(
            header.internal_compression,
            Compression::None | Compression::Gzip
        ) {
            return Err(crate::Error::Unsupported(
                "PMTiles directories must be uncompressed or gzip".into(),
            ));
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
/// the curve. `None` for a tile that does not exist: a zoom beyond [`MAX_ZOOM`], or a
/// column or row outside the level.
pub(crate) fn tile_id(z: u8, x: u32, y: u32) -> Option<u64> {
    if z > MAX_ZOOM || u64::from(x.max(y)) >= 1u64 << z {
        return None;
    }
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
    Some(lower_zooms + d)
}

fn varint(bytes: &[u8], at: &mut usize) -> Result<u64, crate::Error> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes
            .get(*at)
            .ok_or_else(|| crate::Error::msg("truncated PMTiles directory"))?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(crate::Error::msg("overlong varint in a PMTiles directory"))
}

/// Decodes an (already decompressed) directory.
pub(crate) fn parse_directory(bytes: &[u8]) -> Result<Vec<Entry>, crate::Error> {
    let mut at = 0;
    let count = usize::try_from(varint(bytes, &mut at)?)
        .map_err(|_| crate::Error::msg("huge directory"))?;
    // Every entry takes at least four bytes: a larger count is corrupt.
    if count > bytes.len() {
        return Err(crate::Error::msg("corrupt PMTiles directory"));
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
        last = last
            .checked_add(varint(bytes, &mut at)?)
            .ok_or_else(|| crate::Error::msg("corrupt PMTiles tile id"))?;
        entry.tile_id = last;
    }
    for entry in &mut entries {
        entry.run_length =
            u32::try_from(varint(bytes, &mut at)?).map_err(|_| crate::Error::msg("huge run"))?;
    }
    for entry in &mut entries {
        entry.length =
            u32::try_from(varint(bytes, &mut at)?).map_err(|_| crate::Error::msg("huge entry"))?;
    }
    for i in 0..count {
        let value = varint(bytes, &mut at)?;
        entries[i].offset = if value == 0 && i > 0 {
            // Directly after the entry before.
            entries[i - 1]
                .offset
                .checked_add(u64::from(entries[i - 1].length))
                .ok_or_else(|| crate::Error::msg("corrupt PMTiles offset"))?
        } else {
            value
                .checked_sub(1)
                .ok_or_else(|| crate::Error::msg("corrupt PMTiles offset"))?
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
    let within_run = id
        .checked_sub(entry.tile_id)
        .is_some_and(|ahead| ahead < u64::from(entry.run_length));
    (entry.run_length == 0 || within_run).then_some(entry)
}

pub(crate) fn decompress(
    bytes: Vec<u8>,
    compression: Compression,
) -> Result<Vec<u8>, crate::Error> {
    match compression {
        Compression::None => Ok(bytes),
        Compression::Gzip => crate::bounded::gunzip(&bytes, MAX_UNPACKED_BYTES),
        other => Err(crate::Error::Unsupported(format!(
            "unsupported PMTiles compression {other:?}"
        ))),
    }
}

/// The archive offset and stored length of one tile. Its end (`offset + length`) is
/// a valid offset too: [`Header::tile_location`] refuses one that is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TileLocation {
    pub(crate) offset: u64,
    pub(crate) length: u32,
}

impl TileLocation {
    fn new(offset: Option<u64>, length: u32) -> Result<Self, crate::Error> {
        offset
            .filter(|offset| offset.checked_add(u64::from(length)).is_some())
            .map(|offset| Self { offset, length })
            .ok_or_else(|| crate::Error::msg("corrupt PMTiles offset"))
    }
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
                center: (0, 0.0, 0.0),
                vector_tiles: false,
            },
            root: Vec::new(),
            leaves: Mutex::new(HashMap::new()),
            resolved: Mutex::new(None),
        };
        let first = archive.range(0, FIRST_READ_BYTES).await?;
        let header = Header::parse(&first)?;
        let root_bytes = match header.root_in(&first)? {
            Some(bytes) => bytes.to_vec(),
            None => {
                let (offset, length) = header.root_location();
                archive.range(offset, length).await?
            }
        };
        archive.root = parse_directory(&decompress(root_bytes, header.internal_compression)?)?;
        archive.header = header;
        Ok(archive)
    }

    /// The zoom levels the archive holds.
    pub(crate) fn zooms(&self) -> (u8, u8) {
        self.header.zooms()
    }

    /// The archive's suggested view: zoom, longitude, latitude.
    pub(crate) fn center(&self) -> (u8, f64, f64) {
        self.header.center()
    }

    /// Whether the archive holds vector tiles, not images.
    pub(crate) fn vector_tiles(&self) -> bool {
        self.header.vector_tiles
    }

    /// Tile `z/x/y`'s bytes, unpacked, or `None` where the archive has no tile.
    pub(crate) async fn tile(
        &self,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<Vec<u8>>, crate::Error> {
        let Some(location) = self.locate(z, x, y).await? else {
            return Ok(None);
        };
        let bytes = self
            .range(location.offset, u64::from(location.length))
            .await?;
        decompress(bytes, self.header.tile_compression).map(Some)
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
        let Some(id) = tile_id(z, x, y) else {
            return Ok(None);
        };
        let mut directory = None;
        for _ in 0..MAX_DIRECTORY_DEPTH {
            let entries = directory.as_deref().unwrap_or(&self.root);
            let Some(entry) = find(entries, id) else {
                return Ok(None);
            };
            if entry.run_length > 0 {
                return self.header.tile_location(entry).map(Some);
            }
            directory = Some(self.leaf(entry).await?);
        }
        Err(crate::Error::msg("PMTiles directories nest too deep"))
    }

    async fn leaf(&self, entry: Entry) -> Result<std::sync::Arc<Vec<Entry>>, crate::Error> {
        let TileLocation { offset, length } = self.header.leaf_location(entry)?;
        if let Some(cached) = self
            .leaves
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&offset)
        {
            cached.used = Instant::now();
            return Ok(cached.entries.clone());
        }
        let bytes = self.range(offset, u64::from(length)).await?;
        let compression = self.header.internal_compression;
        let leaf = std::sync::Arc::new(
            crate::cpu::run(crate::cpu::Service::Features, move || {
                parse_directory(&decompress(bytes, compression)?)
            })
            .await?,
        );
        let mut leaves = self
            .leaves
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        // Offsets and lengths come from the archive's own header and directories.
        let last = offset
            .checked_add(length.max(1) - 1)
            .filter(|_| length <= MAX_RANGE_BYTES)
            .ok_or_else(|| crate::Error::msg("corrupt PMTiles range"))?;
        if !crate::user_data::is_url(&self.url) {
            return file_range(self.url.clone(), offset, length).await;
        }
        let _permit = match crate::http::range_limit(self.policy) {
            Some(limit) => Some(
                limit
                    .acquire()
                    .await
                    .map_err(|_| crate::Error::msg("HTTP range limiter closed"))?,
            ),
            None => None,
        };
        let client = crate::http::client(self.policy)?;
        let reused = self
            .resolved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .filter(|(_, at)| at.elapsed() < REDIRECT_REUSE)
            .map(|(url, _)| url);
        for url in reused.into_iter().chain(std::iter::once(self.url.clone())) {
            let response = client
                .get(&url)
                .header(reqwest::header::RANGE, format!("bytes={offset}-{last}"))
                .send()
                .await?;
            REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let status = response.status();
            // An expired signed link: ask the archive's own address again.
            if url != self.url && !status.is_success() {
                *self
                    .resolved
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                continue;
            }
            let response = response.error_for_status()?;
            if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                return Err(crate::Error::msg(format!(
                    "{} does not serve byte ranges",
                    self.url
                )));
            }
            if url == self.url && response.url().as_str() != self.url {
                *self
                    .resolved
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some((response.url().to_string(), Instant::now()));
            }
            let mut response = response;
            let mut bytes = Vec::with_capacity(length.min(MAX_TILE_BYTES) as usize);
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
        Err(crate::Error::msg(format!("{} could not be read", self.url)))
    }
}

/// Bytes `offset..offset + length` of a file on this computer (fewer at its end).
async fn file_range(path: String, offset: u64, length: u64) -> Result<Vec<u8>, crate::Error> {
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>, crate::Error> {
        use std::io::{Read, Seek};
        let mut file = std::fs::File::open(&path)?;
        file.seek(std::io::SeekFrom::Start(offset))?;
        let mut bytes = Vec::with_capacity(length.min(MAX_TILE_BYTES) as usize);
        file.take(length).read_to_end(&mut bytes)?;
        Ok(bytes)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id of a tile that exists.
    fn id(z: u8, x: u32, y: u32) -> u64 {
        tile_id(z, x, y).expect("a tile that exists")
    }

    /// Found by the `pmtiles` fuzz target: every case here used to overflow (a panic
    /// in a debug build, a wrong answer in a release build).
    #[test]
    fn tiles_and_offsets_that_cannot_exist_are_refused() {
        // Zoom 32 and deeper have no 64-bit id; a column or row outside its level
        // has none either.
        assert_eq!(tile_id(32, 0, 0), None);
        assert_eq!(tile_id(255, u32::MAX, u32::MAX), None);
        assert_eq!(tile_id(1, 2, 0), None);
        assert_eq!(tile_id(0, 0, 1), None);
        // The deepest level's last tile still has one.
        assert!(tile_id(MAX_ZOOM, u32::MAX >> 1, u32::MAX >> 1).is_some());

        // Tile ids are written as differences: their sum must fit.
        let mut directory = Vec::new();
        put_varint(&mut directory, 2);
        put_varint(&mut directory, u64::MAX);
        put_varint(&mut directory, 1);
        for _ in 0..6 {
            put_varint(&mut directory, 1);
        }
        assert!(parse_directory(&directory).is_err());
        // An offset "directly after the entry before" must fit as well.
        let mut directory = Vec::new();
        put_varint(&mut directory, 2);
        for value in [0, 1, 1, 1, u64::from(u32::MAX), 1, u64::MAX, 0] {
            put_varint(&mut directory, value);
        }
        assert!(parse_directory(&directory).is_err());

        // A run at the very end of the id space.
        let last = Entry {
            tile_id: u64::MAX - 1,
            offset: u64::MAX - 10,
            length: 100,
            run_length: 5,
        };
        assert_eq!(find(&[last], u64::MAX), Some(last));
        assert_eq!(find(&[last], u64::MAX - 2), None);

        // Offsets are added to the header's section starts.
        let mut bytes = vec![0u8; HEADER_BYTES];
        bytes[..7].copy_from_slice(b"PMTiles");
        bytes[7] = 3;
        bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        bytes[16..24].copy_from_slice(&40u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&100u64.to_le_bytes());
        bytes[56..64].copy_from_slice(&100u64.to_le_bytes());
        bytes[97] = 1;
        let header = Header::parse(&bytes).unwrap();
        assert!(header.root_in(&bytes).is_err());
        assert!(header.tile_location(last).is_err());
        assert!(header.leaf_location(last).is_err());
        // A location whose end would pass the last offset is refused too.
        let mut near = last;
        near.offset = u64::MAX - 150;
        assert!(header.tile_location(near).is_err());
        near.offset = u64::MAX - 250;
        assert!(header.tile_location(near).is_ok());
        // A root directory beyond the first read is read on its own; one larger than
        // any directory is refused before it is asked for.
        bytes[8..16].copy_from_slice(&1_000_000u64.to_le_bytes());
        assert_eq!(
            Header::parse(&bytes).unwrap().root_in(&bytes).unwrap(),
            None
        );
        bytes[16..24].copy_from_slice(&(MAX_DIRECTORY_BYTES + 1).to_le_bytes());
        assert!(Header::parse(&bytes).unwrap().root_in(&bytes).is_err());
        bytes[16..24].copy_from_slice(&40u64.to_le_bytes());

        // Zoom levels: any byte is a safe question for a band, but a header whose
        // levels are reversed or beyond the id space is corrupt.
        bytes[102..106].copy_from_slice(&(-1_800_000_000i32).to_le_bytes());
        let west = Header::parse(&bytes).unwrap();
        assert!(!west.may_hold(255, u32::MAX) && !west.may_hold(32, u32::MAX));
        assert!(west.may_hold(31, 0));
        bytes[100] = 3;
        bytes[101] = 2;
        assert!(Header::parse(&bytes).is_err());
        bytes[100] = 0;
        bytes[101] = MAX_ZOOM + 1;
        assert!(Header::parse(&bytes).is_err());
        bytes[101] = MAX_ZOOM;
        assert!(Header::parse(&bytes).is_ok());
    }

    #[test]
    fn compressed_directories_and_tiles_unpack_to_a_limit() {
        let packed = gzip(&vec![0u8; MAX_UNPACKED_BYTES as usize + 1]);
        assert!(packed.len() < 100_000);
        assert!(decompress(packed, Compression::Gzip).is_err());
        assert_eq!(
            decompress(gzip(b"tile"), Compression::Gzip).unwrap(),
            b"tile"
        );
    }

    #[test]
    fn tile_ids_follow_the_specification() {
        // The specification's examples.
        assert_eq!(id(0, 0, 0), 0);
        assert_eq!(id(1, 0, 0), 1);
        assert_eq!(id(1, 0, 1), 2);
        assert_eq!(id(1, 1, 1), 3);
        assert_eq!(id(1, 1, 0), 4);
        assert_eq!(id(2, 0, 0), 5);
        assert_eq!(id(12, 3423, 1763), 19_078_479);
        // Against the reference implementation (`pmtiles` 3.4.1 for Python, seed 7).
        for line in include_str!("pmtiles/tile_ids.txt").lines() {
            let v: Vec<u64> = line.split(' ').map(|n| n.parse().unwrap()).collect();
            assert_eq!(id(v[0] as u8, v[1] as u32, v[2] as u32), v[3], "{line}");
        }
        // Every tile of a zoom gets its own id, in that zoom's block.
        let mut ids: Vec<u64> = (0..8)
            .flat_map(|x| (0..8).map(move |y| id(3, x, y)))
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
        // Zoom levels the wrong way round, or beyond any pyramid.
        bytes[97] = 2;
        bytes[100] = 13;
        assert!(Header::parse(&bytes).is_err());
        bytes[100] = 0;
        bytes[101] = 200;
        assert!(Header::parse(&bytes).is_err());
        bytes[101] = 12;
        assert!(!Header::parse(&bytes).unwrap().vector_tiles);
        bytes[99] = 1;
        assert!(Header::parse(&bytes).unwrap().vector_tiles);
    }

    #[test]
    fn damaged_archives_are_errors_not_crashes() {
        // A gzip stream that unpacks to more than any directory or tile.
        let bomb = gzip(&vec![0u8; MAX_UNPACKED_BYTES as usize + 1]);
        assert!(bomb.len() < 200_000);
        assert!(decompress(bomb, Compression::Gzip).is_err());
        // Tile ids and offsets that run past 64 bits.
        let mut directory = Vec::new();
        put_varint(&mut directory, 2);
        put_varint(&mut directory, u64::MAX);
        put_varint(&mut directory, 5);
        assert!(parse_directory(&directory).is_err());
        // A root directory said to be a thousand million million bytes long is not
        // asked for, nor is room set aside for it.
        let mut bytes = archive_bytes();
        bytes[16..24].copy_from_slice(&(1u64 << 50).to_le_bytes());
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let url = serve(bytes, true);
        let opened = runtime
            .handle()
            .block_on(Archive::open(&url, crate::http::Policy::Boundary));
        assert!(opened.is_err_and(|error| error.to_string().contains("too large")));
    }

    #[test]
    fn an_archive_file_on_this_computer_is_read_like_one_on_the_web() {
        let path =
            std::env::temp_dir().join(format!("globecraft-archive-{}.pmtiles", std::process::id()));
        std::fs::write(&path, archive_bytes()).unwrap();
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        runtime.handle().block_on(async {
            let archive = Archive::open(&path.to_string_lossy(), crate::http::Policy::UserData)
                .await
                .unwrap();
            assert_eq!(archive.zooms(), (0, 2));
            let tile = |z, x, y| archive.tile(z, x, y);
            assert_eq!(tile(0, 0, 0).await.unwrap().as_deref(), Some(&b"world"[..]));
            // Behind a leaf directory.
            assert_eq!(
                tile(2, 0, 0).await.unwrap().as_deref(),
                Some(&b"sixteenth"[..])
            );
            assert_eq!(tile(1, 0, 0).await.unwrap(), None);
        });
        let _ = std::fs::remove_file(&path);
        let missing = runtime.handle().block_on(Archive::open(
            "/nowhere/globecraft-missing.pmtiles",
            crate::http::Policy::UserData,
        ));
        assert!(matches!(missing, Err(crate::Error::Io(_))));
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
            tile_id: id(2, 0, 0),
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
                tile_id: id(1, 1, 0),
                offset: 5,
                length: 7,
                run_length: 1,
            },
            Entry {
                tile_id: id(2, 0, 0),
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

    #[test]
    fn damaged_headers_and_directories_are_refused_without_panicking() {
        let directory = encode(&[
            Entry {
                tile_id: 0,
                offset: 0,
                length: 100,
                run_length: 1,
            },
            Entry {
                tile_id: 5,
                offset: 100,
                length: 20,
                run_length: 3,
            },
            Entry {
                tile_id: 40,
                offset: 0,
                length: 10,
                run_length: 0,
            },
        ]);
        let archive = archive_bytes();
        let mut inputs = crate::test_inputs::Inputs::new(13);
        for _ in 0..50_000 {
            let _ = parse_directory(&inputs.mutate(&directory));
            let _ = Header::parse(&inputs.mutate(&archive[..HEADER_BYTES]));
        }
    }
}
