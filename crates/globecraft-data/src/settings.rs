pub mod appearance;
pub mod assistant;
pub mod cache;
pub mod controllers;
mod coordinates;
pub mod layers;
pub mod map_style;
pub mod navigation;
pub mod sidebar;
pub mod sky;
pub mod workspace;
mod writer;
use crate::paths::{cache_dir, default_out_dir};
use globecraft_core::{earth_model::EarthModel, language::LanguagePreference, units::RulerUnit};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use writer::SettingsWriter;
pub const MIN_UI_SCALE: f32 = 0.7;
pub const MAX_UI_SCALE: f32 = 2.5;
pub const MIN_MAP_SCALE: f32 = 0.6;
pub const MAX_MAP_SCALE: f32 = 3.0;

fn lang_file() -> PathBuf {
    cache_dir().join("lang.txt")
}
pub fn load_language_preference() -> LanguagePreference {
    load_language_preference_at(&lang_file())
}
fn load_language_preference_at(path: &std::path::Path) -> LanguagePreference {
    std::fs::read_to_string(path)
        .ok()
        .as_deref()
        .map(str::trim)
        .and_then(LanguagePreference::from_code)
        .unwrap_or_default()
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitPreference {
    #[default]
    System,
    Metric,
    Imperial,
}
impl UnitPreference {
    pub fn resolve(
        self,
        system: globecraft_core::units::DistanceUnits,
    ) -> globecraft_core::units::DistanceUnits {
        use globecraft_core::units::DistanceUnits;
        match self {
            Self::System => system,
            Self::Metric => DistanceUnits::Metric,
            Self::Imperial => DistanceUnits::Imperial,
        }
    }
}
#[derive(Clone, PartialEq, Serialize)]
pub struct Settings {
    pub sky: sky::SkyPreferences,
    pub controllers: controllers::ControllerPreferences,
    pub appearance: appearance::AppearancePreferences,
    pub navigation: navigation::NavigationPreferences,
    pub cache: cache::CachePreferences,
    pub sidebar: sidebar::SidebarLayout,
    pub workspace: workspace::WorkspaceLayout,
    pub layers: layers::LayerPreferences,
    pub assistant: assistant::AssistantPreferences,
    #[serde(serialize_with = "coordinates::serialize")]
    pub coordinate_display: globecraft_core::coordinates::CoordinateDisplay,
    pub screenshot_dir: String,
    pub kml_dir: String,
    pub ui_scale: f32,  // text size of buttons/panels
    pub map_scale: f32, // text size of map labels
    /// Colour, width and dash of border lines; label sizes.
    pub map_style: map_style::MapStyle,
    pub uncap_fps: bool,
    pub grid_numbers: bool,
    pub hide_navigation_controls: bool,
    /// Camera altitude (km) below which state/province lines and their names
    /// appear; user-tunable from the Layers panel.
    pub state_max_altitude_km: f64,
    pub distance_units: UnitPreference,
    #[serde(serialize_with = "serialize_earth_model")]
    pub earth_model: EarthModel,
    #[serde(serialize_with = "serialize_ruler_unit")]
    pub ruler_unit: RulerUnit,
}
fn serialize_earth_model<S: serde::Serializer>(
    model: &EarthModel,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(model.code())
}
fn serialize_ruler_unit<S: serde::Serializer>(
    unit: &RulerUnit,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(unit.code())
}
impl Default for Settings {
    fn default() -> Self {
        Settings {
            sky: sky::SkyPreferences::default(),
            controllers: controllers::ControllerPreferences::default(),
            appearance: appearance::AppearancePreferences::default(),
            navigation: navigation::NavigationPreferences::default(),
            cache: cache::CachePreferences::default(),
            sidebar: sidebar::SidebarLayout::default(),
            workspace: workspace::WorkspaceLayout::default(),
            layers: layers::LayerPreferences::default(),
            assistant: assistant::AssistantPreferences::default(),
            coordinate_display: globecraft_core::coordinates::CoordinateDisplay::default(),
            screenshot_dir: default_out_dir("Pictures"),
            kml_dir: default_out_dir("Documents"),
            ui_scale: 1.0,
            map_scale: 1.0,
            map_style: map_style::MapStyle::default(),
            uncap_fps: false,
            grid_numbers: false,
            hide_navigation_controls: false,
            state_max_altitude_km: STATE_MAX_ALTITUDE_DEFAULT_KM,
            distance_units: UnitPreference::System,
            earth_model: EarthModel::default(),
            ruler_unit: RulerUnit::Automatic,
        }
    }
}
/// Policy bounds for the user-tunable state-line visibility altitude (km):
/// the shipped default and a sane clamp ceiling for the Layers panel input.
pub const STATE_MAX_ALTITUDE_DEFAULT_KM: f64 = 1_609.0;
pub const STATE_MAX_ALTITUDE_MAX_KM: f64 = 8_000.0;

fn settings_file() -> PathBuf {
    cache_dir().join("settings.json")
}
#[derive(Default, Deserialize)]
struct SettingsInput {
    sky: Option<serde_json::Value>,
    controllers: Option<serde_json::Value>,
    appearance: Option<serde_json::Value>,
    navigation: Option<serde_json::Value>,
    cache: Option<serde_json::Value>,
    sidebar: Option<serde_json::Value>,
    workspace: Option<serde_json::Value>,
    layers: Option<serde_json::Value>,
    assistant: Option<serde_json::Value>,
    coordinate_display: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::json_fields::string")]
    screenshot_dir: Option<String>,
    #[serde(default, deserialize_with = "crate::json_fields::string")]
    kml_dir: Option<String>,
    #[serde(default, deserialize_with = "crate::json_fields::number")]
    ui_scale: Option<f64>,
    #[serde(default, deserialize_with = "crate::json_fields::number")]
    map_scale: Option<f64>,
    map_style: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::json_fields::boolean")]
    uncap_fps: Option<bool>,
    #[serde(default, deserialize_with = "crate::json_fields::boolean")]
    grid_numbers: Option<bool>,
    #[serde(default, deserialize_with = "crate::json_fields::boolean")]
    hide_navigation_controls: Option<bool>,
    #[serde(default, deserialize_with = "crate::json_fields::number")]
    state_max_altitude_km: Option<f64>,
    #[serde(default, deserialize_with = "crate::json_fields::string")]
    distance_units: Option<String>,
    #[serde(default, deserialize_with = "crate::json_fields::string")]
    earth_model: Option<String>,
    #[serde(default, deserialize_with = "crate::json_fields::string")]
    ruler_unit: Option<String>,
}
impl Settings {
    fn apply_json(&mut self, bytes: &[u8]) -> Result<(), crate::Error> {
        let input: SettingsInput = serde_json::from_slice(bytes)?;
        if let Some(value) = input.sky {
            self.sky.apply(&value);
        }
        if let Some(value) = input.controllers {
            self.controllers.apply(&value);
        }
        if let Some(value) = input.appearance {
            self.appearance.apply(&value);
        }
        if let Some(value) = input.navigation {
            self.navigation.apply(&value);
        }
        if let Some(value) = input.cache {
            self.cache.apply(&value);
        }
        if let Some(value) = input.coordinate_display {
            coordinates::apply(&mut self.coordinate_display, &value);
        }
        if let Some(sidebar) = input.sidebar {
            self.sidebar.apply_json(&sidebar);
        }
        if let Some(value) = input.workspace {
            self.workspace.apply(&value);
        }
        if let Some(value) = input.layers {
            self.layers.apply(&value);
        }
        if let Some(value) = input.assistant {
            self.assistant.apply(&value);
        }
        if let Some(path) = input.screenshot_dir.filter(|s| !s.is_empty()) {
            self.screenshot_dir = path;
        }
        if let Some(path) = input.kml_dir.filter(|s| !s.is_empty()) {
            self.kml_dir = path;
        }
        if let Some(scale) = input.ui_scale {
            self.ui_scale = (scale as f32).clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        }
        if let Some(scale) = input.map_scale {
            self.map_scale = (scale as f32).clamp(MIN_MAP_SCALE, MAX_MAP_SCALE);
        }
        if let Some(value) = input.map_style {
            self.map_style.apply(&value);
        }
        if let Some(uncap) = input.uncap_fps {
            self.uncap_fps = uncap;
        }
        if let Some(hide) = input.hide_navigation_controls {
            self.hide_navigation_controls = hide;
        }
        if let Some(state_max) = input.state_max_altitude_km {
            self.state_max_altitude_km = state_max.clamp(0.0, STATE_MAX_ALTITUDE_MAX_KM);
        }
        if let Some(model) = input.earth_model.as_deref().and_then(EarthModel::from_code) {
            self.earth_model = model;
        }
        if let Some(units) = input.distance_units {
            self.distance_units = match units.as_str() {
                "metric" => UnitPreference::Metric,
                "imperial" => UnitPreference::Imperial,
                "system" => UnitPreference::System,
                _ => self.distance_units,
            };
        }
        if let Some(unit) = input.ruler_unit.as_deref().and_then(RulerUnit::from_code) {
            self.ruler_unit = unit;
        }
        if let Some(value) = input.grid_numbers {
            self.grid_numbers = value;
        }
        Ok(())
    }
    /// Whether a layer is shown. Stars, cinematic sky and grid labels keep their original
    /// settings fields, so files from earlier releases keep those choices.
    pub fn layer_visible(&self, layer: globecraft_core::layers::MapLayer) -> bool {
        use globecraft_core::layers::MapLayer;
        match layer {
            MapLayer::Stars => self.sky.enabled,
            MapLayer::CinematicSky => self.sky.cinematic,
            MapLayer::GridLabels => self.grid_numbers,
            _ => self.layers.visible.contains(layer),
        }
    }
    pub fn set_layer_visible(&mut self, layer: globecraft_core::layers::MapLayer, visible: bool) {
        use globecraft_core::layers::MapLayer;
        match layer {
            MapLayer::Stars => self.sky.enabled = visible,
            MapLayer::CinematicSky => self.sky.cinematic = visible,
            MapLayer::GridLabels => self.grid_numbers = visible,
            _ => self.layers.visible.set(layer, visible),
        }
    }
    /// Every visible layer, for frame inputs.
    pub fn visible_layers(&self) -> globecraft_core::layers::LayerSet {
        globecraft_core::layers::MapLayer::ALL
            .into_iter()
            .filter(|layer| self.layer_visible(*layer))
            .collect()
    }
    pub fn load() -> Self {
        let mut settings = Self::default();
        if let Ok(bytes) = std::fs::read(settings_file()) {
            let _ = settings.apply_json(&bytes);
        }
        settings
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layer_visibility_uses_one_field_per_layer() {
        use globecraft_core::layers::MapLayer;
        let mut settings = Settings::default();
        for layer in MapLayer::ALL {
            let before = settings.layer_visible(layer);
            settings.set_layer_visible(layer, !before);
            assert_eq!(settings.layer_visible(layer), !before, "{layer:?}");
            assert_eq!(settings.visible_layers().contains(layer), !before);
        }
        let mut restored = Settings::default();
        restored
            .apply_json(&serde_json::to_vec(&settings).unwrap())
            .unwrap();
        // Live feeds are not restored; everything else is.
        let expected: globecraft_core::layers::LayerSet = settings
            .visible_layers()
            .iter()
            .filter(|layer| !layer.starts_hidden())
            .collect();
        assert_eq!(restored.visible_layers(), expected);
    }
    #[test]
    fn celestial_preferences_roundtrip_and_ignore_bad_fields() {
        let mut settings = Settings::default();
        assert!(!settings.sky.enabled);
        settings.apply_json(br#"{}"#).unwrap();
        assert!(!settings.sky.enabled);
        settings
            .apply_json(br#"{"sky":{"enabled":true,"cinematic":true}}"#)
            .unwrap();
        let mut restored = Settings::default();
        restored
            .apply_json(&serde_json::to_vec(&settings).unwrap())
            .unwrap();
        assert_eq!(restored.sky, settings.sky);
        restored
            .apply_json(br#"{"sky":{"enabled":42,"cinematic":"bad"}}"#)
            .unwrap();
        assert_eq!(restored.sky, settings.sky);
    }
    #[test]
    fn categorized_preferences_roundtrip_and_validate_independent_fields() {
        let mut settings = Settings::default();
        settings.apply_json(br#"{
          "appearance":{"theme":"light","ui_font":"Example UI","map_font":"Example Map"},
          "navigation":{"pan_sensitivity":2,"zoom_sensitivity":0,"rotation_sensitivity":99,
            "invert_scroll":true,"startup":{"latitude":90,"longitude":180,"altitude_km":-10,"heading_deg":370}},
          "cache":{"decoded_mib":1,"disk_mib":999999}}
        "#).unwrap();
        assert_eq!(
            settings.appearance.theme,
            appearance::ThemePreference::Light
        );
        assert_eq!(settings.navigation.pan_sensitivity, 2.0);
        assert_eq!(
            settings.navigation.zoom_sensitivity,
            navigation::MIN_SENSITIVITY
        );
        assert_eq!(
            settings.navigation.rotation_sensitivity,
            navigation::MAX_SENSITIVITY
        );
        assert_eq!(
            settings.navigation.startup.camera().lat,
            globecraft_core::MAX_CAMERA_LATITUDE_DEG
        );
        assert_eq!(
            settings.navigation.startup.camera().alt,
            globecraft_core::MIN_CAMERA_ALTITUDE_KM
        );
        assert_eq!(settings.navigation.startup.heading_deg, 10.0);
        assert_eq!(settings.cache.decoded_mib, cache::MIN_CACHE_MIB);
        assert_eq!(settings.cache.disk_mib, cache::MAX_DISK_CACHE_MIB);
        let bytes = serde_json::to_vec(&settings).unwrap();
        let mut restored = Settings::default();
        restored.apply_json(&bytes).unwrap();
        assert_eq!(restored.appearance, settings.appearance);
        assert_eq!(restored.navigation, settings.navigation);
        assert_eq!(restored.cache, settings.cache);
        restored.apply_json(br#"{"appearance":{"theme":"invalid","map_font":4},"navigation":{"pan_sensitivity":"bad","startup":{"latitude":999,"longitude":0}},"cache":{"decoded_mib":"bad"}}"#).unwrap();
        assert_eq!(restored.appearance, settings.appearance);
        assert_eq!(restored.navigation, settings.navigation);
        assert_eq!(restored.cache, settings.cache);
    }
    #[test]
    fn earth_models_roundtrip_and_tolerate_unknown_preferences() {
        let mut settings = Settings::default();
        assert_eq!(settings.earth_model, EarthModel::Wgs84);
        settings.apply_json(br#"{}"#).unwrap();
        assert_eq!(settings.earth_model, EarthModel::Wgs84);
        for model in EarthModel::ALL {
            settings.earth_model = model;
            let mut restored = Settings::default();
            restored
                .apply_json(&serde_json::to_vec(&settings).unwrap())
                .unwrap();
            assert_eq!(restored.earth_model, model);
            for bytes in [
                br#"{}"#.as_slice(),
                br#"{"earth_model":"unknown"}"#,
                br#"{"earth_model":42}"#,
            ] {
                restored.apply_json(bytes).unwrap();
                assert_eq!(restored.earth_model, model);
            }
        }
    }
    #[test]
    fn grid_preference_roundtrips_and_tolerates_missing_or_invalid_fields() {
        let mut settings = Settings::default();
        assert!(!settings.grid_numbers);
        settings.apply_json(br#"{"grid_numbers":true}"#).unwrap();
        for input in [
            br#"{"navigation_compass":false}"#.as_slice(),
            br#"{"grid_numbers":42}"#,
        ] {
            settings.apply_json(input).unwrap();
            assert!(settings.grid_numbers);
        }
        let mut restored = Settings::default();
        restored
            .apply_json(&serde_json::to_vec(&settings).unwrap())
            .unwrap();
        assert!(restored.grid_numbers);
        restored.apply_json(br#"{"grid_numbers":false}"#).unwrap();
        assert!(!restored.grid_numbers);
    }

    #[test]
    fn every_ruler_preference_roundtrips_and_invalid_values_keep_the_selection() {
        let mut settings = Settings::default();
        settings.apply_json(br#"{}"#).unwrap();
        assert_eq!(settings.ruler_unit, RulerUnit::Automatic);
        for unit in RulerUnit::ALL {
            settings.ruler_unit = unit;
            let bytes = serde_json::to_vec(&settings).unwrap();
            let mut loaded = Settings::default();
            loaded.apply_json(&bytes).unwrap();
            assert_eq!(loaded.ruler_unit, unit);
            loaded
                .apply_json(br#"{"distance_units":"imperial"}"#)
                .unwrap();
            assert_eq!(loaded.ruler_unit, unit);
            loaded.apply_json(br#"{"ruler_unit":"unknown"}"#).unwrap();
            assert_eq!(loaded.ruler_unit, unit);
            loaded.apply_json(br#"{"ruler_unit":42}"#).unwrap();
            assert_eq!(loaded.ruler_unit, unit);
        }
    }
    #[test]
    fn units_roundtrip_and_tolerate_old_or_invalid_preferences() {
        use globecraft_core::units::DistanceUnits;
        let mut settings = Settings::default();
        settings.apply_json(br#"{}"#).unwrap();
        assert_eq!(settings.distance_units, UnitPreference::System);
        settings
            .apply_json(br#"{"distance_units":"imperial"}"#)
            .unwrap();
        assert_eq!(
            settings.distance_units.resolve(DistanceUnits::Metric),
            DistanceUnits::Imperial
        );
        for bytes in [
            br#"{"distance_units":42}"#.as_slice(),
            br#"{"distance_units":"unknown"}"#,
        ] {
            settings.apply_json(bytes).unwrap();
            assert_eq!(settings.distance_units, UnitPreference::Imperial);
        }
        let bytes = serde_json::to_vec(&settings).unwrap();
        let mut loaded = Settings::default();
        loaded.apply_json(&bytes).unwrap();
        assert_eq!(loaded.distance_units, UnitPreference::Imperial);
        loaded
            .apply_json(br#"{"distance_units":"system"}"#)
            .unwrap();
        assert_eq!(
            loaded.distance_units.resolve(DistanceUnits::Metric),
            DistanceUnits::Metric
        );
        assert_eq!(
            loaded.distance_units.resolve(DistanceUnits::Imperial),
            DistanceUnits::Imperial
        );
    }
    #[test]
    fn partial_invalid_settings_preserve_valid_fields_and_clamping() {
        let mut settings = Settings {
            sky: sky::SkyPreferences::default(),
            controllers: controllers::ControllerPreferences::default(),
            appearance: appearance::AppearancePreferences::default(),
            navigation: navigation::NavigationPreferences::default(),
            cache: cache::CachePreferences::default(),
            sidebar: sidebar::SidebarLayout::default(),
            workspace: workspace::WorkspaceLayout::default(),
            layers: layers::LayerPreferences::default(),
            assistant: assistant::AssistantPreferences::default(),
            coordinate_display: globecraft_core::coordinates::CoordinateDisplay::default(),
            screenshot_dir: "pictures".into(),
            kml_dir: "documents".into(),
            ui_scale: 1.0,
            map_scale: 1.0,
            map_style: map_style::MapStyle::default(),
            uncap_fps: false,
            grid_numbers: false,
            hide_navigation_controls: false,
            state_max_altitude_km: STATE_MAX_ALTITUDE_DEFAULT_KM,
            distance_units: UnitPreference::System,
            earth_model: EarthModel::default(),
            ruler_unit: RulerUnit::Automatic,
        };
        settings
            .apply_json(
                br#"{"screenshot_dir":123,"kml_dir":"custom","ui_scale":9,"map_scale":"wrong","uncap_fps":"wrong"}"#,
            )
            .unwrap();
        assert_eq!(settings.screenshot_dir, "pictures");
        assert_eq!(settings.kml_dir, "custom");
        assert_eq!(settings.ui_scale, MAX_UI_SCALE);
        assert_eq!(settings.map_scale, 1.0);
        assert!(!settings.uncap_fps);
        assert!(!settings.hide_navigation_controls);
        settings
            .apply_json(br#"{"kml_dir":"","ui_scale":-5,"map_scale":9,"uncap_fps":true,"hide_navigation_controls":true}"#)
            .unwrap();
        assert_eq!(settings.kml_dir, "custom");
        assert_eq!(settings.ui_scale, MIN_UI_SCALE);
        assert_eq!(settings.map_scale, MAX_MAP_SCALE);
        assert!(settings.uncap_fps);
        settings.apply_json(br#"{}"#).unwrap();
        assert!(settings.uncap_fps);
        assert!(settings.hide_navigation_controls);
        settings
            .apply_json(br#"{"hide_navigation_controls":"wrong"}"#)
            .unwrap();
        assert!(settings.hide_navigation_controls);
        let json: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(json["kml_dir"], "custom");
        assert_eq!(json["uncap_fps"], true);
        assert_eq!(json["hide_navigation_controls"], true);
        let mut restored = Settings::default();
        restored
            .apply_json(&serde_json::to_vec(&settings).unwrap())
            .unwrap();
        assert!(restored.hide_navigation_controls);
    }
}
