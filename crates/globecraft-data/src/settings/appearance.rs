//! Persisted UI appearance and installed-font family preferences.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follows macOS, Windows or the desktop's light or dark appearance (HIG: Dark Mode).
    #[default]
    System,
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
    /// Fly to places without the flight and stop the globe from coasting after a drag.
    /// macOS's own Reduce Motion turns this on as well.
    pub reduce_motion: bool,
    /// Interface icons in one colour (ink and accent) instead of their own colours.
    pub mono_icons: bool,
    /// The monochrome icons' colour; `None` is the system's accent colour.
    pub mono_icon_color: Option<super::map_style::Rgba>,
}
impl AppearancePreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        super::lenient::apply(self, value, &[]);
    }
}
