//! The border tile cache on disk: one SQLite file, one row per tile in the compact
//! format, kept under a byte limit by dropping the tiles used longest ago.
//!
//! A tile is stored with the tile set version it came from (OpenFreeMap publishes a new
//! set weekly). A tile of an older version is still shown, and refetched when the
//! network is free, so a new version never leaves the globe without borders.
use super::compact::FORMAT_VERSION;
use super::content::TileKey;
use rusqlite::{OptionalExtension, params};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "osm_borders.sqlite";
/// Reading a tile marks it used, at most this often (so reads rarely write).
const TOUCH_INTERVAL_SECS: i64 = 3_600;
/// Tiles dropped per round while the file is over its limit.
const EVICT_BATCH: i64 = 64;
const BUSY_TIMEOUT_MS: u64 = 1_000;
const TEMPLATE_KEY: &str = "tile_url_template";

/// A stored tile and whether it belongs to the current tile set.
pub(super) struct Stored {
    pub(super) bytes: Vec<u8>,
    pub(super) current: bool,
}

pub(super) struct DiskStore {
    path: PathBuf,
    connection: tokio::sync::OnceCell<tokio_rusqlite::Connection>,
    /// The file's size after the last write, for the settings page.
    usage: Arc<AtomicU64>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The file's size in bytes, from SQLite's own page count (what eviction reclaims).
fn file_bytes(db: &rusqlite::Connection) -> rusqlite::Result<u64> {
    let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    Ok((pages * size).max(0) as u64)
}

impl DiskStore {
    /// The cache in GlobeCraft's data folder.
    pub(super) fn in_data_folder() -> Self {
        Self::at(crate::paths::cache_dir().join(FILE_NAME))
    }

    pub(super) fn at(path: PathBuf) -> Self {
        Self {
            path,
            connection: tokio::sync::OnceCell::new(),
            usage: Arc::default(),
        }
    }

    pub(super) fn usage(&self) -> u64 {
        self.usage.load(Ordering::Relaxed)
    }

    async fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut rusqlite::Connection) -> Result<T, crate::Error> + Send + 'static,
    ) -> Result<T, crate::Error> {
        let usage = self.usage.clone();
        let path = self.path.clone();
        let connection = self
            .connection
            .get_or_try_init(|| async move {
                if let Some(folder) = path.parent() {
                    std::fs::create_dir_all(folder)?;
                }
                let connection = tokio_rusqlite::Connection::open(&path).await?;
                connection
                    .call_raw(move |db| -> Result<(), crate::Error> {
                        db.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))?;
                        // Must precede the first table: deleted pages go back to the file
                        // system at each commit, as in the imagery cache.
                        db.execute_batch(
                            "PRAGMA auto_vacuum = FULL;
                             PRAGMA journal_mode = DELETE;
                             PRAGMA synchronous = NORMAL;
                             CREATE TABLE IF NOT EXISTS tiles (
                                 z INTEGER NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL,
                                 version TEXT NOT NULL, format INTEGER NOT NULL,
                                 data BLOB NOT NULL, used INTEGER NOT NULL,
                                 PRIMARY KEY (z, x, y));
                             CREATE INDEX IF NOT EXISTS tiles_used ON tiles (used);
                             CREATE TABLE IF NOT EXISTS meta (
                                 key TEXT PRIMARY KEY, value TEXT NOT NULL);",
                        )?;
                        usage.store(file_bytes(db)?, Ordering::Relaxed);
                        Ok(())
                    })
                    .await??;
                Ok::<_, crate::Error>(connection)
            })
            .await?;
        connection.call_raw(work).await?
    }

    /// The stored tile, if any (of any tile set version, but only of this format).
    pub(super) async fn read(
        &self,
        key: TileKey,
        version: String,
    ) -> Result<Option<Stored>, crate::Error> {
        self.call(move |db| {
            let row = db
                .query_row(
                    "SELECT data, version, used FROM tiles
                     WHERE z = ?1 AND x = ?2 AND y = ?3 AND format = ?4",
                    params![key.z, key.x, key.y, FORMAT_VERSION],
                    |r| {
                        Ok((
                            r.get::<_, Vec<u8>>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    },
                )
                .optional()?;
            let Some((bytes, stored_version, used)) = row else {
                return Ok(None);
            };
            let now = now_secs();
            if now - used > TOUCH_INTERVAL_SECS {
                db.execute(
                    "UPDATE tiles SET used = ?4 WHERE z = ?1 AND x = ?2 AND y = ?3",
                    params![key.z, key.x, key.y, now],
                )?;
            }
            Ok(Some(Stored {
                bytes,
                current: stored_version == version,
            }))
        })
        .await
    }

    /// Stores a tile, then drops the tiles used longest ago until the file fits
    /// `limit_bytes`, whose pages go back to the file system (`auto_vacuum`).
    pub(super) async fn write(
        &self,
        key: TileKey,
        version: String,
        bytes: Vec<u8>,
        limit_bytes: u64,
    ) -> Result<(), crate::Error> {
        let usage = self.usage.clone();
        self.call(move |db| {
            db.execute(
                "INSERT OR REPLACE INTO tiles (z, x, y, version, format, data, used)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    key.z,
                    key.x,
                    key.y,
                    version,
                    FORMAT_VERSION,
                    bytes,
                    now_secs()
                ],
            )?;
            evict(db, limit_bytes)?;
            usage.store(file_bytes(db)?, Ordering::Relaxed);
            Ok(())
        })
        .await
    }

    /// Applies a smaller limit at once (from the settings page).
    pub(super) async fn shrink(&self, limit_bytes: u64) -> Result<(), crate::Error> {
        let usage = self.usage.clone();
        self.call(move |db| {
            evict(db, limit_bytes)?;
            usage.store(file_bytes(db)?, Ordering::Relaxed);
            Ok(())
        })
        .await
    }

    /// The tile URL last read from the TileJSON, so an offline start can use the cache.
    pub(super) async fn template(&self) -> Result<Option<String>, crate::Error> {
        self.call(|db| {
            Ok(db
                .query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    [TEMPLATE_KEY],
                    |r| r.get(0),
                )
                .optional()?)
        })
        .await
    }

    pub(super) async fn set_template(&self, template: String) -> Result<(), crate::Error> {
        self.call(move |db| {
            db.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                params![TEMPLATE_KEY, template],
            )?;
            Ok(())
        })
        .await
    }
}

fn evict(db: &mut rusqlite::Connection, limit_bytes: u64) -> Result<(), crate::Error> {
    while file_bytes(db)? > limit_bytes {
        let deleted = db.execute(
            "DELETE FROM tiles WHERE (z, x, y) IN
                 (SELECT z, x, y FROM tiles ORDER BY used LIMIT ?1)",
            [EVICT_BATCH],
        )?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> (DiskStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "globecraft-border-cache-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (DiskStore::at(dir.join(FILE_NAME)), dir)
    }

    fn key(x: u32) -> TileKey {
        TileKey { z: 12, x, y: 1400 }
    }

    #[tokio::test]
    async fn tiles_and_the_template_survive_and_older_versions_stay_usable() {
        let (store, dir) = temp_store("versions");
        assert!(store.read(key(1), "v1".into()).await.unwrap().is_none());
        store
            .write(key(1), "v1".into(), vec![FORMAT_VERSION, 7], u64::MAX)
            .await
            .unwrap();
        store
            .set_template("https://x/v1/{z}/{x}/{y}.pbf".into())
            .await
            .unwrap();
        let hit = store.read(key(1), "v1".into()).await.unwrap().unwrap();
        assert_eq!((hit.bytes, hit.current), (vec![FORMAT_VERSION, 7], true));
        let stale = store.read(key(1), "v2".into()).await.unwrap().unwrap();
        assert!(
            !stale.current,
            "an older set's tile is still shown, marked stale"
        );
        drop(store);
        let reopened = DiskStore::at(dir.join(FILE_NAME));
        assert_eq!(
            reopened.template().await.unwrap().as_deref(),
            Some("https://x/v1/{z}/{x}/{y}.pbf")
        );
        assert!(reopened.read(key(1), "v1".into()).await.unwrap().is_some());
        assert!(reopened.usage() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_file_stays_under_its_limit_dropping_the_oldest_tiles() {
        let (store, dir) = temp_store("limit");
        let limit = 256 * 1024;
        for x in 0..200 {
            store
                .write(key(x), "v1".into(), vec![x as u8; 8 * 1024], limit)
                .await
                .unwrap();
        }
        assert!(store.usage() <= limit, "{} > {limit}", store.usage());
        assert!(store.read(key(0), "v1".into()).await.unwrap().is_none());
        assert!(store.read(key(199), "v1".into()).await.unwrap().is_some());
        store.shrink(64 * 1024).await.unwrap();
        assert!(store.usage() <= 64 * 1024, "{}", store.usage());
        let _ = std::fs::remove_dir_all(dir);
    }
}
