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
/// The deepest level a web map is asked for.
const MAX_ZOOM: u8 = 22;
/// Web Mercator ends here.
const MAX_MERCATOR_LAT: f64 = 85.051_128_78;
/// The whole world is asked for at the layer's coarsest level when it is this few
/// tiles, so the globe is covered past the tiles round the view.
const MAX_COVER_TILES: u32 = 16;
/// The largest answer to GIBS's list of a layer's days (it is under a kilobyte).
const MAX_DOMAINS_BYTES: u64 = 256 * 1024;
const GIBS_WMTS: &str = "https://gibs.earthdata.nasa.gov/wmts/";
/// GIBS sometimes answers a tile with garbage (2026-11-24: the same OMPS ozone tile
/// came back as its true picture, a palette PNG, or as RGBA noise of another size,
/// differently each time). Its maps have no partly transparent pixels, so a tile with
/// more than this share of them is taken as damaged…
const DAMAGED_PARTLY_TRANSPARENT_SHARE: f64 = 0.01;
/// …and asked for again this soon.
const DAMAGED_RETRY: Duration = Duration::from_secs(2);

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
    /// For a GIBS layer's newest day (`gibs_domains_url`): the day shown instead.
    gibs_day: GibsDay,
}

/// The day a GIBS template's `default` stands for.
#[derive(Default)]
enum GibsDay {
    #[default]
    NotAsked,
    Asking,
    /// The day before GIBS's newest.
    Known(String),
    /// GIBS did not say: its `default` stays.
    Unknown,
}

/// The loaded tiles of one view, read without locking while the overlay is drawn.
pub struct RasterSnapshot {
    tiles: HashMap<TileKey, Arc<RgbaImage>>,
    zoom: u8,
    min_zoom: u8,
    /// The day before's map, seen through this day's gaps (`RasterTiles::under`).
    under: Option<Arc<RasterSnapshot>>,
}

impl RasterSnapshot {
    /// The colour at a point: from the finest loaded tile there, `None` where no tile
    /// covers it (or outside Web Mercator).
    pub fn sample(&self, lat: f64, lon: f64) -> Option<[u8; 4]> {
        let own = self.sample_own(lat, lon);
        match (&self.under, own) {
            (Some(under), None) => under.sample(lat, lon),
            (Some(under), Some(colour)) if colour[3] == 0 => under.sample(lat, lon).or(own),
            _ => own,
        }
    }

    fn sample_own(&self, lat: f64, lon: f64) -> Option<[u8; 4]> {
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

/// What a snapshot was built from: the tiles' revision and the day before's
/// snapshot (its address, 0 for none).
type SnapshotSource = (u64, usize);

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
    /// Where GIBS lists the days of a template asking for its newest day.
    gibs_domains: Option<String>,
    /// For such a template, the day before the day shown: a daily map from orbits
    /// leaves a strip between its first and last orbit, filled from the day before.
    under: Option<Box<RasterTiles>>,
    shared: Arc<Mutex<Shared>>,
    /// The last snapshot: the tiles wanted, the revision and the day before's
    /// snapshot it was built from.
    snapshot: Option<(Vec<TileKey>, SnapshotSource, Arc<RasterSnapshot>)>,
}

impl RasterTiles {
    pub fn new(template: &str, min_zoom: u8, max_zoom: u8, tile_px: u16) -> Self {
        Self {
            template: template.to_owned(),
            archive: super::archive::TileArchive::from_template(template),
            // A saved or imported layer may say anything: `clamp` panics when the
            // lowest level is above the highest.
            min_zoom: min_zoom.min(MAX_ZOOM),
            max_zoom: max_zoom.clamp(min_zoom.min(MAX_ZOOM), MAX_ZOOM),
            tile_px: tile_px.max(64),
            date: super::day_before(globecraft_core::math::now_unix()),
            gibs_domains: gibs_domains_url(template),
            under: None,
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

    /// Requests the tiles the view needs and returns what is loaded for it.
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        view: crate::ViewQuery,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) -> Option<Arc<RasterSnapshot>> {
        let crate::ViewQuery {
            latitude: lat,
            longitude: lon,
            altitude_km,
            tilt_deg,
            ..
        } = view;
        // Tiles across the view: a 256-pixel tile about every 400 screen pixels keeps
        // the overlay about as sharp as the imagery under it.
        let across = 4.0 * 256.0 / f64::from(self.tile_px);
        let mut zoom = ideal_zoom(lat, altitude_km, across)
            .round()
            .clamp(f64::from(self.min_zoom), f64::from(self.max_zoom)) as u8;
        let radius = view_radius_deg(altitude_km, tilt_deg);
        let mut wanted = loop {
            let tiles = around(zoom, lat, lon, radius);
            if tiles.len() <= MAX_WANTED || zoom <= self.min_zoom {
                break tiles.into_iter().take(MAX_WANTED).collect::<Vec<_>>();
            }
            zoom -= 1;
        };
        // The world at the coarsest level first, when it is a few tiles: the tiles
        // round the view left the rest of the globe empty from far away.
        let side = 1u32 << self.min_zoom;
        if side * side <= MAX_COVER_TILES {
            let cover: Vec<TileKey> = (0..side)
                .flat_map(|y| (0..side).map(move |x| (x, y)))
                .map(|(x, y)| TileKey {
                    z: self.min_zoom,
                    x,
                    y,
                })
                .filter(|key| !wanted.contains(key))
                .collect();
            wanted.splice(0..0, cover);
        }
        let now = Instant::now();
        let Ok(mut shared) = self.shared.lock() else {
            return None;
        };
        // GIBS's newest day is often only partly in (a few orbits of a daily map):
        // the day before it, once GIBS has said which that is.
        let template = match (&self.gibs_domains, &shared.gibs_day) {
            (None, _) | (Some(_), GibsDay::Unknown) => self.template.clone(),
            (Some(_), GibsDay::Known(day)) => {
                if self.under.is_none()
                    && let Some(before) = previous_day(day)
                {
                    self.under = Some(Box::new(RasterTiles::new(
                        &self.template.replacen(
                            "/default/default/",
                            &format!("/default/{before}/"),
                            1,
                        ),
                        self.min_zoom,
                        self.max_zoom,
                        self.tile_px,
                    )));
                }
                self.template
                    .replacen("/default/default/", &format!("/default/{day}/"), 1)
            }
            (Some(_), GibsDay::Asking) => return None,
            (Some(url), GibsDay::NotAsked) => {
                shared.gibs_day = GibsDay::Asking;
                let (state, url, repaint) = (self.shared.clone(), url.clone(), repaint.clone());
                runtime.spawn(async move {
                    let day = gibs_day_before_newest(url).await;
                    if let Ok(mut shared) = state.lock() {
                        shared.gibs_day = day.map_or(GibsDay::Unknown, GibsDay::Known);
                    }
                    repaint();
                });
                return None;
            }
        };
        let under = self
            .under
            .as_mut()
            .and_then(|under| under.update(runtime, view, repaint.clone()));
        let gibs = template.starts_with(GIBS_WMTS);
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
            let url = super::tile_url(&template, key.z, key.x, key.y, &self.date);
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
                        Ok(Some(image)) if gibs && damaged(&image) => {
                            let at = Instant::now();
                            let at = at.checked_sub(RETRY - DAMAGED_RETRY).unwrap_or(at);
                            shared.tiles.insert(key, Entry::Failed { at });
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
        let under_ptr = under
            .as_ref()
            .map_or(0, |under| Arc::as_ptr(under) as usize);
        if let Some((keys, built, snapshot)) = &self.snapshot
            && *keys == wanted
            && *built == (revision, under_ptr)
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
            under,
        });
        self.snapshot = Some((wanted, (revision, under_ptr), snapshot.clone()));
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

/// GIBS's list of a layer's days (DescribeDomains) for a template asking for its
/// newest day (`…/best/{layer}/default/default/{matrix set}/{z}/{y}/{x}.png`).
fn gibs_domains_url(template: &str) -> Option<String> {
    if !template.starts_with("https://gibs.earthdata.nasa.gov/wmts/") {
        return None;
    }
    let (before, after) = template.split_once("/default/default/")?;
    let (base, layer) = before.rsplit_once('/')?;
    let matrix_set = after.split('/').next()?;
    Some(format!(
        "{base}/1.0.0/{layer}/default/{matrix_set}/all/all.xml"
    ))
}

/// Whether a GIBS tile is garbage (`DAMAGED_PARTLY_TRANSPARENT_SHARE`).
fn damaged(image: &RgbaImage) -> bool {
    let partly = image
        .pixels()
        .filter(|pixel| !matches!(pixel.0[3], 0 | 255))
        .count();
    partly as f64 > DAMAGED_PARTLY_TRANSPARENT_SHARE * f64::from(image.width() * image.height())
}

/// The day before `day` (`YYYY-MM-DD`).
fn previous_day(day: &str) -> Option<String> {
    let secs = globecraft_core::time::parse_iso(day)?;
    Some(globecraft_core::time::date_utc(
        secs - globecraft_core::time::DAY_SECS,
    ))
}

/// The day before the newest in GIBS's list of a layer's days, or the newest when it
/// stands alone.
fn day_before_newest(domains: &str) -> Option<String> {
    let start = domains.find("<Domain>")? + "<Domain>".len();
    let end = start + domains[start..].find("</Domain>")?;
    let last = domains[start..end].split(',').next_back()?.trim();
    let mut parts = last.split('/');
    let (first, newest) = match (parts.next(), parts.next()) {
        (Some(first), Some(newest)) => (first, newest),
        (Some(only), None) => (only, only),
        _ => return None,
    };
    let newest_secs = globecraft_core::time::parse_iso(newest.get(..10)?)?;
    let first_secs = globecraft_core::time::parse_iso(first.get(..10)?)?;
    let before = newest_secs - globecraft_core::time::DAY_SECS;
    Some(globecraft_core::time::date_utc(if before >= first_secs {
        before
    } else {
        newest_secs
    }))
}

async fn gibs_day_before_newest(url: String) -> Option<String> {
    let client = crate::http::client(crate::http::Policy::UserData).ok()?;
    let bytes = crate::http::body(client.get(&url), MAX_DOMAINS_BYTES)
        .await
        .ok()?;
    day_before_newest(&String::from_utf8_lossy(&bytes))
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
    let url = crate::keys::fill(&url)?;
    let bytes = crate::http::body(client.get(&url), MAX_TILE_BYTES)
        .await
        .map_err(crate::keys::redact_error)?;
    crate::cpu::run(crate::cpu::Service::Tiles, move || decode(&bytes)).await
}

pub(crate) fn decode(bytes: &[u8]) -> Result<RgbaImage, crate::Error> {
    // A WMS answers an error as XML with status 200.
    if bytes.starts_with(b"<") || bytes.starts_with(b"\xef\xbb\xbf<") {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]);
        return Err(crate::Error::msg(format!(
            "the server answered with text, not an image: {}",
            text.trim()
        )));
    }
    Ok(crate::bounded::image(bytes, crate::bounded::ImageLimit::TILE)?.into_rgba8())
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
            under: None,
        };
        assert_eq!(snapshot.sample(40.0, -90.0), Some([200; 4]));
        assert_eq!(snapshot.sample(-40.0, 90.0), Some([10; 4]));
        assert_eq!(snapshot.sample(89.0, 0.0), None);
    }

    #[test]
    fn a_gibs_layers_newest_day_is_read_from_its_list_of_days() {
        let template = "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/\
                        OMPS_Ozone_Total_Column/default/default/\
                        GoogleMapsCompatible_Level6/{z}/{y}/{x}.png";
        assert_eq!(
            gibs_domains_url(template).as_deref(),
            Some(
                "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/1.0.0/\
                 OMPS_Ozone_Total_Column/default/GoogleMapsCompatible_Level6/all/all.xml"
            )
        );
        assert_eq!(
            gibs_domains_url("https://gibs.earthdata.nasa.gov/wmts/x/{date}/{z}"),
            None
        );
        assert_eq!(
            gibs_domains_url("https://example.com/default/default/a/{z}"),
            None
        );
        // As GIBS answered on 2026-11-24: the newest day held a few orbits.
        let domains = "<Domains><DimensionDomain><ows:Identifier>time</ows:Identifier>\
                       <Domain>2012-01-26/2012-01-28/P1D,2022-08-10/2026-07-10/P1D,\
                       2026-07-17/2026-11-02/P1D</Domain></DimensionDomain></Domains>";
        assert_eq!(day_before_newest(domains).as_deref(), Some("2026-11-01"));
        let alone = "<Domain>2026-07-10/2026-07-10/P1D,2026-11-02</Domain>";
        assert_eq!(day_before_newest(alone).as_deref(), Some("2026-11-02"));
        assert_eq!(day_before_newest("no list"), None);
        assert_eq!(previous_day("2026-03-01").as_deref(), Some("2026-02-28"));
    }

    #[test]
    fn garbage_from_gibs_is_told_from_its_maps() {
        // A map: opaque colours and clear pixels.
        let map = RgbaImage::from_fn(16, 16, |x, _| {
            image::Rgba(if x < 8 {
                [0, 0, 0, 0]
            } else {
                [210, 237, 156, 255]
            })
        });
        assert!(!damaged(&map));
        // RGB bytes read as RGBA, as GIBS sent them: the alpha is any byte.
        let garbage = RgbaImage::from_fn(16, 16, |x, y| {
            let n = (x + y * 16) as u8;
            image::Rgba([63, 21, 39, n.wrapping_mul(37)])
        });
        assert!(damaged(&garbage));
    }

    #[test]
    fn a_gap_in_a_day_shows_the_day_before() {
        let tile = |colour: [u8; 4]| {
            let mut tiles = HashMap::new();
            tiles.insert(
                TileKey { z: 0, x: 0, y: 0 },
                Arc::new(RgbaImage::from_pixel(4, 4, image::Rgba(colour))),
            );
            tiles
        };
        let before = Arc::new(RasterSnapshot {
            tiles: tile([10, 20, 30, 255]),
            zoom: 0,
            min_zoom: 0,
            under: None,
        });
        let gap = RasterSnapshot {
            tiles: tile([0, 0, 0, 0]),
            zoom: 0,
            min_zoom: 0,
            under: Some(before.clone()),
        };
        assert_eq!(gap.sample(10.0, 10.0), Some([10, 20, 30, 255]));
        let filled = RasterSnapshot {
            tiles: tile([200, 0, 0, 255]),
            zoom: 0,
            min_zoom: 0,
            under: Some(before),
        };
        assert_eq!(filled.sample(10.0, 10.0), Some([200, 0, 0, 255]));
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
    fn zoom_levels_out_of_range_are_brought_into_it() {
        // From a hand-edited settings file or an imported source list.
        let layer = RasterTiles::new("https://t.example/{z}/{x}/{y}.png", 40, 3, 256);
        assert_eq!((layer.min_zoom, layer.max_zoom), (22, 22));
        let layer = RasterTiles::new("https://t.example/{z}/{x}/{y}.png", 5, 200, 256);
        assert_eq!((layer.min_zoom, layer.max_zoom), (5, 22));
        let layer = RasterTiles::new("https://t.example/{z}/{x}/{y}.png", 9, 2, 256);
        assert_eq!((layer.min_zoom, layer.max_zoom), (9, 9));
    }

    #[test]
    fn service_errors_in_place_of_an_image_are_reported() {
        let error = decode(b"<?xml version=\"1.0\"?><ServiceExceptionReport/>").unwrap_err();
        assert!(error.to_string().contains("ServiceException"), "{error}");
    }
}
