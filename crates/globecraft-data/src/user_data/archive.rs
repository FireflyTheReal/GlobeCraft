//! Tile archives as a user's map layer (roadmap 1.2 and 2.1): a PMTiles archive on the
//! web, read by ranges, or an MBTiles file on this computer. A layer's template names
//! one as `pmtiles:<address>` or `mbtiles:<path>`; images in either are drawn like a
//! web map's tiles.
use crate::boundaries::osm_tiles::content::TileKey;
use crate::pmtiles::{Archive, MAX_ZOOM};
use rusqlite::OptionalExtension;
use std::path::PathBuf;
use std::sync::Arc;

pub const PMTILES_PREFIX: &str = "pmtiles:";
pub const MBTILES_PREFIX: &str = "mbtiles:";

/// Where an archive layer's tiles come from.
#[derive(Clone)]
pub(super) enum TileArchive {
    Pmtiles {
        url: String,
        /// Opened on first use and kept: the header and root directory.
        opened: Arc<tokio::sync::OnceCell<Archive>>,
    },
    Mbtiles(PathBuf),
}

impl TileArchive {
    /// Whether a template names an archive file on this computer, which means
    /// nothing on another one.
    pub(super) fn is_local(template: &str) -> bool {
        match template.strip_prefix(PMTILES_PREFIX) {
            Some(address) => !super::is_url(address),
            None => template.starts_with(MBTILES_PREFIX),
        }
    }

    /// The archive a template names, if it names one.
    pub(super) fn from_template(template: &str) -> Option<Self> {
        if let Some(url) = template.strip_prefix(PMTILES_PREFIX) {
            return Some(Self::Pmtiles {
                url: url.trim().to_owned(),
                opened: Arc::default(),
            });
        }
        template
            .strip_prefix(MBTILES_PREFIX)
            .map(|path| Self::Mbtiles(PathBuf::from(path.trim())))
    }

    async fn archive(&self) -> Result<Option<&Archive>, crate::Error> {
        match self {
            Self::Pmtiles { url, opened } => Ok(Some(
                opened
                    .get_or_try_init(|| Archive::open(url, crate::http::Policy::UserData))
                    .await?,
            )),
            Self::Mbtiles(_) => Ok(None),
        }
    }

    /// Tile `key`'s image bytes, or `None` where the archive has none.
    pub(super) async fn tile(&self, key: TileKey) -> Result<Option<Vec<u8>>, crate::Error> {
        if let Some(archive) = self.archive().await? {
            return archive.tile(key.z, key.x, key.y).await;
        }
        let Self::Mbtiles(path) = self else {
            unreachable!("PMTiles answered above")
        };
        let path = path.clone();
        tokio::task::spawn_blocking(move || mbtiles_tile(&open_mbtiles(&path)?, key))
            .await
            .map_err(|error| crate::Error::msg(error.to_string()))?
    }

    /// The zoom levels held and a tile that exists, for the Add Data form.
    pub(super) async fn describe(&self) -> Result<ArchiveInfo, crate::Error> {
        if let Some(archive) = self.archive().await? {
            if archive.vector_tiles() {
                return Err(crate::Error::msg(
                    "this archive holds vector tiles; an image layer draws only image tiles \
                     (PNG, JPEG or WebP)",
                ));
            }
            return pmtiles_info(archive.zooms(), archive.center());
        }
        let Self::Mbtiles(path) = self else {
            unreachable!("PMTiles answered above")
        };
        let path = path.clone();
        tokio::task::spawn_blocking(move || mbtiles_info(&open_mbtiles(&path)?))
            .await
            .map_err(|error| crate::Error::msg(error.to_string()))?
    }
}

/// What a PMTiles archive holds, from its header's zoom range and suggested view
/// (zoom, longitude, latitude).
pub(crate) fn pmtiles_info(
    (min_zoom, max_zoom): (u8, u8),
    (zoom, lon, lat): (u8, f64, f64),
) -> Result<ArchiveInfo, crate::Error> {
    // `Header::parse` refuses such a header; checked here too, since `clamp` panics
    // on a reversed range and a tile's column needs a zoom of at most `MAX_ZOOM`.
    if min_zoom > max_zoom || max_zoom > MAX_ZOOM {
        return Err(crate::Error::msg("corrupt PMTiles header: zoom levels"));
    }
    let zoom = zoom.clamp(min_zoom, max_zoom);
    Ok(ArchiveInfo {
        min_zoom,
        max_zoom,
        sample: TileKey::at(zoom, lat, lon),
    })
}

/// MBTiles rows count from the south (TMS), map tiles from the north: the row on the
/// other scheme. `None` for a row the zoom level does not have, or a zoom no tile has.
fn flipped_row(zoom: u8, row: u32) -> Option<u32> {
    if zoom > MAX_ZOOM {
        return None;
    }
    // At most 2^31 rows, so this shift and subtraction fit.
    let last_row = (1u32 << zoom) - 1;
    last_row.checked_sub(row)
}

/// Tile `key`'s image bytes in an open MBTiles database, or `None` where it has none.
pub(crate) fn mbtiles_tile(
    db: &rusqlite::Connection,
    key: TileKey,
) -> Result<Option<Vec<u8>>, crate::Error> {
    let Some(row) = flipped_row(key.z, key.y) else {
        return Ok(None);
    };
    Ok(db
        .query_row(
            "SELECT tile_data FROM tiles WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            rusqlite::params![key.z, key.x, row],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .optional()?)
}

/// The zoom levels an open MBTiles database holds and a tile that exists.
pub(crate) fn mbtiles_info(db: &rusqlite::Connection) -> Result<ArchiveInfo, crate::Error> {
    // The file is whatever its author wrote: rows of zoom levels no map has, columns
    // and rows outside their level, or no numbers at all. Only tiles that can exist
    // count. (Both questions are answered from the usual index on zoom, column, row.)
    let empty = || crate::Error::msg("the MBTiles file holds no tiles");
    let (min_zoom, max_zoom): (Option<u8>, Option<u8>) = db.query_row(
        "SELECT min(zoom_level), max(zoom_level) FROM tiles WHERE zoom_level BETWEEN 0 AND ?1",
        [MAX_ZOOM],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (lowest, max_zoom) = min_zoom.zip(max_zoom).ok_or_else(empty)?;
    // The lowest level with a tile inside it (at most 32 short lookups).
    for min_zoom in lowest..=max_zoom {
        let Some(last) = flipped_row(min_zoom, 0) else {
            break;
        };
        let found: Option<(u32, u32)> = db
            .query_row(
                "SELECT tile_column, tile_row FROM tiles WHERE zoom_level = ?1
                 AND tile_column BETWEEN 0 AND ?2 AND tile_row BETWEEN 0 AND ?2 LIMIT 1",
                rusqlite::params![min_zoom, last],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((x, row)) = found
            && let Some(y) = flipped_row(min_zoom, row)
        {
            return Ok(ArchiveInfo {
                min_zoom,
                max_zoom,
                sample: TileKey { z: min_zoom, x, y },
            });
        }
    }
    Err(empty())
}

/// What an archive holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveInfo {
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// A tile the archive has.
    pub(crate) sample: TileKey,
}

fn open_mbtiles(path: &std::path::Path) -> Result<rusqlite::Connection, crate::Error> {
    Ok(rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// The zoom levels a `pmtiles:` or `mbtiles:` template's archive holds, and the size
/// of one of its tiles: the Add Data form's "Test".
pub async fn describe_archive(template: String) -> Result<(ArchiveInfo, (u32, u32)), crate::Error> {
    let archive = TileArchive::from_template(&template)
        .ok_or_else(|| crate::Error::msg("not a tile archive"))?;
    let info = archive.describe().await?;
    let bytes = archive
        .tile(info.sample)
        .await?
        .ok_or_else(|| crate::Error::msg("the archive has no tile where it says it does"))?;
    let image = crate::cpu::run(crate::cpu::Service::Tiles, move || {
        super::raster::decode(&bytes)
    })
    .await?;
    Ok((info, image.dimensions()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-tile MBTiles file at zoom 1, the north-east quarter.
    fn mbtiles_file(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("globecraft-{name}-{}.mbtiles", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);",
        )
        .unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(256, 256, image::Rgba([10, 200, 30, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        // TMS row 1 at zoom 1 is the northern row (XYZ y 0).
        db.execute("INSERT INTO tiles VALUES (1, 1, 1, ?1)", [png.into_inner()])
            .unwrap();
        path
    }

    #[test]
    fn mbtiles_files_are_read_with_their_rows_counted_from_the_south() {
        let path = mbtiles_file("archive-read");
        let template = format!("{MBTILES_PREFIX}{}", path.display());
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let archive = TileArchive::from_template(&template).unwrap();
        runtime.handle().block_on(async {
            let north_east = archive.tile(TileKey { z: 1, x: 1, y: 0 }).await.unwrap();
            assert!(north_east.is_some());
            assert!(
                archive
                    .tile(TileKey { z: 1, x: 1, y: 1 })
                    .await
                    .unwrap()
                    .is_none()
            );
            let (info, size) = describe_archive(template.clone()).await.unwrap();
            assert_eq!((info.min_zoom, info.max_zoom, size), (1, 1, (256, 256)));
            assert_eq!(info.sample, TileKey { z: 1, x: 1, y: 0 });
        });
        assert!(TileArchive::from_template("https://example.org/{z}/{x}/{y}.png").is_none());
        let _ = std::fs::remove_file(path);
        assert!(TileArchive::is_local("mbtiles:/maps/alps.mbtiles"));
        assert!(TileArchive::is_local("pmtiles:/maps/alps.pmtiles"));
        assert!(!TileArchive::is_local(
            "pmtiles:https://example.org/alps.pmtiles"
        ));
        assert!(!TileArchive::is_local(
            "https://example.org/{z}/{x}/{y}.png"
        ));
    }

    /// An in-memory MBTiles database with these `(zoom, column, row)` rows; the
    /// columns are untyped, as in a file somebody wrote by hand.
    fn database(rows: &[(i64, i64, i64)]) -> rusqlite::Connection {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE tiles (zoom_level, tile_column, tile_row, tile_data);")
            .unwrap();
        for row in rows {
            db.execute(
                "INSERT INTO tiles VALUES (?1, ?2, ?3, x'00')",
                rusqlite::params![row.0, row.1, row.2],
            )
            .unwrap();
        }
        db
    }

    /// Found by the `mbtiles_rows` fuzz target: rows outside their zoom level, and
    /// zoom levels of 32 and more, used to overflow the row arithmetic.
    #[test]
    fn mbtiles_rows_that_cannot_exist_are_ignored() {
        assert_eq!(flipped_row(0, 0), Some(0));
        assert_eq!(flipped_row(3, 2), Some(5));
        assert_eq!(flipped_row(3, 8), None);
        assert_eq!(flipped_row(31, 0), Some(u32::MAX >> 1));
        assert_eq!(flipped_row(32, 0), None);

        // Zoom 2 has rows 0 to 3: row 9 is not a tile, and neither is zoom 40.
        let db = database(&[(2, 1, 9), (40, 0, 0), (-1, 0, 0), (3, 7, 0), (5, 1, 1)]);
        let info = mbtiles_info(&db).unwrap();
        assert_eq!((info.min_zoom, info.max_zoom), (3, 5));
        assert_eq!(info.sample, TileKey { z: 3, x: 7, y: 7 });
        // No tile that can exist: there is nothing to show.
        assert!(mbtiles_info(&database(&[(2, 1, 9)])).is_err());
        assert!(mbtiles_info(&database(&[(40, 0, 0), (-3, 0, 0)])).is_err());
        assert!(mbtiles_info(&database(&[])).is_err());

        let db = database(&[(3, 7, 0), (5, 1, 1), (40, 0, 0)]);
        let info = mbtiles_info(&db).unwrap();
        assert_eq!((info.min_zoom, info.max_zoom), (3, 5));
        assert_eq!(info.sample, TileKey { z: 3, x: 7, y: 7 });
        assert!(mbtiles_tile(&db, info.sample).unwrap().is_some());
        // Keys no map has are simply absent.
        for key in [
            TileKey { z: 3, x: 7, y: 8 },
            TileKey { z: 40, x: 0, y: 0 },
            TileKey {
                z: 255,
                x: u32::MAX,
                y: u32::MAX,
            },
        ] {
            assert!(mbtiles_tile(&db, key).unwrap().is_none());
        }
    }

    #[test]
    fn a_pmtiles_header_with_impossible_zoom_levels_is_refused() {
        assert!(pmtiles_info((5, 2), (3, 0.0, 0.0)).is_err());
        assert!(pmtiles_info((0, 40), (40, 0.0, 0.0)).is_err());
        let info = pmtiles_info((2, 6), (9, 9.68, 47.54)).unwrap();
        assert_eq!(info.sample.z, 6);
    }
}
