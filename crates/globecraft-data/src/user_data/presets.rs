//! The service library (roadmap 2.3): keyless sources from `docs/DATA_SOURCES.md` that
//! were checked to answer, each with its licence, whether commercial use is allowed,
//! and how long it is likely to last.
use super::{FeatureFormat, UserSource, wms_template};

/// How long a source is likely to keep working as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Longevity {
    /// A government or intergovernmental body's standing service.
    Institutional,
    /// A project or company that has run it for years without a guarantee.
    Goodwill,
}

/// A library entry.
#[derive(Clone, Debug)]
pub struct Preset {
    pub code: &'static str,
    /// Display name and description translation keys.
    pub name_key: &'static str,
    pub help_key: &'static str,
    pub category: Category,
    pub licence: &'static str,
    pub commercial_use: bool,
    pub longevity: Longevity,
    pub attribution: &'static str,
    /// The default opacity of its layer.
    pub opacity: f32,
    pub color: &'static str,
    source: fn() -> UserSource,
}

impl Preset {
    pub fn source(&self) -> UserSource {
        (self.source)()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Ocean,
    Earth,
    Hazards,
}

impl Category {
    pub const ALL: [Self; 3] = [Self::Ocean, Self::Earth, Self::Hazards];
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Ocean => "add-data-category-ocean",
            Self::Earth => "add-data-category-earth",
            Self::Hazards => "add-data-category-hazards",
        }
    }
}

fn gebco() -> UserSource {
    UserSource::Tiles {
        template: wms_template("https://wms.gebco.net/mapserv", "GEBCO_LATEST"),
        min_zoom: 0,
        max_zoom: 12,
        tile_px: 256,
    }
}

fn emodnet() -> UserSource {
    UserSource::Tiles {
        template: wms_template("https://ows.emodnet-bathymetry.eu/wms", "emodnet:mean_2018"),
        min_zoom: 0,
        max_zoom: 13,
        tile_px: 256,
    }
}

/// Marine Regions' GeoServer (Flanders Marine Institute); its line styles keep the
/// boundaries visible over imagery.
const MARINE_REGIONS_WMS: &str = "https://geo.vliz.be/geoserver/MarineRegions/wms";

fn styled(template: String, style: &str) -> String {
    template.replace("&STYLES=&", &format!("&STYLES={style}&"))
}

fn eez_boundaries() -> UserSource {
    UserSource::Tiles {
        template: styled(
            wms_template(MARINE_REGIONS_WMS, "MarineRegions:eez_boundaries"),
            "Line_yellow",
        ),
        min_zoom: 0,
        max_zoom: 12,
        tile_px: 256,
    }
}

fn sea_areas() -> UserSource {
    UserSource::Tiles {
        template: styled(
            wms_template(MARINE_REGIONS_WMS, "MarineRegions:iho"),
            "Line_blue_green",
        ),
        min_zoom: 0,
        max_zoom: 10,
        tile_px: 256,
    }
}

fn gibs_true_colour() -> UserSource {
    UserSource::Tiles {
        template: "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/\
                   MODIS_Terra_CorrectedReflectance_TrueColor/default/{date}/\
                   GoogleMapsCompatible_Level9/{z}/{y}/{x}.jpg"
            .into(),
        min_zoom: 0,
        max_zoom: 9,
        tile_px: 256,
    }
}

fn forest_loss() -> UserSource {
    UserSource::Tiles {
        template:
            "https://tiles.globalforestwatch.org/umd_tree_cover_loss/v1.13/mercator/{z}/{x}/{y}.png"
                .into(),
        min_zoom: 0,
        max_zoom: 12,
        tile_px: 512,
    }
}

fn earthquakes() -> UserSource {
    UserSource::Features {
        location: "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_week.geojson"
            .into(),
        format: FeatureFormat::GeoJson,
        // USGS updates the feed every minute and asks clients not to poll faster.
        refresh_minutes: 5,
        value_field: Some("mag".into()),
    }
}

/// Every library entry, by category.
pub static PRESETS: &[Preset] = &[
    Preset {
        code: "gebco",
        name_key: "preset-gebco",
        help_key: "preset-gebco-help",
        category: Category::Ocean,
        licence: "Public domain (GEBCO)",
        commercial_use: true,
        longevity: Longevity::Institutional,
        attribution: "GEBCO Compilation Group (2026) GEBCO 2026 Grid",
        opacity: 1.0,
        color: "#4aa3ff",
        source: gebco,
    },
    Preset {
        code: "emodnet_bathymetry",
        name_key: "preset-emodnet",
        help_key: "preset-emodnet-help",
        category: Category::Ocean,
        licence: "EMODnet open data",
        commercial_use: true,
        longevity: Longevity::Institutional,
        attribution: "EMODnet Bathymetry Consortium",
        opacity: 0.85,
        color: "#4aa3ff",
        source: emodnet,
    },
    Preset {
        code: "eez_boundaries",
        name_key: "preset-eez",
        help_key: "preset-eez-help",
        category: Category::Ocean,
        // CC BY since Maritime Boundaries v11; meant for research and education, not
        // navigation or resource exploration (marineregions.org/disclaimer.php).
        licence: "CC BY 4.0 (Marine Regions)",
        commercial_use: true,
        longevity: Longevity::Institutional,
        attribution: "Flanders Marine Institute, MarineRegions.org Maritime Boundaries",
        opacity: 1.0,
        color: "#ffff66",
        source: eez_boundaries,
    },
    Preset {
        code: "iho_sea_areas",
        name_key: "preset-iho",
        help_key: "preset-iho-help",
        category: Category::Ocean,
        licence: "CC BY 4.0 (Marine Regions)",
        commercial_use: true,
        longevity: Longevity::Institutional,
        attribution: "Flanders Marine Institute, MarineRegions.org IHO Sea Areas",
        opacity: 1.0,
        color: "#2fb5b0",
        source: sea_areas,
    },
    Preset {
        code: "gibs_true_colour",
        name_key: "preset-gibs",
        help_key: "preset-gibs-help",
        category: Category::Earth,
        licence: "NASA open data",
        commercial_use: true,
        longevity: Longevity::Institutional,
        attribution: "NASA EOSDIS GIBS, MODIS Terra",
        opacity: 1.0,
        color: "#ffffff",
        source: gibs_true_colour,
    },
    Preset {
        code: "forest_loss",
        name_key: "preset-forest-loss",
        help_key: "preset-forest-loss-help",
        category: Category::Earth,
        licence: "CC BY 4.0",
        commercial_use: true,
        longevity: Longevity::Goodwill,
        attribution: "Hansen/UMD/Google/USGS/NASA, Global Forest Watch",
        opacity: 1.0,
        color: "#ff4fa0",
        source: forest_loss,
    },
    Preset {
        code: "earthquakes",
        name_key: "preset-earthquakes",
        help_key: "preset-earthquakes-help",
        category: Category::Hazards,
        licence: "Public domain (USGS)",
        commercial_use: true,
        longevity: Longevity::Institutional,
        attribution: "U.S. Geological Survey",
        opacity: 1.0,
        color: "#ff5a3c",
        source: earthquakes,
    },
];

pub fn find(code: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_complete_and_addressed_over_https() {
        let mut codes: Vec<&str> = PRESETS.iter().map(|p| p.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), PRESETS.len());
        for preset in PRESETS {
            assert!(!preset.attribution.is_empty() && !preset.licence.is_empty());
            match preset.source() {
                UserSource::Tiles { template, .. } => {
                    assert!(template.starts_with("https://"), "{}", preset.code);
                    assert!(
                        template.contains("{bbox}") || template.contains("{z}"),
                        "{}",
                        preset.code
                    );
                    if preset.code.starts_with("eez") || preset.code.starts_with("iho") {
                        assert!(!template.contains("&STYLES=&"), "{}: unstyled", preset.code);
                    }
                }
                UserSource::Features { location, .. } => {
                    assert!(location.starts_with("https://"), "{}", preset.code);
                }
            }
        }
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// Every library source answers: a tile for web maps, features for feeds.
    #[tokio::test]
    #[ignore = "network: fetches a sample from every library source"]
    async fn every_preset_answers() {
        let mut failed = Vec::new();
        for preset in PRESETS {
            let result = match preset.source() {
                UserSource::Tiles { template, .. } => super::super::test_tile(template, 2)
                    .await
                    .map(|(w, h)| format!("tile {w}x{h}")),
                UserSource::Features {
                    location,
                    format,
                    value_field,
                    ..
                } => super::super::load_features(location, format, value_field)
                    .await
                    .map(|set| format!("{} points, {} lines", set.points.len(), set.lines.len())),
            };
            match result {
                Ok(text) => println!("PRESET {}: ok, {text}", preset.code),
                Err(error) => {
                    println!("PRESET {}: FAILED {error}", preset.code);
                    failed.push(preset.code);
                }
            }
        }
        assert!(failed.is_empty(), "{failed:?}");
    }
}
