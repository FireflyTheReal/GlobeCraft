use globecraft_core::language::Lang;
use std::sync::{Arc, Mutex};
mod adm;
mod brazil;
mod geojson;
mod load;
mod lod;
mod municipal;
mod national;
mod parents;
mod places;
pub use places::PlaceView;
mod sources;
mod states;
mod updates;
mod world;
pub use geojson::dedupe_city_labels;
pub use load::{BoundaryLoader, load_boundaries_async};
pub use municipal::topology::{CountryCells, MAJOR_CELL_DEG, MAJOR_COVER_DEG, MajorCells};
/// The ISO codes of the two countries a land-border line separates.
pub type CountryPair = [[u8; 3]; 2];

pub use municipal::{CountryBorderLines, MunicipalView};
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
    fn expanded(self, margin: f64) -> Self {
        Self {
            min_lat: self.min_lat - margin,
            max_lat: self.max_lat + margin,
            min_lon: self.min_lon - margin,
            max_lon: self.max_lon + margin,
        }
    }
    fn overlaps(self, other: Self) -> bool {
        self.min_lat <= other.max_lat
            && self.max_lat >= other.min_lat
            && self.min_lon <= other.max_lon
            && self.max_lon >= other.min_lon
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
    pub countries: Vec<Line>,
    pub states: Vec<Line>, // admin-1 of "ordinary" countries (shown at medium zoom)
    pub states_big: Vec<Line>, // admin-1 of the largest countries (shown even with the whole Earth in view)
    pub cities: Vec<Line>,
    /// Interior municipal lines (and coasts), drawn once zoomed in far enough.
    pub world_cities: Vec<Line>,
    /// Municipal segments on state borders, drawn from the municipal altitude down.
    pub world_cities_major: Vec<Line>,
    /// Below this altitude `world_cities` and the small municipal names appear.
    pub municipal_detail_max_km: f64,
    /// Where `world_cities_major` replaces coarser lines; see [`MajorCells`].
    pub municipal_major_cells: Arc<MajorCells>,
    /// Municipal country borders, drawn by one side per border (see
    /// `municipal::borders`) in place of the Natural Earth country line they cover.
    pub world_cities_country: Vec<Line>,
    /// Where `world_cities_country` replaces the Natural Earth country lines.
    pub municipal_country_cells: Arc<CountryCells>,
    /// For each line of `countries`, the two countries it separates (Natural Earth's
    /// `ADM0_A3_L`/`ADM0_A3_R`), when known.
    pub country_pairs: Vec<Option<CountryPair>>,
    /// For each line of `countries`, whether it is disputed (Natural Earth's feature
    /// class; drawn in the disputed style).
    pub country_disputed: Vec<bool>,
    /// Where disputed lines run: municipal country borders give way to them there, so
    /// a disputed border is never drawn twice.
    pub disputed_cells: Arc<MajorCells>,
    pub labels: Vec<MapLabel>,
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
const BIG_COUNTRIES_NAME: [&str; 10] = [
    "Russia",
    "Canada",
    "United States of America",
    "China",
    "Brazil",
    "Australia",
    "India",
    "Argentina",
    "Kazakhstan",
    "Algeria",
];
/// Below this camera altitude (km) state lines and labels appear; the app fades
/// them in from here to `STATE_FADE_FULL_ALTITUDE_KM` (about 1000 → 500 mi).
pub const STATE_VISIBLE_MAX_ALTITUDE_KM: f64 = 1_609.0;
/// Provinces of the big countries (Brazil, China, India, USA, Canada, ...)
/// cover so much screen area that they read well much earlier; they appear
/// below this altitude instead (about 2,500 mi) and fade in to the same
/// altitude the smaller countries' provinces use.
pub const STATE_BIG_VISIBLE_MAX_ALTITUDE_KM: f64 = 4_000.0;
/// Below this camera altitude (km) municipal outlines and their labels appear
/// (about 250 mi). At this altitude the municipal state borders replace the coarse
/// state lines they trace in one frame, with no cross-fade; both draw with the same
/// color and dash, so the swap reads as one continuous linework.
pub const MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM: f64 = 402.0;
/// Interior municipal lines fade in from `BoundaryData::municipal_detail_max_km` to this
/// fraction of it; small municipal names appear at the same fraction.
pub const MUNICIPAL_DETAIL_FADE_FULL_RATIO: f64 = 0.8;

/// Municipal labels from live/national boundary sources; base populated places
/// rank 0-8. The biggest municipalities (by area) rank 9 so their names arrive
/// together with the outlines at the handoff start; the rest rank 10 and only
/// appear once the handoff completes.
pub(super) const LOCAL_BIG_LABEL_RANK: u8 = 9;
pub(super) const LOCAL_LABEL_RANK: u8 = 10;
/// Municipalities spanning at least this many square kilometres count as big.
const MUNICIPAL_BIG_AREA_KM2: f64 = 1_000.0;

/// Label rank for a municipal feature: big by area, else the common rank.
/// Takes the property/attribute object directly so both GeoJSON features and
/// Esri JSON attributes work.
pub(super) fn municipal_label_rank(props: Option<&serde_json::Value>) -> u8 {
    let Some(props) = props else {
        return LOCAL_LABEL_RANK;
    };
    for (key, scale) in [
        ("AREA_KM2", 1.0),
        ("area_km2", 1.0),
        ("Shape_Area", 1e-6),
        ("SHAPE_Area", 1e-6),
    ] {
        if let Some(area) = props
            .get(key)
            .and_then(|v| v.as_f64())
            .filter(|area| area.is_finite() && *area > 0.0)
        {
            return if area * scale >= MUNICIPAL_BIG_AREA_KM2 {
                LOCAL_BIG_LABEL_RANK
            } else {
                LOCAL_LABEL_RANK
            };
        }
    }
    LOCAL_LABEL_RANK
}
const CITY_RANK_0_1_MAX_ALTITUDE_KM: f64 = 9000.0;
const CITY_RANK_2_3_MAX_ALTITUDE_KM: f64 = 4000.0;
const CITY_RANK_4_5_MAX_ALTITUDE_KM: f64 = 2000.0;
const CITY_RANK_6_7_MAX_ALTITUDE_KM: f64 = 900.0;
const OTHER_CITY_MAX_ALTITUDE_KM: f64 = 300.0;
const PLACE_TOWN_MAX_ALTITUDE_KM: f64 = 150.0;
const PLACE_VILLAGE_MAX_ALTITUDE_KM: f64 = 50.0;
const PLACE_HAMLET_MAX_ALTITUDE_KM: f64 = 20.0;

pub(super) fn is_big_country(feature: &serde_json::Value) -> bool {
    let Some(props) = feature.get("properties") else {
        return false;
    };
    for k in ["adm0_a3", "ADM0_A3", "sov_a3", "SOV_A3"] {
        if let Some(a3) = props.get(k).and_then(|v| v.as_str()) {
            return BIG_COUNTRIES_A3.contains(&a3);
        }
    }
    for k in ["admin", "ADMIN", "geonunit"] {
        if let Some(n) = props.get(k).and_then(|v| v.as_str()) {
            return BIG_COUNTRIES_NAME.contains(&n);
        }
    }
    false
}

#[derive(Clone)]
pub struct MapLabel {
    pub lat: f64,
    pub lon: f64,
    pub en: String,
    pub pt: String,
    pub kind: LabelKind,
    pub rank: u8,
    pub big: bool,
}
impl MapLabel {
    pub fn text(&self, lg: Lang) -> &str {
        match lg {
            Lang::En => &self.en,
            Lang::PtBr => &self.pt,
        }
    }
    /// Whether the label should be shown at the given camera altitude (km), with
    /// municipal names staged at the default altitude.
    pub fn visible_at(&self, alt: f64) -> bool {
        self.visible_with_detail(alt, MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM)
    }
    /// Whether this is a municipality's name from the municipal layer (rather than
    /// one of the populated places shown with place names).
    pub fn is_municipal(&self) -> bool {
        self.kind == LabelKind::City
            && (LOCAL_BIG_LABEL_RANK..=LOCAL_LABEL_RANK).contains(&self.rank)
    }
    /// Whether this is an OpenStreetMap place name (see `places`).
    pub fn is_place(&self) -> bool {
        self.kind == LabelKind::City && self.rank >= places::PLACE_TOWN_RANK
    }
    /// Whether the label should be shown at the given camera altitude (km).
    /// Bigger places (lower rank) appear first, smaller ones only when zoomed in.
    /// Municipal names wait for `municipal_detail_max_km`
    /// (`BoundaryData::municipal_detail_max_km`), which follows their size.
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
                    8 => OTHER_CITY_MAX_ALTITUDE_KM,
                    // Municipal labels from local sources: big ones (rank 9)
                    // arrive with the interior lines; small ones once they are full.
                    LOCAL_BIG_LABEL_RANK => municipal_detail_max_km,
                    LOCAL_LABEL_RANK => municipal_detail_max_km * MUNICIPAL_DETAIL_FADE_FULL_RATIO,
                    // OpenStreetMap places: towns, then villages, then hamlets.
                    places::PLACE_TOWN_RANK => PLACE_TOWN_MAX_ALTITUDE_KM,
                    places::PLACE_VILLAGE_RANK => PLACE_VILLAGE_MAX_ALTITUDE_KM,
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
    Countries,
    States,
    Cities,
    WorldCities,
    /// OpenStreetMap town, village and island names around the view.
    Places,
}

/// One finished piece of overlay data, sent from a loader thread to the UI thread.
pub struct Patch {
    pub layer: Layer,
    /// For `Layer::Countries`: the two countries each of `lines` separates, when known
    /// (shorter than `lines` when the tail has none).
    pub pairs: Vec<Option<CountryPair>>,
    /// For `Layer::Countries`: whether each of `lines` is disputed (shorter than `lines`
    /// when the tail is not).
    pub disputed: Vec<bool>,
    pub lines: Vec<Line>,
    pub lines_big: Vec<Line>,
    pub labels: Vec<MapLabel>,
    pub(crate) regions: Vec<CountryRegion>,
    /// Replace the layer's current content instead of appending to it.
    /// A coarser detail level is upgraded to a finer one through this flag, so
    /// the two sets never accumulate.
    pub replace: bool,
    /// For `Layer::WorldCities`: the staging of the municipal layer.
    pub municipal: Option<MunicipalStaging>,
}

/// How the municipal layer is staged, sent with each `WorldCities` patch.
#[derive(Clone, Debug)]
pub struct MunicipalStaging {
    /// Below this altitude interior municipal lines and small names appear (set
    /// from the municipalities' size).
    pub detail_max_km: f64,
    /// Where municipal state and country borders run, so the coarser Natural
    /// Earth state lines can hand over to them.
    pub major_cells: Arc<MajorCells>,
    /// Municipal country borders.
    pub country_lines: Vec<Line>,
    /// The border each of `country_lines` draws, when known.
    pub country_pairs: Vec<Option<CountryPair>>,
    /// Where `country_lines` run, so the Natural Earth country lines hand over too.
    pub country_cells: Arc<CountryCells>,
}

#[derive(Default)]
pub(super) struct Detail {
    pub(super) lines: Vec<Line>,
    /// The state (or other parent unit) of each line, as an opaque code, when the
    /// source names it. Shorter than `lines` when unknown.
    pub(super) parents: Vec<Option<u64>>,
    pub(super) labels: Vec<MapLabel>,
    pub(super) features: usize,
    /// Neighbouring lines of this detail share identical vertices (geoBoundaries files
    /// are simplified with shared topology), so they match at a tight tolerance. Sources
    /// that simplify each polygon on their own leave this off.
    pub(super) exact_topology: bool,
}

/// Human-readable load log shown in the Layers panel, so failures are never silent.
pub type StatusLog = Arc<Mutex<Vec<String>>>;

pub(super) fn log_status(log: &StatusLog, msg: String) {
    if let Ok(mut v) = log.lock() {
        v.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn big_countries() {
        let f = serde_json::json!({"properties": {"adm0_a3": "BRA", "admin": "Brazil"}});
        assert!(is_big_country(&f));
        let f = serde_json::json!({"properties": {"adm0_a3": "FRA", "admin": "France"}});
        assert!(!is_big_country(&f));
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
