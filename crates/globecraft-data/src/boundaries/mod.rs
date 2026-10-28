//! Borders and place names.
//!
//! Borders and most names come from OpenStreetMap vector tiles (`osm_tiles`), streamed
//! like imagery. Natural Earth's 1:110m countries, land borders and populated places are
//! bundled: they answer "which country is this point in", name the countries, and are
//! drawn until the first border tile is loaded (offline starts, a failed service).
use globecraft_core::language::Lang;
use std::sync::{Arc, Mutex};
mod geojson;
mod load;
pub(crate) mod osm_tiles;
mod sources;
mod updates;
pub use geojson::dedupe_city_labels;
pub use load::{BoundaryLoader, load_boundaries_async};
pub use osm_tiles::{MUNICIPAL_MIN_ZOOM as OSM_MUNICIPAL_MIN_ZOOM, OsmBorderLines, OsmTileView};
pub use updates::{BoundarySnapshot, BoundaryUpdates};
pub type Line = Arc<Vec<(f64, f64)>>;

#[derive(Clone, Copy)]
pub(crate) struct GeoBounds {
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
}
impl GeoBounds {
    fn of(line: &[(f64, f64)]) -> Option<Self> {
        let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);
        let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
        for &(lat, lon) in line {
            if !lat.is_finite() || !lon.is_finite() {
                continue;
            }
            min_lat = min_lat.min(lat);
            max_lat = max_lat.max(lat);
            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
        }
        (min_lat <= max_lat).then_some(Self {
            min_lat,
            max_lat,
            min_lon,
            max_lon,
        })
    }
    fn union(self, other: Self) -> Self {
        Self {
            min_lat: self.min_lat.min(other.min_lat),
            max_lat: self.max_lat.max(other.max_lat),
            min_lon: self.min_lon.min(other.min_lon),
            max_lon: self.max_lon.max(other.max_lon),
        }
    }
    fn contains(self, lat: f64, lon: f64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat && lon >= self.min_lon && lon <= self.max_lon
    }
}

/// One outer ring of a country, retained only to identify the country under the camera.
#[derive(Clone)]
pub(crate) struct CountryRing {
    line: Line,
    bounds: GeoBounds,
}
impl CountryRing {
    pub(super) fn new(line: Line) -> Option<Self> {
        let bounds = GeoBounds::of(&line)?;
        Some(Self { line, bounds })
    }
    fn contains(&self, lat: f64, lon: f64) -> bool {
        if !self.bounds.contains(lat, lon) {
            return false;
        }
        let points = self.line.as_slice();
        let mut inside = false;
        for i in 0..points.len() {
            let (lat_a, lon_a) = points[i];
            let (lat_b, lon_b) = points[(i + 1) % points.len()];
            if (lat_a > lat) != (lat_b > lat)
                && lon < (lon_b - lon_a) * (lat - lat_a) / (lat_b - lat_a) + lon_a
            {
                inside = !inside;
            }
        }
        inside
    }
}

#[derive(Clone)]
pub(crate) struct CountryRegion {
    iso3: String,
    rings: Vec<CountryRing>,
    bounds: GeoBounds,
}
impl CountryRegion {
    pub(super) fn new(iso3: String, rings: Vec<CountryRing>) -> Option<Self> {
        let bounds = rings
            .iter()
            .map(|ring| ring.bounds)
            .reduce(GeoBounds::union)?;
        Some(Self {
            iso3,
            rings,
            bounds,
        })
    }
    fn contains(&self, lat: f64, lon: f64) -> bool {
        self.bounds.contains(lat, lon) && self.rings.iter().any(|ring| ring.contains(lat, lon))
    }
}

#[derive(Clone, Default)]
pub struct BoundaryData {
    /// Natural Earth's 1:110m land borders (bundled), drawn until border tiles arrive.
    pub countries: Vec<Line>,
    pub labels: Vec<MapLabel>,
    /// Borders from the OpenStreetMap tiles around the view.
    pub osm_borders: Arc<OsmBorderLines>,
    /// Below this altitude municipal lines and village names appear (where the tiles
    /// start to hold municipal lines, at the view's latitude).
    pub municipal_detail_max_km: f64,
    country_regions: Vec<CountryRegion>,
    countries_loaded: bool,
}
impl BoundaryData {
    /// ISO3 of the country containing the point, from Natural Earth outer rings.
    pub fn country_at(&self, lat: f64, lon: f64) -> Option<&str> {
        self.country_regions
            .iter()
            .find(|region| region.contains(lat, lon))
            .map(|region| region.iso3.as_str())
    }
    /// Geographic bounds of the country containing the point, so a frontend can
    /// gate a country's name by its apparent size on screen.
    pub fn country_bounds_at(&self, lat: f64, lon: f64) -> Option<CountryBounds> {
        let region = self
            .country_regions
            .iter()
            .find(|region| region.contains(lat, lon))?;
        Some(CountryBounds {
            min_lat: region.bounds.min_lat,
            min_lon: region.bounds.min_lon,
            max_lat: region.bounds.max_lat,
            max_lon: region.bounds.max_lon,
        })
    }
    pub fn has_countries(&self) -> bool {
        self.countries_loaded
    }
    /// Whether the country at the point has its provinces shown from far out.
    fn in_big_country(&self, lat: f64, lon: f64) -> bool {
        self.country_at(lat, lon)
            .is_some_and(|iso3| BIG_COUNTRIES_A3.contains(&iso3))
    }
}

/// Latitude/longitude bounding box of one country's retained outer rings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CountryBounds {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

/// States/provinces of these countries (the 10 largest by area) are visible when zoomed far out.
const BIG_COUNTRIES_A3: [&str; 10] = [
    "RUS", "CAN", "USA", "CHN", "BRA", "AUS", "IND", "ARG", "KAZ", "DZA",
];
/// Below this camera altitude (km) state lines and labels appear; the app fades
/// them in from here to `STATE_FADE_FULL_ALTITUDE_KM` (about 1000 → 500 mi).
pub const STATE_VISIBLE_MAX_ALTITUDE_KM: f64 = 1_609.0;
/// Provinces of the big countries (Brazil, China, India, USA, Canada, ...)
/// cover so much screen area that they read well much earlier; they appear
/// below this altitude instead (about 2,500 mi) and fade in to the same
/// altitude the smaller countries' provinces use.
pub const STATE_BIG_VISIBLE_MAX_ALTITUDE_KM: f64 = 4_000.0;
/// Municipal lines never appear above this altitude (km), whatever the tiles hold.
pub const MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM: f64 = 402.0;
/// Interior municipal lines fade in from `BoundaryData::municipal_detail_max_km` to this
/// fraction of it; small municipal names appear at the same fraction.
pub const MUNICIPAL_DETAIL_FADE_FULL_RATIO: f64 = 0.8;

const CITY_RANK_0_1_MAX_ALTITUDE_KM: f64 = 9000.0;
const CITY_RANK_2_3_MAX_ALTITUDE_KM: f64 = 4000.0;
const CITY_RANK_4_5_MAX_ALTITUDE_KM: f64 = 2000.0;
const CITY_RANK_6_7_MAX_ALTITUDE_KM: f64 = 900.0;
const OTHER_CITY_MAX_ALTITUDE_KM: f64 = 300.0;
const PLACE_TOWN_MAX_ALTITUDE_KM: f64 = 150.0;
const PLACE_VILLAGE_MAX_ALTITUDE_KM: f64 = 50.0;
const PLACE_HAMLET_MAX_ALTITUDE_KM: f64 = 20.0;

/// Label ranks of OpenStreetMap places: towns, villages, hamlets. Cities use the
/// populated-place ranks 0–8 (from the tiles' own importance rank).
pub const PLACE_TOWN_RANK: u8 = 11;
pub(crate) const PLACE_VILLAGE_RANK: u8 = 12;
pub(crate) const PLACE_HAMLET_RANK: u8 = 13;

#[derive(Clone)]
pub struct MapLabel {
    pub lat: f64,
    pub lon: f64,
    pub en: String,
    pub pt: String,
    pub kind: LabelKind,
    pub rank: u8,
    pub big: bool,
    /// From the border tiles on show: replaced whenever they change. The bundled
    /// country and place names stay.
    pub from_tiles: bool,
}
impl MapLabel {
    pub fn text(&self, lg: Lang) -> &str {
        match lg {
            Lang::En => &self.en,
            Lang::PtBr => &self.pt,
        }
    }
    /// Whether the label should be shown at the given camera altitude (km), with
    /// village names staged at the default municipal altitude.
    pub fn visible_at(&self, alt: f64) -> bool {
        self.visible_with_detail(alt, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM)
    }
    /// Whether the label should be shown at the given camera altitude (km).
    /// Bigger places (lower rank) appear first, smaller ones only when zoomed in.
    /// Village names wait for `municipal_detail_max_km`
    /// (`BoundaryData::municipal_detail_max_km`), where the municipal lines appear.
    pub fn visible_with_detail(&self, alt: f64, municipal_detail_max_km: f64) -> bool {
        match self.kind {
            LabelKind::Country => true,
            LabelKind::State => {
                if self.big {
                    alt < STATE_BIG_VISIBLE_MAX_ALTITUDE_KM
                } else {
                    alt < STATE_VISIBLE_MAX_ALTITUDE_KM
                }
            }
            LabelKind::City => {
                alt < match self.rank {
                    0..=1 => CITY_RANK_0_1_MAX_ALTITUDE_KM,
                    2..=3 => CITY_RANK_2_3_MAX_ALTITUDE_KM,
                    4..=5 => CITY_RANK_4_5_MAX_ALTITUDE_KM,
                    6..=7 => CITY_RANK_6_7_MAX_ALTITUDE_KM,
                    8..PLACE_TOWN_RANK => OTHER_CITY_MAX_ALTITUDE_KM,
                    PLACE_TOWN_RANK => PLACE_TOWN_MAX_ALTITUDE_KM,
                    // Villages appear once the municipal lines have faded in.
                    PLACE_VILLAGE_RANK if municipal_detail_max_km > 0.0 => {
                        PLACE_VILLAGE_MAX_ALTITUDE_KM
                            .min(municipal_detail_max_km * MUNICIPAL_DETAIL_FADE_FULL_RATIO)
                    }
                    PLACE_VILLAGE_RANK => PLACE_VILLAGE_MAX_ALTITUDE_KM,
                    _ => PLACE_HAMLET_MAX_ALTITUDE_KM,
                }
            }
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
pub enum LabelKind {
    Country,
    State,
    City,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Layer {
    /// Natural Earth's bundled countries (names, lookup outlines, fallback borders).
    Countries,
    /// Natural Earth's bundled populated places.
    Cities,
    /// Names from the border tiles on show (replaces the previous ones).
    Places,
    /// Borders from the border tiles on show.
    OsmBorders,
}

/// One finished piece of overlay data, sent from a loader thread to the UI thread.
pub struct Patch {
    pub layer: Layer,
    pub lines: Vec<Line>,
    pub labels: Vec<MapLabel>,
    pub(crate) regions: Vec<CountryRegion>,
    /// For `Layer::OsmBorders`: the border lines of the tiles on show.
    pub osm: Option<OsmBorderLines>,
}

/// Human-readable load log shown in the Layers panel, so failures are never silent.
pub type StatusLog = Arc<Mutex<Vec<String>>>;

/// Lines kept in the status log; older ones are dropped (an offline session logs one
/// line per failed tile).
const STATUS_LOG_LINES: usize = 64;

pub(super) fn log_status(log: &StatusLog, msg: String) {
    if let Ok(mut v) = log.lock() {
        if v.len() >= STATUS_LOG_LINES {
            let excess = v.len() + 1 - STATUS_LOG_LINES;
            v.drain(..excess);
        }
        v.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_status_log_keeps_only_its_latest_lines() {
        let log: StatusLog = Arc::default();
        for i in 0..200 {
            log_status(&log, i.to_string());
        }
        let lines = log.lock().unwrap();
        assert_eq!(lines.len(), STATUS_LOG_LINES);
        assert_eq!(lines.last().map(String::as_str), Some("199"));
    }
    #[test]
    fn country_lookup_uses_the_outer_ring_not_its_label_centre() {
        let ring = CountryRing::new(Arc::new(vec![
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]))
        .unwrap();
        let region = CountryRegion::new("TST".into(), vec![ring]).unwrap();
        let data = BoundaryData {
            country_regions: vec![region],
            ..Default::default()
        };
        assert_eq!(data.country_at(5.0, 5.0), Some("TST"));
        assert_eq!(data.country_at(15.0, 5.0), None);
    }
}
