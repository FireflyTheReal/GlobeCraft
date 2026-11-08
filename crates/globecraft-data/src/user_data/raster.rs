//! Tiles of a user's web map, streamed for the view: the level for the altitude, the
//! tiles around the view nearest first, a few downloads at a time, decoded off the
//! interface thread and kept in memory. Coarser loaded tiles stand in while finer
//! ones arrive.
use crate::boundaries::osm_tiles::content::TileKey;
use crate::boundaries::osm_tiles::pick::{around, ideal_zoom, view_radius_deg};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Downloads running at once per layer.
const MAX_INFLIGHT: usize = 8;
/// Tiles kept per layer.
const MAX_TILES: usize = 384;
/// The most tiles one view asks for.
const MAX_WANTED: usize = 64;
/// A tile that failed is asked for again after this long.
const RETRY: Duration = Duration::from_secs(60);
/// The largest tile accepted.
const MAX_TILE_BYTES: u64 = 8 * 1024 * 1024;
/// Web Mercator ends here.
const MAX_MERCATOR_LAT: f64 = 85.051_128_78;

enum Entry {
    Loading,
    Loaded {
        image: Arc<RgbaImage>,
        used: Instant,
    },
    Failed {
        at: Instant,
    },
}

#[derive(Default)]
struct Shared {
    tiles: HashMap<TileKey, Entry>,
    /// Bumped whenever a tile arrives.
    revision: u64,
    /// The last download error, for the layer's status.
    error: Option<String>,
    loaded_any: bool,
    /// Bumped when the day changes: downloads for an earlier day are dropped.
    day: u64,
    /// The tiles of earlier days, shown where the current day's have not arrived.
    previous: HashMap<TileKey, Arc<RgbaImage>>,
}

/// The loaded tiles of one view, read without locking while the overlay is drawn.
pub struct RasterSnapshot {
    tiles: HashMap<TileKey, Arc<RgbaImage>>,
    zoom: u8,
    min_zoom: u8,
}

impl RasterSnapshot {
    /// The colour at a point: from the finest loaded tile there, `None` where no tile
    /// covers it (or outside Web Mercator).
    pub fn sample(&self, lat: f64, lon: f64) -> Option<[u8; 4]> {
        if lat.abs() > MAX_MERCATOR_LAT {
            return None;
        }
        let x_unit = (lon + 180.0) / 360.0;
        let radians = lat.to_radians();
        let y_unit = (1.0 - radians.tan().asinh() / std::f64::consts::PI) / 2.0;
        let mut zoom = self.zoom;
        loop {
            let n = f64::from(1u32 << zoom);
            let (fx, fy) = (x_unit.rem_euclid(1.0) * n, y_unit.clamp(0.0, 0.999_999) * n);
            let key = TileKey {
                z: zoom,
                x: (fx as u32).min((1u32 << zoom) - 1),
                y: (fy as u32).min((1u32 << zoom) - 1),
            };
            if let Some(image) = self.tiles.get(&key) {
                let (w, h) = image.dimensions();
                let px = ((fx.fract() * f64::from(w)) as u32).min(w - 1);
                let py = ((fy.fract() * f64::from(h)) as u32).min(h - 1);
                return Some(image.get_pixel(px, py).0);
            }
            if zoom <= self.min_zoom {
                return None;
            }
            zoom -= 1;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

/// One web map layer's tiles.
pub struct RasterTiles {
    template: String,
    /// The PMTiles or MBTiles archive the template names, read instead of a web address.
    archive: Option<super::archive::TileArchive>,
    min_zoom: u8,
    max_zoom: u8,
    tile_px: u16,
    /// The day `{date}` stands for (`YYYY-MM-DD`).
    date: String,
    shared: Arc<Mutex<Shared>>,
    snapshot: Option<(Vec<TileKey>, u64, Arc<RasterSnapshot>)>,
}

impl RasterTiles {
    pub fn new(template: &str, min_zoom: u8, max_zoom: u8, tile_px: u16) -> Self {
        Self {
            template: template.to_owned(),
            archive: super::archive::TileArchive::from_template(template),
            min_zoom,
            max_zoom: max_zoom.clamp(min_zoom, 22),
            tile_px: tile_px.max(64),
            date: super::day_before(globecraft_core::math::now_unix()),
            shared: Arc::default(),
            snapshot: None,
        }
    }

    /// The day a template's `{date}` stands for: daily imagery follows the app's
    /// time. Another day starts the tiles over.
    pub fn set_date(&mut self, date: String) {
        if date == self.date {
            return;
        }
        self.date = date;
        if self.template.contains("{date}") {
            if let Ok(mut shared) = self.shared.lock() {
                let shared = &mut *shared;
                shared.day += 1;
                let loaded: Vec<(TileKey, Arc<RgbaImage>)> = shared
                    .tiles
                    .drain()
                    .filter_map(|(key, entry)| match entry {
                        Entry::Loaded { image, .. } => Some((key, image)),
                        _ => None,
                    })
                    .collect();
                // The newest tile of each place stays until the new day's arrives.
                if shared.previous.len() + loaded.len() > MAX_TILES {
                    shared.previous.clear();
                }
                shared.previous.extend(loaded);
            }
            self.snapshot = None;
        }
    }

    /// Requests the tiles the view needs and returns what is loaded for it. `view`:
    /// latitude, longitude, altitude (km), tilt (degrees).
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        view: (f64, f64, f64, f64),
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) -> Option<Arc<RasterSnapshot>> {
        let (lat, lon, altitude_km, tilt_deg) = view;
        // Tiles across the view: a 256-pixel tile about every 400 screen pixels keeps
        // the overlay about as sharp as the imagery under it.
        let across = 4.0 * 256.0 / f64::from(self.tile_px);
        let mut zoom = ideal_zoom(lat, altitude_km, across)
            .round()
            .clamp(f64::from(self.min_zoom), f64::from(self.max_zoom)) as u8;
        let radius = view_radius_deg(altitude_km, tilt_deg);
        let wanted = loop {
            let tiles = around(zoom, lat, lon, radius);
            if tiles.len() <= MAX_WANTED || zoom <= self.min_zoom {
                break tiles.into_iter().take(MAX_WANTED).collect::<Vec<_>>();
            }
            zoom -= 1;
        };
        let now = Instant::now();
        let Ok(mut shared) = self.shared.lock() else {
            return None;
        };
        let mut inflight = shared
            .tiles
            .values()
            .filter(|entry| matches!(entry, Entry::Loading))
            .count();
        for key in &wanted {
            if inflight >= MAX_INFLIGHT {
                break;
            }
            let due = match shared.tiles.get(key) {
                None => true,
                Some(Entry::Failed { at }) => now.duration_since(*at) >= RETRY,
                Some(_) => false,
            };
            if !due {
                continue;
            }
            shared.tiles.insert(*key, Entry::Loading);
            inflight += 1;
            let url = super::tile_url(&self.template, key.z, key.x, key.y, &self.date);
            let (state, key, repaint) = (self.shared.clone(), *key, repaint.clone());
            let archive = self.archive.clone();
            let day = shared.day;
            runtime.spawn(async move {
                let result = match archive {
                    Some(archive) => archive_tile(&archive, key).await,
                    None => fetch_tile(url).await.map(Some),
                };
                if let Ok(mut shared) = state.lock() {
                    if shared.day != day {
                        // Another day is showing by now.
                        return;
                    }
                    match result {
                        // The archive has no tile here: coarser ones stand in.
                        Ok(None) => {
                            shared
                                .tiles
                                .insert(key, Entry::Failed { at: Instant::now() });
                        }
                        Ok(Some(image)) => {
                            shared.tiles.insert(
                                key,
                                Entry::Loaded {
                                    image: Arc::new(image),
                                    used: Instant::now(),
                                },
                            );
                            shared.loaded_any = true;
                            shared.error = None;
                        }
                        Err(error) => {
                            shared
                                .tiles
                                .insert(key, Entry::Failed { at: Instant::now() });
                            shared.error = Some(error.to_string());
                        }
                    }
                    shared.revision += 1;
                    evict(&mut shared.tiles);
                }
                repaint();
            });
        }
        let revision = shared.revision;
        if let Some((keys, built, snapshot)) = &self.snapshot
            && *keys == wanted
            && *built == revision
        {
            return Some(snapshot.clone());
        }
        // The wanted tiles and every loaded ancestor of them, for the fallback; an
        // earlier day's tile where this day's has not arrived.
        let mut tiles = HashMap::new();
        for key in &wanted {
            if !matches!(shared.tiles.get(key), Some(Entry::Loaded { .. }))
                && let Some(image) = shared.previous.get(key)
            {
                tiles.insert(*key, image.clone());
            }
        }
        for key in &wanted {
            let mut at = Some(*key);
            while let Some(k) = at {
                if let Some(Entry::Loaded { image, used }) = shared.tiles.get_mut(&k) {
                    *used = now;
                    tiles.entry(k).or_insert_with(|| image.clone());
                }
                at = k.parent().filter(|parent| parent.z >= self.min_zoom);
            }
        }
        let snapshot = Arc::new(RasterSnapshot {
            tiles,
            zoom,
            min_zoom: self.min_zoom,
        });
        self.snapshot = Some((wanted, revision, snapshot.clone()));
        Some(snapshot)
    }

    /// Why tiles fail, when they do and none has loaded.
    pub fn error(&self) -> Option<String> {
        let shared = self.shared.lock().ok()?;
        (!shared.loaded_any).then(|| shared.error.clone()).flatten()
    }

    pub fn loading(&self) -> bool {
        self.shared.lock().is_ok_and(|shared| {
            shared
                .tiles
                .values()
                .any(|entry| matches!(entry, Entry::Loading))
        })
    }
}

fn evict(tiles: &mut HashMap<TileKey, Entry>) {
    if tiles.len() <= MAX_TILES {
        return;
    }
    let mut loaded: Vec<(Instant, TileKey)> = tiles
        .iter()
        .filter_map(|(key, entry)| match entry {
            Entry::Loaded { used, .. } => Some((*used, *key)),
            Entry::Failed { at } => Some((*at, *key)),
            Entry::Loading => None,
        })
        .collect();
    loaded.sort();
    for (_, key) in loaded.into_iter().take(tiles.len() - MAX_TILES) {
        tiles.remove(&key);
    }
}

async fn archive_tile(
    archive: &super::archive::TileArchive,
    key: TileKey,
) -> Result<Option<RgbaImage>, crate::Error> {
    let Some(bytes) = archive.tile(key).await? else {
        return Ok(None);
    };
    crate::cpu::run(crate::cpu::Service::Tiles, move || decode(&bytes))
        .await
        .map(Some)
}

async fn fetch_tile(url: String) -> Result<RgbaImage, crate::Error> {
    let client = crate::http::client(crate::http::Policy::UserData)?;
    let bytes = crate::http::body(client.get(&url), MAX_TILE_BYTES).await?;
    crate::cpu::run(crate::cpu::Service::Tiles, move || decode(&bytes)).await
}

pub(super) fn decode(bytes: &[u8]) -> Result<RgbaImage, crate::Error> {
    // A WMS answers an error as XML with status 200.
    if bytes.starts_with(b"<") || bytes.starts_with(b"\xef\xbb\xbf<") {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]);
        return Err(format!(
            "the server answered with text, not an image: {}",
            text.trim()
        )
        .into());
    }
    Ok(image::load_from_memory(bytes)?.to_rgba8())
}

/// Fetches one tile of a template (the form's "Test"): its size in pixels.
pub async fn test_tile(template: String, zoom: u8) -> Result<(u32, u32), crate::Error> {
    // The tile under 0°, 0° at `zoom`, or the world tile.
    let key = TileKey::at(zoom, 0.0, 0.0);
    let date = super::day_before(globecraft_core::math::now_unix());
    let image = fetch_tile(super::tile_url(&template, key.z, key.x, key.y, &date)).await?;
    Ok(image.dimensions())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_takes_the_finest_loaded_tile() {
        let mut coarse = RgbaImage::new(2, 2);
        coarse.fill(10);
        let mut fine = RgbaImage::new(2, 2);
        fine.fill(200);
        let mut tiles = HashMap::new();
        tiles.insert(TileKey { z: 0, x: 0, y: 0 }, Arc::new(coarse));
        // Zoom 1, top-left tile: north-west quarter.
        tiles.insert(TileKey { z: 1, x: 0, y: 0 }, Arc::new(fine));
        let snapshot = RasterSnapshot {
            tiles,
            zoom: 1,
            min_zoom: 0,
        };
        assert_eq!(snapshot.sample(40.0, -90.0), Some([200; 4]));
        assert_eq!(snapshot.sample(-40.0, 90.0), Some([10; 4]));
        assert_eq!(snapshot.sample(89.0, 0.0), None);
    }

    #[test]
    fn another_day_keeps_the_earlier_tiles_until_its_own_arrive() {
        let mut layer = RasterTiles::new("https://gibs.example/{date}/{z}/{y}/{x}.jpg", 0, 9, 256);
        layer.set_date("2026-11-01".into());
        let key = TileKey { z: 1, x: 0, y: 0 };
        {
            let mut shared = layer.shared.lock().unwrap();
            shared.tiles.insert(
                key,
                Entry::Loaded {
                    image: Arc::new(RgbaImage::new(1, 1)),
                    used: Instant::now(),
                },
            );
            shared
                .tiles
                .insert(TileKey { z: 1, x: 1, y: 0 }, Entry::Loading);
        }
        let day = layer.shared.lock().unwrap().day;
        layer.set_date("2026-11-02".into());
        let shared = layer.shared.lock().unwrap();
        // Nothing of the earlier day counts as this day's, its downloads are dropped,
        // and its loaded tile stands in.
        assert!(shared.tiles.is_empty());
        assert_eq!(shared.day, day + 1);
        assert!(shared.previous.contains_key(&key));
    }

    #[test]
    fn service_errors_in_place_of_an_image_are_reported() {
        let error = decode(b"<?xml version=\"1.0\"?><ServiceExceptionReport/>").unwrap_err();
        assert!(error.to_string().contains("ServiceException"), "{error}");
    }
}
