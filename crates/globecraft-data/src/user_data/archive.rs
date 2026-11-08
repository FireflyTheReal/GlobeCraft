//! Tile archives as a user's map layer (roadmap 1.2 and 2.1): a PMTiles archive on the
//! web, read by ranges, or an MBTiles file on this computer. A layer's template names
//! one as `pmtiles:<address>` or `mbtiles:<path>`; images in either are drawn like a
//! web map's tiles.
use crate::boundaries::osm_tiles::content::TileKey;
use crate::pmtiles::Archive;
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
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>, crate::Error> {
            let db = open_mbtiles(&path)?;
            // MBTiles rows count from the south (TMS).
            let row = (1u32 << key.z) - 1 - key.y;
            Ok(db
                .query_row(
                    "SELECT tile_data FROM tiles WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
                    rusqlite::params![key.z, key.x, row],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .optional()?)
        })
        .await
        .map_err(|error| crate::Error::from(error.to_string()))?
    }

    /// The zoom levels held and a tile that exists, for the Add Data form.
    pub(super) async fn describe(&self) -> Result<ArchiveInfo, crate::Error> {
        if let Some(archive) = self.archive().await? {
            let (min_zoom, max_zoom) = archive.zooms();
            let (zoom, lon, lat) = archive.center();
            let zoom = zoom.clamp(min_zoom, max_zoom);
            return Ok(ArchiveInfo {
                min_zoom,
                max_zoom,
                sample: TileKey::at(zoom, lat, lon),
            });
        }
        let Self::Mbtiles(path) = self else {
            unreachable!("PMTiles answered above")
        };
        let path = path.clone();
        tokio::task::spawn_blocking(move || -> Result<ArchiveInfo, crate::Error> {
            let db = open_mbtiles(&path)?;
            let (min_zoom, max_zoom, x, row): (u8, u8, u32, u32) = db.query_row(
                "SELECT (SELECT min(zoom_level) FROM tiles), (SELECT max(zoom_level) FROM tiles), tile_column, tile_row
                 FROM tiles ORDER BY zoom_level LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or("the MBTiles file holds no tiles")?;
            Ok(ArchiveInfo {
                min_zoom,
                max_zoom,
                sample: TileKey {
                    z: min_zoom,
                    x,
                    y: (1u32 << min_zoom) - 1 - row,
                },
            })
        })
        .await
        .map_err(|error| crate::Error::from(error.to_string()))?
    }
}

/// What an archive holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveInfo {
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// A tile the archive has.
    pub(super) sample: TileKey,
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
    let archive = TileArchive::from_template(&template).ok_or("not a tile archive")?;
    let info = archive.describe().await?;
    let bytes = archive
        .tile(info.sample)
        .await?
        .ok_or("the archive has no tile where it says it does")?;
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
    }
}
