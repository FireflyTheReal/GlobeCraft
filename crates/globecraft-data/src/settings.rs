pub mod appearance;
pub mod assistant;
pub mod cache;
pub mod controllers;
mod coordinates;
pub mod experience;
pub mod layers;
mod lenient;
pub mod map_style;
pub mod navigation;
pub mod screenshot;
pub mod sidebar;
pub mod sky;
pub mod workspace;
mod writer;
use crate::paths::{config_dir, default_out_dir};
use globecraft_core::{earth_model::EarthModel, language::LanguagePreference, units::RulerUnit};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
pub use writer::SettingsWriter;
pub const MIN_UI_SCALE: f32 = 0.7;
pub const MAX_UI_SCALE: f32 = 2.5;
pub const MIN_MAP_SCALE: f32 = 0.6;
pub const MAX_MAP_SCALE: f32 = 3.0;

fn lang_file() -> PathBuf {
    config_dir().join("lang.txt")
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
/// The settings file format. Raise it when a release changes what a saved value means,
/// and teach [`migrate`] to bring older files forward.
pub const SETTINGS_VERSION: u64 = 1;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Always [`SETTINGS_VERSION`] once loaded; files without one are version 0.
    pub version: u64,
    pub sky: sky::SkyPreferences,
    pub controllers: controllers::ControllerPreferences,
    pub appearance: appearance::AppearancePreferences,
    pub navigation: navigation::NavigationPreferences,
    pub cache: cache::CachePreferences,
    pub sidebar: sidebar::SidebarLayout,
    pub workspace: workspace::WorkspaceLayout,
    pub layers: layers::LayerPreferences,
    pub assistant: assistant::AssistantPreferences,
    #[serde(
        serialize_with = "coordinates::serialize",
        deserialize_with = "coordinates::deserialize"
    )]
    pub coordinate_display: globecraft_core::coordinates::CoordinateDisplay,
    pub screenshot_dir: String,
    pub screenshot_quality: screenshot::ScreenshotQuality,
    pub kml_dir: String,
    pub ui_scale: f32,  // text size of buttons/panels
    pub map_scale: f32, // text size of map labels
    /// Colour, width and dash of border lines; label sizes.
    pub map_style: map_style::MapStyle,
    pub uncap_fps: bool,
    /// Work offline: no request leaves GlobeCraft; tiles and data come from disk only.
    pub offline: bool,
    /// Keep service and assistant keys in the system keychain (Settings › Keys).
    pub keys_in_keychain: bool,
    /// Lets the globe use the GPU renderer where it is on by default (Apple
    /// Silicon, beta); off keeps the CPU renderer. Read at startup.
    pub gpu_globe: bool,
    /// The resolution a moving view without relief renders at, as a share of the full
    /// one (0.2–1); `None`: automatic (0.4).
    pub motion_resolution: Option<f32>,
    /// The same for a view with 3D relief (it also applies while relief grows in);
    /// `None`: automatic (0.28).
    pub motion_resolution_relief: Option<f32>,
    pub grid_numbers: bool,
    pub hide_navigation_controls: bool,
    /// Camera altitude (km) below which state/province lines and their names
    /// appear; user-tunable from the Layers panel.
    pub state_max_altitude_km: f64,
    pub distance_units: UnitPreference,
    #[serde(
        serialize_with = "serialize_earth_model",
        deserialize_with = "deserialize_earth_model"
    )]
    pub earth_model: EarthModel,
    #[serde(
        serialize_with = "serialize_ruler_unit",
        deserialize_with = "deserialize_ruler_unit"
    )]
    pub ruler_unit: RulerUnit,
    /// Layers the user added (Add Data), bottom to top.
    #[serde(deserialize_with = "deserialize_user_layers")]
    pub user_layers: Vec<crate::user_data::UserLayer>,
    /// Starred and recently added sources (the Browser pane).
    pub saved_sources: crate::user_data::SavedSources,
    /// How much of the interface shows (Basic or Advanced).
    pub experience: experience::ExperienceLevel,
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
fn deserialize_earth_model<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<EarthModel, D::Error> {
    lenient::coded(deserializer, EarthModel::from_code)
}
fn deserialize_ruler_unit<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<RulerUnit, D::Error> {
    lenient::coded(deserializer, RulerUnit::from_code)
}
fn deserialize_user_layers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<crate::user_data::UserLayer>, D::Error> {
    lenient::applied(deserializer, |layers: &mut Vec<_>, value| {
        *layers = crate::user_data::layers_from_json(value);
    })
}
impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
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
            screenshot_quality: screenshot::ScreenshotQuality::default(),
            kml_dir: default_out_dir("Documents"),
            ui_scale: 1.0,
            map_scale: 1.0,
            map_style: map_style::MapStyle::default(),
            uncap_fps: false,
            offline: false,
            keys_in_keychain: false,
            gpu_globe: true,
            motion_resolution: None,
            motion_resolution_relief: None,
            grid_numbers: false,
            hide_navigation_controls: false,
            state_max_altitude_km: STATE_MAX_ALTITUDE_DEFAULT_KM,
            distance_units: UnitPreference::System,
            earth_model: EarthModel::default(),
            ruler_unit: RulerUnit::Automatic,
            user_layers: Vec::new(),
            saved_sources: Default::default(),
            experience: Default::default(),
        }
    }
}
/// Policy bounds for the user-tunable state-line visibility altitude (km):
/// the shipped default and a sane clamp ceiling for the Layers panel input.
pub const STATE_MAX_ALTITUDE_DEFAULT_KM: f64 = 1_609.0;
pub const STATE_MAX_ALTITUDE_MAX_KM: f64 = 8_000.0;

/// Brings a settings file written by an older version to [`SETTINGS_VERSION`], in place.
/// Fields a version renamed or whose meaning changed are rewritten here, so the readers
/// below only know the current format.
fn migrate(value: &mut serde_json::Value) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let version = map
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if version < 1 {
        // Version 0 files without a level were written before levels existed: they
        // keep showing everything.
        map.entry("experience")
            .or_insert_with(|| serde_json::Value::from("advanced"));
    }
    map.insert("version".to_owned(), SETTINGS_VERSION.into());
}

/// `settings.json.bak` beside `path`: the last settings file that loaded.
fn backup_path(path: &std::path::Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".bak");
    path.with_file_name(name)
}

fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}
/// Top-level fields `apply_json` reads by its own rules: an empty folder name is not
/// taken, and the user's layers are kept entry by entry.
const OWN_RULES: [&str; 3] = ["screenshot_dir", "kml_dir", "user_layers"];

impl Settings {
    fn apply_json(&mut self, bytes: &[u8]) -> Result<(), crate::Error> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
        migrate(&mut value);
        if !value.is_object() {
            return Err(crate::Error::msg("a settings file holds a JSON object"));
        }
        // Every plain setting, by its field's name and type.
        lenient::apply(self, &value, &OWN_RULES);
        self.ui_scale = self.ui_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        self.map_scale = self.map_scale.clamp(MIN_MAP_SCALE, MAX_MAP_SCALE);
        self.state_max_altitude_km = self
            .state_max_altitude_km
            .clamp(0.0, STATE_MAX_ALTITUDE_MAX_KM);
        let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
        if let Some(path) = text("screenshot_dir").filter(|path| !path.is_empty()) {
            self.screenshot_dir = path.to_owned();
        }
        if let Some(path) = text("kml_dir").filter(|path| !path.is_empty()) {
            self.kml_dir = path.to_owned();
        }
        // The sections and tables read themselves.
        if let Some(value) = value.get("sky") {
            self.sky.apply(value);
        }
        if let Some(value) = value.get("controllers") {
            self.controllers.apply(value);
        }
        if let Some(value) = value.get("appearance") {
            self.appearance.apply(value);
        }
        if let Some(value) = value.get("navigation") {
            self.navigation.apply(value);
        }
        if let Some(value) = value.get("cache") {
            self.cache.apply(value);
        }
        if let Some(value) = value.get("coordinate_display") {
            coordinates::apply(&mut self.coordinate_display, value);
        }
        if let Some(value) = value.get("sidebar") {
            self.sidebar.apply_json(value);
        }
        if let Some(value) = value.get("workspace") {
            self.workspace.apply(value);
        }
        if let Some(value) = value.get("layers") {
            self.layers.apply(value);
        }
        if let Some(value) = value.get("assistant") {
            self.assistant.apply(value);
        }
        if let Some(value) = value.get("map_style") {
            self.map_style.apply(value);
        }
        if let Some(value) = value.get("user_layers") {
            self.user_layers = crate::user_data::layers_from_json(value);
            crate::user_data::presets::refresh_sources(&mut self.user_layers);
        }
        if let Some(value) = value.get("saved_sources") {
            self.saved_sources.apply(value);
        }
        Ok(())
    }
    /// Whether a layer is shown. Sky layers and grid labels keep their settings fields
    /// (`sky.stars`, `sky.enabled` for the Milky Way), so earlier files keep their choices.
    pub fn layer_visible(&self, layer: globecraft_core::layers::MapLayer) -> bool {
        use globecraft_core::layers::MapLayer;
        match layer {
            MapLayer::Stars => self.sky.stars,
            MapLayer::MilkyWay => self.sky.enabled,
            MapLayer::GridLabels => self.grid_numbers,
            _ => self.layers.visible.contains(layer),
        }
    }
    pub fn set_layer_visible(&mut self, layer: globecraft_core::layers::MapLayer, visible: bool) {
        use globecraft_core::layers::MapLayer;
        match layer {
            MapLayer::Stars => self.sky.stars = visible,
            MapLayer::MilkyWay => self.sky.enabled = visible,
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
    /// The settings as an export file (File › Export Settings): readable JSON. Service and
    /// assistant keys are stored apart and never included.
    pub fn export(&self) -> Result<Vec<u8>, crate::Error> {
        Ok(serde_json::to_vec_pretty(self)?)
    }
    /// Settings read from an export file, over the defaults, as a saved file is read. A
    /// JSON value that shares no field with the settings is refused, so importing the wrong
    /// file cannot reset everything.
    pub fn import(bytes: &[u8]) -> Result<Self, crate::Error> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let known = serde_json::to_value(Self::default())?;
        let shares_a_field = value
            .as_object()
            .zip(known.as_object())
            .is_some_and(|(file, known)| file.keys().any(|key| known.contains_key(key)));
        if !shares_a_field {
            return Err(crate::Error::Message(
                "the file holds no GlobeCraft settings".to_owned(),
            ));
        }
        let mut settings = Self::default();
        settings.apply_json(bytes)?;
        Ok(settings)
    }
    /// The saved settings, and an English message when `settings.json` exists but could not
    /// be used. A file that does not parse is renamed `settings.json.corrupt-<unix seconds>`
    /// first, so the next save cannot overwrite what it held, and the defaults are used.
    pub fn load() -> (Self, Option<String>) {
        Self::load_from(&settings_file())
    }
    fn load_from(path: &std::path::Path) -> (Self, Option<String>) {
        let mut settings = Self::default();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (settings, None),
            Err(error) => {
                return (
                    settings,
                    Some(format!(
                        "Could not read {} ({error}); default settings are in use.",
                        path.display()
                    )),
                );
            }
        };
        let backup = backup_path(path);
        let Err(error) = settings.apply_json(&bytes) else {
            // The last file that loaded, for when a later one cannot be read.
            if let Err(error) = crate::paths::write_atomic(&backup, &bytes, false) {
                crate::log_line!("Could not keep a copy of the settings: {error}");
            }
            return (settings, None);
        };
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let mut kept_name = path.file_name().unwrap_or_default().to_owned();
        kept_name.push(format!(".corrupt-{seconds}"));
        let kept = path.with_file_name(kept_name);
        let set_aside = std::fs::rename(path, &kept);
        // The copy of the last file that loaded, if there is one and it reads.
        let mut restored = Self::default();
        let from_backup = std::fs::read(&backup)
            .ok()
            .is_some_and(|bytes| restored.apply_json(&bytes).is_ok());
        let fallback = if from_backup {
            "the settings from the last start are in use"
        } else {
            "default settings are in use"
        };
        let message = match set_aside {
            Ok(()) => format!(
                "The settings file could not be read ({error}). It was kept as {} and {fallback}.",
                kept.display()
            ),
            Err(rename) => format!(
                "The settings file could not be read ({error}) or set aside ({rename}); {fallback}."
            ),
        };
        if from_backup {
            (restored, Some(message))
        } else {
            (Self::default(), Some(message))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The reader starts from the settings as they save and reads the result back
    /// (`lenient::apply`), so whatever the settings hold must read back as itself:
    /// customised values, and states only a running application has (a live layer
    /// shown, a tab that is not on the ribbon).
    #[test]
    fn settings_read_back_exactly_what_they_save() {
        use globecraft_core::layers::MapLayer;
        let mut customised = Settings::default();
        customised
            .apply_json(include_bytes!("../tests/fixtures/settings-v0.4.0.json"))
            .unwrap();
        let mut running = customised.clone();
        running.set_layer_visible(MapLayer::Flights, true);
        running.set_layer_visible(MapLayer::Radar, true);
        running.workspace.ribbon_tab = globecraft_core::commands::Tab::Help;
        running.workspace.open_right(workspace::RightPane::Legend);
        running.coordinate_display.precision = 1;
        for settings in [Settings::default(), customised, running] {
            let saved = serde_json::to_value(&settings).unwrap();
            let read: Settings = serde_json::from_value(saved).unwrap();
            assert!(read == settings, "a setting does not read back as saved");
            // A file with nothing usable changes nothing.
            let mut again = settings.clone();
            again.apply_json(br#"{"version": 1}"#).unwrap();
            again
                .apply_json(br#"{"version": 1, "ui_scale": "bad", "uncap_fps": 1}"#)
                .unwrap();
            assert!(again == settings);
        }
    }

    /// A setting is a field and its default: nothing else names it. Each plain field
    /// of the defaults is read from a file that holds only that field.
    #[test]
    fn every_plain_setting_is_read_by_its_name_alone() {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        let mut read = 0;
        for (key, value) in defaults.as_object().unwrap() {
            let changed = match value {
                serde_json::Value::Bool(on) => serde_json::Value::Bool(!on),
                _ => continue,
            };
            let mut settings = Settings::default();
            settings
                .apply_json(
                    &serde_json::to_vec(&serde_json::json!({ "version": 1, key: changed }))
                        .unwrap(),
                )
                .unwrap();
            let saved = serde_json::to_value(&settings).unwrap();
            assert_eq!(saved[key], changed, "{key} was not read");
            read += 1;
        }
        assert!(read >= 6, "only {read} switches found");
    }

    /// Every value a settings file of each release holds survives loading and saving
    /// it with this version (roadmap 10.3). Each fixture was written by that release's
    /// code itself, with many values customised; add one per release.
    #[test]
    fn settings_written_by_v0_3_0_keep_every_value() {
        keeps_every_value(include_bytes!("../tests/fixtures/settings-v0.3.0.json"));
    }
    #[test]
    fn settings_written_by_v0_4_0_keep_every_value() {
        keeps_every_value(include_bytes!("../tests/fixtures/settings-v0.4.0.json"));
    }
    fn keeps_every_value(old_bytes: &[u8]) {
        use serde_json::Value;
        fn leaves(path: &str, value: &Value, out: &mut Vec<(String, Value)>) {
            match value {
                Value::Object(map) => {
                    for (key, value) in map {
                        leaves(&format!("{path}/{key}"), value, out);
                    }
                }
                // Lists hold sets (visible layers, open panes): order is not meaning.
                Value::Array(items) => {
                    let mut items: Vec<String> = items.iter().map(Value::to_string).collect();
                    items.sort();
                    out.push((path.to_owned(), Value::from(items)));
                }
                _ => out.push((path.to_owned(), value.clone())),
            }
        }
        let old: Value = serde_json::from_slice(old_bytes).unwrap();
        let mut settings = Settings::default();
        settings.apply_json(old_bytes).unwrap();
        // Through text, as the settings writer stores it (f32 values print as written).
        let saved: Value = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        let mut old_leaves = Vec::new();
        leaves("", &old, &mut old_leaves);
        let mut new_leaves = Vec::new();
        leaves("", &saved, &mut new_leaves);
        let new_leaves: std::collections::HashMap<_, _> = new_leaves.into_iter().collect();
        // Values a later version renames on purpose: (path, old, new).
        let migrated = |path: &str, old: &Value| -> Option<Value> {
            // The ribbon's tabs were merged into Explore, Map, Live and View (2026-11).
            let renamed = [
                ("home", "explore"),
                ("layers", "map"),
                ("places", "explore"),
            ];
            // The Explore pane went into Places (2026-11).
            let panes = [("explore", "places")];
            let table: &[(&str, &str)] = match path {
                "/workspace/ribbon_tab" => &renamed,
                "/sidebar/panel" => &panes,
                _ => &[],
            };
            table
                .iter()
                .find(|(from, _)| old.as_str() == Some(from))
                .map(|(_, to)| Value::from(*to))
        };
        let lost: Vec<String> = old_leaves
            .iter()
            .filter(|(path, value)| {
                let expected = migrated(path, value).unwrap_or_else(|| value.clone());
                new_leaves.get(path) != Some(&expected)
            })
            .map(|(path, value)| format!("{path}: {value} -> {:?}", new_leaves.get(path)))
            .collect();
        assert!(
            lost.is_empty(),
            "values changed or lost:\n{}",
            lost.join("\n")
        );
    }
    #[test]
    fn exported_settings_import_and_other_files_are_refused() {
        let settings = Settings {
            ui_scale: 1.4,
            offline: true,
            kml_dir: "elsewhere".into(),
            ..Settings::default()
        };
        let imported = Settings::import(&settings.export().unwrap()).unwrap();
        assert!(imported.ui_scale == 1.4 && imported.offline && imported.kml_dir == "elsewhere");
        for other in [
            &b"{}"[..],
            br#"{"type":"FeatureCollection"}"#,
            b"[1, 2]",
            b"not json",
        ] {
            assert!(
                Settings::import(other).is_err(),
                "{}",
                String::from_utf8_lossy(other)
            );
        }
    }
    #[test]
    fn an_unreadable_settings_file_is_set_aside_not_overwritten() {
        let root =
            std::env::temp_dir().join(format!("globecraft-settings-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        let (_, problem) = Settings::load_from(&path);
        assert!(problem.is_none(), "a missing file is a first start");
        std::fs::write(&path, br#"{"ui_scale": 1.5, "map_sc"#).unwrap();
        let (settings, problem) = Settings::load_from(&path);
        assert!(settings == Settings::default());
        assert!(problem.unwrap().contains("corrupt-"));
        assert!(!path.exists());
        let kept: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            kept.len() == 1 && kept[0].starts_with("settings.json.corrupt-"),
            "{kept:?}"
        );
        std::fs::write(&path, br#"{"ui_scale": 1.5}"#).unwrap();
        let (settings, problem) = Settings::load_from(&path);
        assert!(problem.is_none());
        assert!((settings.ui_scale - 1.5).abs() < 1e-6);
        // That load kept a copy, so a file damaged later falls back to it.
        assert!(backup_path(&path).exists());
        std::fs::write(&path, br#"{"ui_scale": 1.2, "map_sc"#).unwrap();
        let (settings, problem) = Settings::load_from(&path);
        assert!((settings.ui_scale - 1.5).abs() < 1e-6);
        assert!(problem.unwrap().contains("from the last start"));
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn new_installs_start_basic_and_earlier_settings_load_advanced() {
        use experience::ExperienceLevel;
        assert_eq!(Settings::default().experience, ExperienceLevel::Basic);
        let mut upgraded = Settings::default();
        upgraded
            .apply_json(include_bytes!("../tests/fixtures/settings-v0.3.0.json"))
            .unwrap();
        assert_eq!(upgraded.experience, ExperienceLevel::Advanced);
        let mut restored = Settings::default();
        restored
            .apply_json(&serde_json::to_vec(&Settings::default()).unwrap())
            .unwrap();
        assert_eq!(restored.experience, ExperienceLevel::Basic);
        // From version 1 on, a missing level is the default, not the version 0 rule.
        let mut current = Settings::default();
        current
            .apply_json(br#"{"version": 1, "ui_scale": 1.2}"#)
            .unwrap();
        assert_eq!(current.experience, ExperienceLevel::Basic);
        assert_eq!(current.version, SETTINGS_VERSION);
    }
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
            version: SETTINGS_VERSION,
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
            screenshot_quality: screenshot::ScreenshotQuality::Pro,
            kml_dir: "documents".into(),
            ui_scale: 1.0,
            map_scale: 1.0,
            map_style: map_style::MapStyle::default(),
            uncap_fps: false,
            offline: false,
            keys_in_keychain: false,
            gpu_globe: true,
            motion_resolution: None,
            motion_resolution_relief: None,
            grid_numbers: false,
            hide_navigation_controls: false,
            state_max_altitude_km: STATE_MAX_ALTITUDE_DEFAULT_KM,
            distance_units: UnitPreference::System,
            earth_model: EarthModel::default(),
            ruler_unit: RulerUnit::Automatic,
            user_layers: Vec::new(),
            saved_sources: Default::default(),
            experience: experience::ExperienceLevel::Basic,
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
        assert_eq!(json["gpu_globe"], true);
        settings.apply_json(br#"{"gpu_globe":false}"#).unwrap();
        assert!(!settings.gpu_globe);
        let mut restored = Settings::default();
        restored
            .apply_json(&serde_json::to_vec(&settings).unwrap())
            .unwrap();
        assert!(restored.hide_navigation_controls);
    }
}
