//! Persisted UI appearance and installed-font family preferences.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    #[default]
    Dark,
    Light,
}
impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Dark, Self::Light];
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearancePreferences {
    pub theme: ThemePreference,
    pub ui_font: String,
    pub map_font: String,
}
impl AppearancePreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        super::lenient::apply(self, value, &[]);
    }
}
