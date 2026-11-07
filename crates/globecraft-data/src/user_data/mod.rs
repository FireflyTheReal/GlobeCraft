//! Data the user adds (roadmap 2.1–2.4): web maps (XYZ or WMTS REST tiles, WMS GetMap
//! by tile), and features (GeoJSON, GPX and CSV with coordinates) from a file on this
//! computer or a URL, refreshed at an interval when it is a live feed. Each becomes a
//! user layer with its own visibility, opacity and place in the order, saved in the
//! settings. KML and KMZ keep their own path: they open into My Places.
mod capabilities;
mod legend;
pub mod presets;
mod properties;
mod raster;
mod saved;
mod vector;

pub use capabilities::{WmsLayer, wms_layers};
pub use legend::{legend_image, wms_legend_url};
pub use properties::format_time_utc;
pub use raster::{RasterSnapshot, RasterTiles, test_tile};
pub use saved::{SavedSources, export_sources, import_sources};
pub use vector::{FeatureSet, NamedPoint, VectorLayer, load as load_features, parse_features};

use serde::{Deserialize, Serialize};

/// The file formats features are read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureFormat {
    GeoJson,
    Gpx,
    Csv,
}

impl FeatureFormat {
    pub const ALL: [Self; 3] = [Self::GeoJson, Self::Gpx, Self::Csv];
    pub fn code(self) -> &'static str {
        match self {
            Self::GeoJson => "geojson",
            Self::Gpx => "gpx",
            Self::Csv => "csv",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.code() == code)
    }
    /// The format a file name or URL path suggests.
    pub fn guess(location: &str) -> Option<Self> {
        let path = location.split(['?', '#']).next().unwrap_or(location);
        let extension = path.rsplit('.').next()?.to_ascii_lowercase();
        match extension.as_str() {
            "geojson" | "json" => Some(Self::GeoJson),
            "gpx" => Some(Self::Gpx),
            "csv" | "tsv" | "txt" => Some(Self::Csv),
            _ => None,
        }
    }
}

/// Where a user layer's data comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserSource {
    /// Map tiles in Web Mercator: a URL template with `{z}`, `{x}` and `{y}` (or `{-y}`
    /// for TMS), `{bbox}` for a WMS GetMap by tile, and `{date}` for the day before the app's time.
    Tiles {
        template: String,
        #[serde(default)]
        min_zoom: u8,
        #[serde(default = "default_max_zoom")]
        max_zoom: u8,
        #[serde(default = "default_tile_px")]
        tile_px: u16,
    },
    /// Points, lines and outlines from a file path or URL.
    Features {
        location: String,
        format: FeatureFormat,
        /// Reloaded this often (minutes) when it is a URL; 0 loads it once.
        #[serde(default)]
        refresh_minutes: u32,
        /// The property the points are sized and ranked by; `None` looks for the
        /// usual ones (`mag`, `population`…).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_field: Option<String>,
    },
}

fn default_max_zoom() -> u8 {
    18
}

fn default_tile_px() -> u16 {
    256
}

/// One layer the user added.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserLayer {
    /// Stable for the layer's life, unique among the user's layers.
    pub id: u64,
    pub name: String,
    pub source: UserSource,
    #[serde(default = "visible")]
    pub visible: bool,
    #[serde(default = "full_opacity")]
    pub opacity: f32,
    /// Credit the data's licence asks for, shown with the layer.
    #[serde(default)]
    pub attribution: String,
    /// Colour of points and lines, `#rrggbb` (features only).
    #[serde(default = "default_color")]
    pub color: String,
    /// The library entry it came from, if any.
    #[serde(default)]
    pub preset: Option<String>,
}

fn visible() -> bool {
    true
}

fn full_opacity() -> f32 {
    1.0
}

fn default_color() -> String {
    "#ffb000".into()
}

impl UserLayer {
    /// The colour of its points and lines.
    pub fn rgb(&self) -> [u8; 3] {
        crate::settings::map_style::Rgba::parse_hex(&self.color)
            .map_or([255, 176, 0], |c| [c.0[0], c.0[1], c.0[2]])
    }
    /// The host its data comes from; `None` for a file on this computer.
    pub fn network_host(&self) -> Option<&str> {
        let location = match &self.source {
            UserSource::Tiles { template, .. } => template,
            UserSource::Features { location, .. } => location,
        };
        crate::network::host_of_url(location)
    }
}

/// Reads the saved list, keeping every entry that still parses.
pub fn layers_from_json(value: &serde_json::Value) -> Vec<UserLayer> {
    let mut layers: Vec<UserLayer> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| serde_json::from_value(entry.clone()).ok())
        .collect();
    let mut seen = std::collections::HashSet::new();
    layers.retain(|layer| seen.insert(layer.id));
    for layer in &mut layers {
        layer.opacity = layer.opacity.clamp(0.0, 1.0);
    }
    layers
}

/// A WMS GetMap request for one Web Mercator tile, as a tile template: `base` is the
/// service address (with or without its own query), `layers` comma-separated names.
pub fn wms_template(base: &str, layers: &str) -> String {
    let joiner = if base.contains('?') {
        if base.ends_with('?') || base.ends_with('&') {
            ""
        } else {
            "&"
        }
    } else {
        "?"
    };
    format!(
        "{base}{joiner}SERVICE=WMS&REQUEST=GetMap&VERSION=1.3.0&LAYERS={layers}&STYLES=\
         &CRS=EPSG:3857&BBOX={{bbox}}&WIDTH=256&HEIGHT=256&FORMAT=image/png&TRANSPARENT=TRUE"
    )
}

/// The address of tile `z/x/y` of a template (see [`UserSource::Tiles`]), with
/// `{date}` standing for `date` (`YYYY-MM-DD`).
pub fn tile_url(template: &str, z: u8, x: u32, y: u32, date: &str) -> String {
    let mut url = template
        .replace("{z}", &z.to_string())
        .replace("{x}", &x.to_string())
        .replace("{-y}", &((1u32 << z) - 1 - y).to_string())
        .replace("{y}", &y.to_string());
    if url.contains("{bbox}") {
        let [west, south, east, north] = mercator_bounds(z, x, y);
        url = url.replace(
            "{bbox}",
            &format!("{west:.2},{south:.2},{east:.2},{north:.2}"),
        );
    }
    if url.contains("{date}") {
        url = url.replace("{date}", date);
    }
    url
}

/// Web Mercator metres of tile `z/x/y`: west, south, east, north.
fn mercator_bounds(z: u8, x: u32, y: u32) -> [f64; 4] {
    const HALF: f64 = 20_037_508.342_789_244;
    let size = 2.0 * HALF / f64::from(1u32 << z);
    let west = -HALF + f64::from(x) * size;
    let north = HALF - f64::from(y) * size;
    [west, north - size, west + size, north]
}

/// The day before `unix` (UTC) as `YYYY-MM-DD`: daily products such as NASA GIBS
/// have it complete.
pub fn day_before(unix: f64) -> String {
    globecraft_core::time::date_utc(unix - globecraft_core::time::DAY_SECS)
}

/// Whether a location is a web address (else a path on this computer).
pub fn is_url(location: &str) -> bool {
    let lower = location.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_xyz_tms_and_wms_addresses() {
        assert_eq!(
            tile_url("https://t.example/{z}/{x}/{y}.png", 3, 5, 2, ""),
            "https://t.example/3/5/2.png"
        );
        assert_eq!(
            tile_url("https://t.example/{z}/{x}/{-y}.png", 3, 5, 2, ""),
            "https://t.example/3/5/5.png"
        );
        let wms = wms_template("https://wms.example/mapserv", "GEBCO_LATEST");
        let url = tile_url(&wms, 0, 0, 0, "");
        assert!(
            url.starts_with("https://wms.example/mapserv?SERVICE=WMS&REQUEST=GetMap"),
            "{url}"
        );
        assert!(
            url.contains("BBOX=-20037508.34,-20037508.34,20037508.34,20037508.34"),
            "{url}"
        );
        assert!(wms_template("https://wms.example/ows?map=x", "a").contains("ows?map=x&SERVICE"));
    }

    #[test]
    fn daily_imagery_shows_the_day_before_the_app_time() {
        // 2026-11-04 09:38 UTC: the day before is complete.
        assert_eq!(day_before(1_793_785_080.0), "2026-11-03");
        let url = tile_url(
            "https://gibs.example/{date}/{z}/{y}/{x}.jpg",
            3,
            1,
            2,
            "2026-11-03",
        );
        assert_eq!(url, "https://gibs.example/2026-11-03/3/2/1.jpg");
    }

    #[test]
    fn formats_follow_the_file_name() {
        assert_eq!(
            FeatureFormat::guess("quakes.geojson"),
            Some(FeatureFormat::GeoJson)
        );
        assert_eq!(
            FeatureFormat::guess("https://x.example/feed.json?x=1"),
            Some(FeatureFormat::GeoJson)
        );
        assert_eq!(
            FeatureFormat::guess("/home/me/Track.GPX"),
            Some(FeatureFormat::Gpx)
        );
        assert_eq!(FeatureFormat::guess("points.csv"), Some(FeatureFormat::Csv));
        assert_eq!(FeatureFormat::guess("photo.jpeg"), None);
    }

    #[test]
    fn saved_layers_survive_bad_entries_and_keep_ids_unique() {
        let layer = UserLayer {
            id: 7,
            name: "Seafloor".into(),
            source: UserSource::Tiles {
                template: "https://t.example/{z}/{x}/{y}.png".into(),
                min_zoom: 0,
                max_zoom: 10,
                tile_px: 256,
            },
            visible: true,
            opacity: 0.6,
            attribution: "GEBCO".into(),
            color: default_color(),
            preset: None,
        };
        let json = serde_json::json!([
            serde_json::to_value(&layer).unwrap(),
            {"id": 8, "name": "broken"},
            serde_json::to_value(&layer).unwrap(),
        ]);
        assert_eq!(layers_from_json(&json), vec![layer]);
    }

    #[test]
    fn a_layer_names_only_a_remote_host() {
        let mut layer = UserLayer {
            id: 1,
            name: "Tiles".into(),
            source: UserSource::Tiles {
                template: "https://tiles.example.org/{z}/{x}/{y}.png".into(),
                min_zoom: 0,
                max_zoom: 10,
                tile_px: 256,
            },
            visible: true,
            opacity: 1.0,
            attribution: String::new(),
            color: default_color(),
            preset: None,
        };
        assert_eq!(layer.network_host(), Some("tiles.example.org"));
        layer.source = UserSource::Features {
            location: "https://user:pass@data.example.org:8443/feed.json".into(),
            format: FeatureFormat::GeoJson,
            refresh_minutes: 5,
            value_field: None,
        };
        assert_eq!(layer.network_host(), Some("data.example.org:8443"));
        layer.source = UserSource::Features {
            location: "/home/me/points.geojson".into(),
            format: FeatureFormat::GeoJson,
            refresh_minutes: 0,
            value_field: None,
        };
        assert_eq!(layer.network_host(), None);
    }
}
