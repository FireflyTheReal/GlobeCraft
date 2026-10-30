//! Railways, trams and subways, ferries and airports from GlobeCraft's own `transport`
//! tile set (OpenMapTiles `transportation`, `aeroway` and `aerodrome_label`, cut from
//! the OpenFreeMap planet to zoom 14; `crate::map_data`). Streamed like the border
//! tiles: a zoom level for the altitude, the tiles around the view, nearest first, and
//! coarser loaded tiles drawn while finer ones arrive.
use crate::boundaries::osm_tiles::content::{TileBounds, TileKey, clip};
use crate::mvt::{self, GeomType};
use crate::vector_tiles::{Snapshot, Spec, TileStream, View};
use std::sync::Arc;

/// Above this the layers are hidden (ferries show up to here).
pub const MAX_ALTITUDE_KM: f64 = 3_000.0;
/// Railways and airports exist from `DETAIL_FLOOR_ZOOM`: while they are on, views up
/// to this altitude take tiles at least that fine (zoom 8 tiles average under 1 KiB).
pub const DETAIL_FLOOR_MAX_ALTITUDE_KM: f64 = 1_000.0;
const DETAIL_FLOOR_ZOOM: u8 = 8;

/// Measured on the published tiles (2026-10-28): ferries from zoom 5, railways and
/// airport names from zoom 8, runways from zoom 10; trams, subways and sidings only
/// at zoom 14, and zoom 13 holds nothing zoom 14 lacks.
static SPEC: Spec<TransportTile> = Spec {
    set: "transport",
    bands: &[],
    min_zoom: 5,
    max_zoom: 14,
    skip_zoom: Some(13),
    // Finer than borders: rail lines are drawn at every level.
    tiles_across_view: 3.0,
    max_tiles: 64,
    max_floor_tiles: 200,
    max_cached_tiles: 600,
    max_cached_bytes: crate::vector_tiles::MAX_VECTOR_CACHE_BYTES,
    size_bytes,
    decode,
};

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

/// Estimated heap size of a decoded transport tile, for the stream's byte budget.
fn size_bytes(tile: &TransportTile) -> usize {
    let lines: usize = tile
        .lines
        .iter()
        .map(|line| {
            std::mem::size_of::<TransportLine>()
                + line.points.capacity() * std::mem::size_of::<(f64, f64)>()
        })
        .sum();
    let airports: usize = tile
        .airports
        .iter()
        .map(|airport| {
            std::mem::size_of::<Airport>()
                + airport.name.capacity()
                + airport.class.capacity()
                + airport.iata.as_ref().map_or(0, String::capacity)
                + airport.icao.as_ref().map_or(0, String::capacity)
        })
        .sum();
    std::mem::size_of::<TransportTile>()
        + tile.lines.capacity() * std::mem::size_of::<TransportLine>()
        + tile.airports.capacity() * std::mem::size_of::<Airport>()
        + lines
        + airports
}

/// The tiles to draw for the current view.
pub type TransportSnapshot = Snapshot<TransportTile>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportQuery {
    pub enabled: bool,
    /// Railways or airports are on (they need zoom 8 tiles; ferries do not).
    pub rail_or_airports: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_km: f64,
    pub tilt_deg: f64,
}

/// The transport tile layer: call [`Self::update`] every frame it is shown.
pub struct TransportTiles {
    stream: TileStream<TransportTile>,
}

impl Default for TransportTiles {
    fn default() -> Self {
        Self {
            stream: TileStream::new(&SPEC),
        }
    }
}

impl TransportTiles {
    /// The tiles to draw, when the layer is on and something is loaded.
    pub fn shared_snapshot(&self) -> Option<Arc<TransportSnapshot>> {
        self.stream.shared_snapshot()
    }

    /// Why nothing shows yet, if something is wrong.
    pub fn error(&self) -> Option<String> {
        self.stream.error()
    }

    /// Whether downloads are running.
    pub fn loading(&self) -> bool {
        self.stream.loading()
    }

    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        query: TransportQuery,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        if !query.enabled || query.altitude_km > MAX_ALTITUDE_KM {
            self.stream.hide();
            return;
        }
        let floor = if query.rail_or_airports && query.altitude_km <= DETAIL_FLOOR_MAX_ALTITUDE_KM {
            DETAIL_FLOOR_ZOOM
        } else {
            SPEC.min_zoom
        };
        let view = View {
            latitude: query.latitude,
            longitude: query.longitude,
            altitude_km: query.altitude_km,
            tilt_deg: query.tilt_deg,
            floor,
        };
        self.stream.update(runtime, view, repaint);
    }
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

    fn wanted(
        lat: f64,
        lon: f64,
        altitude_km: f64,
        tilt_deg: f64,
        floor: u8,
    ) -> (u8, Vec<TileKey>) {
        crate::vector_tiles::wanted(
            &SPEC,
            View {
                latitude: lat,
                longitude: lon,
                altitude_km,
                tilt_deg,
                floor,
            },
        )
    }
    const MIN_ZOOM: u8 = 5;
    const MAX_ZOOM: u8 = 14;
    const MAX_TILES: usize = 64;
    const MAX_FLOOR_TILES: usize = 200;

    #[test]
    fn closer_views_want_finer_tiles_up_to_zoom_fourteen() {
        let (far, _) = wanted(48.1, 11.6, 2_500.0, 0.0, MIN_ZOOM);
        let (near, tiles) = wanted(48.1, 11.6, 0.5, 0.0, MIN_ZOOM);
        assert!((MIN_ZOOM..8).contains(&far), "{far}");
        assert_eq!(near, MAX_ZOOM);
        assert!(!tiles.is_empty() && tiles.len() <= MAX_TILES);
        let (_, tilted) = wanted(48.1, 11.6, 20.0, 70.0, MIN_ZOOM);
        assert!(tilted.len() <= MAX_TILES);
        // Railways from 600 km: zoom 8 tiles, where the rail lines start.
        let (floor, tiles) = wanted(48.1, 11.6, 600.0, 0.0, DETAIL_FLOOR_ZOOM);
        assert_eq!(floor, DETAIL_FLOOR_ZOOM);
        assert!(tiles.len() <= MAX_FLOOR_TILES);
        // Never zoom 13.
        for altitude in [1.0, 2.0, 3.0, 5.0, 8.0] {
            assert_ne!(
                wanted(48.1, 11.6, altitude, 0.0, MIN_ZOOM).0,
                13,
                "{altitude}"
            );
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::collections::HashMap;

    /// The published transport archive, read as the app reads it, and what each level
    /// holds around Munich (rail, S-Bahn/U-Bahn, tram, airport MUC) and the Strait of
    /// Dover (ferries).
    #[tokio::test]
    #[ignore = "network: reads GlobeCraft's published transport tiles"]
    async fn published_transport_tiles_hold_rail_ferries_and_airports() {
        let template = crate::map_data::template("transport").await.unwrap();
        let mut found = HashMap::new();
        for (place, lat, lon) in [("Munich", 48.14, 11.56), ("Dover", 51.05, 1.6)] {
            for z in 5..=14 {
                let key = TileKey::at(z, lat, lon);
                let mut tiles = crate::vector_tiles::fetch_many(&SPEC, &[key], &template)
                    .await
                    .unwrap();
                let tile = tiles.pop().unwrap().1.unwrap();
                let mut kinds: HashMap<LineKind, usize> = HashMap::new();
                for line in &tile.lines {
                    *kinds.entry(line.kind).or_default() += 1;
                }
                let mut kinds: Vec<_> = kinds.into_iter().collect();
                kinds.sort_by_key(|(kind, _)| format!("{kind:?}"));
                let airports: Vec<_> = tile
                    .airports
                    .iter()
                    .map(|airport| format!("{} {:?} {}", airport.name, airport.iata, airport.class))
                    .collect();
                println!("{place} z{z}: {kinds:?} airports {airports:?}");
                for (kind, count) in kinds {
                    *found.entry((place, kind)).or_insert(0) += count;
                }
            }
        }
        assert!(found.contains_key(&("Munich", LineKind::Rail)));
        assert!(found.contains_key(&("Munich", LineKind::Subway)));
        assert!(found.contains_key(&("Dover", LineKind::Ferry)));
    }
}
