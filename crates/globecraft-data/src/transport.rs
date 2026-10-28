//! Railways, trams and subways, ferries and airports from GlobeCraft's own `transport`
//! tile set (OpenMapTiles `transportation`, `aeroway` and `aerodrome_label`, cut from
//! the OpenFreeMap planet to zoom 14; `crate::map_data`). Streamed like the border
//! tiles: a zoom level for the altitude, the tiles around the view, nearest first, and
//! coarser loaded tiles drawn while finer ones arrive.
use crate::boundaries::osm_tiles::content::{TileBounds, TileKey, clip};
use crate::boundaries::osm_tiles::pick::{around, ideal_zoom, view_radius_deg};
use crate::mvt::{self, GeomType};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Above this the layers are hidden: railways and airports are lost in the imagery.
pub const MAX_ALTITUDE_KM: f64 = 3_000.0;
/// The coarsest and finest levels of the set.
const MIN_ZOOM: u8 = 5;
const MAX_ZOOM: u8 = 14;
/// Tiles across the view (finer than borders: rail lines are drawn at every level).
const TILES_ACROSS_VIEW: f64 = 3.0;
/// More tiles than this go one level up instead.
const MAX_TILES: usize = 64;
const MAX_INFLIGHT: usize = 6;
/// Decoded tiles kept in memory.
const MAX_CACHED_TILES: usize = 600;
const FAILED_RETRY: Duration = Duration::from_secs(30);
const TEMPLATE_RETRY: Duration = Duration::from_secs(60);

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// Main railway lines (also narrow gauge, heritage, funicular).
    Rail,
    /// Yards, sidings and spurs.
    RailService,
    Subway,
    /// Light rail, monorail and trams.
    Tram,
    Ferry,
    Runway,
    Taxiway,
}

impl LineKind {
    pub fn is_rail(self) -> bool {
        matches!(
            self,
            Self::Rail | Self::RailService | Self::Subway | Self::Tram
        )
    }
    pub fn is_airport(self) -> bool {
        matches!(self, Self::Runway | Self::Taxiway)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransportLine {
    pub kind: LineKind,
    /// In a tunnel: drawn fainter.
    pub tunnel: bool,
    /// Latitude/longitude, clipped to the tile.
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Airport {
    pub lat: f64,
    pub lon: f64,
    pub name: String,
    pub iata: Option<String>,
    pub icao: Option<String>,
    /// OpenMapTiles `class`: international, public, regional, military, private, other.
    pub class: String,
}

impl Airport {
    /// International airports first, then public and regional ones.
    pub fn importance(&self) -> u8 {
        match self.class.as_str() {
            "international" => 0,
            "public" => 1,
            "regional" => 2,
            "military" => 3,
            _ => 4,
        }
    }
}

/// One tile's content.
#[derive(Debug, Default)]
pub struct TransportTile {
    pub lines: Vec<TransportLine>,
    pub airports: Vec<Airport>,
}

/// The tiles to draw for the current view: wanted tiles that are loaded, and coarser
/// loaded tiles in place of those still loading.
#[derive(Debug, Default)]
pub struct TransportSnapshot {
    pub tiles: Vec<Arc<TransportTile>>,
    /// The level the view wants.
    pub zoom: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportQuery {
    pub enabled: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_km: f64,
    pub tilt_deg: f64,
}

#[derive(Default)]
struct Shared {
    template: Option<Arc<str>>,
    template_loading: bool,
    template_failed: Option<Instant>,
    template_error: Option<String>,
    tiles: HashMap<TileKey, (Arc<TransportTile>, Instant)>,
    inflight: HashSet<TileKey>,
    failed: HashMap<TileKey, Instant>,
    /// Bumped whenever a tile arrives.
    revision: u64,
}

/// The transport tile layer: call [`Self::update`] every frame it is shown.
#[derive(Default)]
pub struct TransportTiles {
    shared: Arc<Mutex<Shared>>,
    snapshot: Option<Arc<TransportSnapshot>>,
    /// What the snapshot was built from: the wanted tiles and the revision.
    built: Option<(Vec<TileKey>, u64)>,
}

impl TransportTiles {
    /// The tiles to draw, when the layer is on and something is loaded.
    pub fn shared_snapshot(&self) -> Option<Arc<TransportSnapshot>> {
        self.snapshot
            .clone()
            .filter(|snapshot| !snapshot.tiles.is_empty())
    }

    /// Why nothing shows yet, if something is wrong.
    pub fn error(&self) -> Option<String> {
        self.shared.lock().ok()?.template_error.clone()
    }

    /// Whether downloads are running.
    pub fn loading(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|shared| !shared.inflight.is_empty())
    }

    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        query: TransportQuery,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        if !query.enabled || query.altitude_km > MAX_ALTITUDE_KM {
            self.snapshot = None;
            self.built = None;
            return;
        }
        let (zoom, wanted) = wanted(
            query.latitude,
            query.longitude,
            query.altitude_km,
            query.tilt_deg,
        );
        let now = Instant::now();
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let Some(template) = shared.template.clone() else {
            let due = shared
                .template_failed
                .is_none_or(|at| now.duration_since(at) >= TEMPLATE_RETRY);
            if !shared.template_loading && due {
                shared.template_loading = true;
                let state = self.shared.clone();
                runtime.spawn(async move {
                    let result = crate::map_data::template("transport").await;
                    if let Ok(mut shared) = state.lock() {
                        shared.template_loading = false;
                        match result {
                            Ok(template) => {
                                shared.template = Some(template.into());
                                shared.template_error = None;
                            }
                            Err(error) => {
                                shared.template_failed = Some(Instant::now());
                                shared.template_error = Some(error.to_string());
                            }
                        }
                    }
                    repaint();
                });
            }
            return;
        };
        // Downloads for missing tiles, nearest first.
        for key in &wanted {
            if shared.inflight.len() >= MAX_INFLIGHT {
                break;
            }
            let retry = shared
                .failed
                .get(key)
                .is_none_or(|at| now.duration_since(*at) >= FAILED_RETRY);
            if shared.tiles.contains_key(key) || shared.inflight.contains(key) || !retry {
                continue;
            }
            shared.inflight.insert(*key);
            let (state, template, key, repaint) =
                (self.shared.clone(), template.clone(), *key, repaint.clone());
            runtime.spawn(async move {
                let result = fetch(key, &template).await;
                if let Ok(mut shared) = state.lock() {
                    shared.inflight.remove(&key);
                    match result {
                        Ok(tile) => {
                            shared.failed.remove(&key);
                            shared.tiles.insert(key, (Arc::new(tile), Instant::now()));
                            shared.revision += 1;
                            evict(&mut shared.tiles);
                        }
                        Err(_) => {
                            shared.failed.insert(key, Instant::now());
                        }
                    }
                }
                repaint();
            });
        }
        if self
            .built
            .as_ref()
            .is_some_and(|(keys, revision)| *keys == wanted && *revision == shared.revision)
        {
            return;
        }
        // Each wanted tile, or its nearest loaded ancestor while it loads.
        let mut chosen: Vec<TileKey> = Vec::new();
        for key in &wanted {
            let mut candidate = Some(*key);
            while let Some(at) = candidate {
                if shared.tiles.contains_key(&at) {
                    if !chosen.contains(&at) {
                        chosen.push(at);
                    }
                    break;
                }
                candidate = at.parent().filter(|parent| parent.z >= MIN_ZOOM);
            }
        }
        let tiles = chosen
            .iter()
            .filter_map(|key| {
                let entry = shared.tiles.get_mut(key)?;
                entry.1 = now;
                Some(entry.0.clone())
            })
            .collect();
        self.built = Some((wanted, shared.revision));
        self.snapshot = Some(Arc::new(TransportSnapshot { tiles, zoom }));
    }
}

/// The zoom level and the tiles around the view, nearest first.
fn wanted(lat: f64, lon: f64, altitude_km: f64, tilt_deg: f64) -> (u8, Vec<TileKey>) {
    let mut zoom = ideal_zoom(lat, altitude_km, TILES_ACROSS_VIEW)
        .round()
        .clamp(f64::from(MIN_ZOOM), f64::from(MAX_ZOOM)) as u8;
    let radius = view_radius_deg(altitude_km, tilt_deg);
    loop {
        let tiles = around(zoom, lat, lon, radius);
        if tiles.len() <= MAX_TILES || zoom == MIN_ZOOM {
            return (zoom, tiles.into_iter().take(MAX_TILES * 2).collect());
        }
        zoom -= 1;
    }
}

/// Drops the tiles used longest ago beyond the cache size.
fn evict(tiles: &mut HashMap<TileKey, (Arc<TransportTile>, Instant)>) {
    if tiles.len() <= MAX_CACHED_TILES {
        return;
    }
    let mut ages: Vec<(Instant, TileKey)> =
        tiles.iter().map(|(key, (_, at))| (*at, *key)).collect();
    ages.sort();
    for (_, key) in ages.into_iter().take(tiles.len() - MAX_CACHED_TILES) {
        tiles.remove(&key);
    }
}

async fn fetch(key: TileKey, template: &str) -> Result<TransportTile, crate::Error> {
    let archives = template
        .strip_prefix(crate::map_data::PMTILES_PREFIX)
        .ok_or("transport tiles need GlobeCraft's archives")?
        .to_owned();
    let parts = crate::map_data::tile_parts(key.z, key.x, key.y, &archives).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || {
        let mut bytes = Vec::new();
        for part in parts {
            bytes.extend(crate::map_data::gunzip_if_needed(part)?);
        }
        decode(key, &bytes).ok_or_else(|| {
            crate::Error::from(format!(
                "malformed transport tile {}/{}/{}",
                key.z, key.x, key.y
            ))
        })
    })
    .await
}

/// The lines and airports of a vector tile, in latitude/longitude.
fn decode(key: TileKey, bytes: &[u8]) -> Option<TransportTile> {
    let layers = mvt::decode(bytes, |name| {
        matches!(name, "transportation" | "aeroway" | "aerodrome_label")
    })?;
    let mut tile = TransportTile::default();
    for layer in &layers {
        let extent = f64::from(layer.extent.max(1));
        let to_degrees = |(x, y): (f64, f64)| key.point(x / extent, y / extent);
        let inside = TileBounds {
            south: 0.0,
            north: extent,
            west: 0.0,
            east: extent,
        };
        for feature in &layer.features {
            let text = |tag: &str| {
                layer
                    .tag(feature, tag)
                    .and_then(mvt::Value::as_text)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            match layer.name.as_str() {
                "transportation" | "aeroway" => {
                    let Some(kind) = line_kind(
                        layer.name.as_str(),
                        text("class").as_deref(),
                        text("subclass").as_deref(),
                        text("service").as_deref(),
                    ) else {
                        continue;
                    };
                    if !matches!(feature.kind, GeomType::Line | GeomType::Polygon) {
                        continue;
                    }
                    let tunnel = text("brunnel").as_deref() == Some("tunnel");
                    for part in &feature.parts {
                        // `clip` works on (lat, lon) pairs; tile units are (x, y).
                        let swapped: Vec<(f64, f64)> = part.iter().map(|&(x, y)| (y, x)).collect();
                        for run in clip(&swapped, inside) {
                            if run.len() < 2 {
                                continue;
                            }
                            tile.lines.push(TransportLine {
                                kind,
                                tunnel,
                                points: run.into_iter().map(|(y, x)| to_degrees((x, y))).collect(),
                            });
                        }
                    }
                }
                "aerodrome_label" if feature.kind == GeomType::Point => {
                    let name = text("name_en")
                        .or_else(|| text("name:latin"))
                        .or_else(|| text("name"));
                    for &(x, y) in feature.parts.iter().flatten() {
                        if !(0.0..extent).contains(&x) || !(0.0..extent).contains(&y) {
                            continue;
                        }
                        let (lat, lon) = to_degrees((x, y));
                        let iata = text("iata");
                        let Some(name) = name.clone().or_else(|| iata.clone()) else {
                            continue;
                        };
                        tile.airports.push(Airport {
                            lat,
                            lon,
                            name,
                            iata,
                            icao: text("icao"),
                            class: text("class").unwrap_or_default(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Some(tile)
}

fn line_kind(
    layer: &str,
    class: Option<&str>,
    subclass: Option<&str>,
    service: Option<&str>,
) -> Option<LineKind> {
    Some(match (layer, class?) {
        ("transportation", "rail") => match service {
            Some("yard" | "siding" | "spur" | "crossover") => LineKind::RailService,
            _ => LineKind::Rail,
        },
        ("transportation", "transit") => match subclass {
            Some("subway") => LineKind::Subway,
            _ => LineKind::Tram,
        },
        ("transportation", "ferry") => LineKind::Ferry,
        ("aeroway", "runway") => LineKind::Runway,
        ("aeroway", "taxiway") => LineKind::Taxiway,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mvt::encode::{TestFeature, TestValue, layer, tile};

    #[test]
    fn tiles_yield_rail_ferry_runway_lines_and_airports() {
        let feature = |tags: Vec<(&'static str, TestValue<'static>)>, kind, parts| TestFeature {
            tags,
            kind,
            parts,
        };
        let line = vec![vec![(0, 2048), (4096, 2048)]];
        let bytes = tile(&[
            layer(
                "transportation",
                4096,
                &[
                    feature(vec![("class", TestValue::Text("rail"))], 2, line.clone()),
                    feature(
                        vec![
                            ("class", TestValue::Text("rail")),
                            ("service", TestValue::Text("yard")),
                        ],
                        2,
                        line.clone(),
                    ),
                    feature(
                        vec![
                            ("class", TestValue::Text("transit")),
                            ("subclass", TestValue::Text("subway")),
                            ("brunnel", TestValue::Text("tunnel")),
                        ],
                        2,
                        line.clone(),
                    ),
                    feature(vec![("class", TestValue::Text("ferry"))], 2, line.clone()),
                    feature(
                        vec![("class", TestValue::Text("motorway"))],
                        2,
                        line.clone(),
                    ),
                ],
            ),
            layer(
                "aeroway",
                4096,
                &[feature(
                    vec![("class", TestValue::Text("runway"))],
                    2,
                    line.clone(),
                )],
            ),
            layer(
                "aerodrome_label",
                4096,
                &[
                    feature(
                        vec![
                            ("name", TestValue::Text("Flughafen München")),
                            ("name_en", TestValue::Text("Munich Airport")),
                            ("iata", TestValue::Text("MUC")),
                            ("class", TestValue::Text("international")),
                        ],
                        1,
                        vec![vec![(2048, 2048)]],
                    ),
                    // In the tile's buffer: belongs to the neighbour.
                    feature(
                        vec![("name", TestValue::Text("Elsewhere"))],
                        1,
                        vec![vec![(-20, 2048)]],
                    ),
                ],
            ),
        ]);
        let key = TileKey {
            z: 8,
            x: 136,
            y: 88,
        };
        let tile = decode(key, &bytes).unwrap();
        let kinds: Vec<_> = tile
            .lines
            .iter()
            .map(|line| (line.kind, line.tunnel))
            .collect();
        assert_eq!(
            kinds,
            [
                (LineKind::Rail, false),
                (LineKind::RailService, false),
                (LineKind::Subway, true),
                (LineKind::Ferry, false),
                (LineKind::Runway, false),
            ]
        );
        let bounds = key.bounds();
        for point in &tile.lines[0].points {
            assert!((bounds.south..=bounds.north).contains(&point.0));
            assert!((bounds.west - 1e-9..=bounds.east + 1e-9).contains(&point.1));
        }
        assert_eq!(tile.airports.len(), 1);
        assert_eq!(tile.airports[0].name, "Munich Airport");
        assert_eq!(tile.airports[0].iata.as_deref(), Some("MUC"));
        assert_eq!(tile.airports[0].importance(), 0);
    }

    #[test]
    fn closer_views_want_finer_tiles_up_to_zoom_fourteen() {
        let (far, _) = wanted(48.1, 11.6, 2_500.0, 0.0);
        let (near, tiles) = wanted(48.1, 11.6, 0.5, 0.0);
        assert!((MIN_ZOOM..8).contains(&far), "{far}");
        assert_eq!(near, MAX_ZOOM);
        assert!(!tiles.is_empty() && tiles.len() <= MAX_TILES * 2);
        let (_, tilted) = wanted(48.1, 11.6, 20.0, 70.0);
        assert!(tilted.len() <= MAX_TILES);
    }
}
