//! Stored desktop layout; invalid fields retain their last valid preference.
use serde::{Deserialize, Serialize};

pub const DEFAULT_SIDEBAR_WIDTH_POINTS: f32 = 240.0;
pub const MIN_SIDEBAR_WIDTH_POINTS: f32 = 210.0;
pub const MAX_SIDEBAR_WIDTH_POINTS: f32 = 600.0;
pub const DEFAULT_PINNED_PLACES_HEIGHT_POINTS: f32 = 300.0;
pub const MIN_PINNED_PLACES_HEIGHT_POINTS: f32 = 180.0;
pub const MAX_PINNED_PLACES_HEIGHT_POINTS: f32 = 1000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarPanel {
    /// `data` in files from before the Layers pane.
    #[serde(alias = "data")]
    Layers,
    /// `explore` in files from before the Places pane.
    #[default]
    #[serde(alias = "explore")]
    Places,
}
impl SidebarPanel {
    pub const ALL: [Self; 2] = [Self::Layers, Self::Places];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarLayout {
    pub panel: SidebarPanel,
    pub open: bool,
    pub places_pinned: bool,
    pub width_points: f32,
    pub pinned_places_height_points: f32,
}
impl Default for SidebarLayout {
    fn default() -> Self {
        Self {
            panel: SidebarPanel::Places,
            open: true,
            places_pinned: false,
            width_points: DEFAULT_SIDEBAR_WIDTH_POINTS,
            pinned_places_height_points: DEFAULT_PINNED_PLACES_HEIGHT_POINTS,
        }
    }
}
impl SidebarLayout {
    pub(super) fn apply_json(&mut self, value: &serde_json::Value) {
        super::lenient::apply(self, value, &[]);
        self.width_points = self
            .width_points
            .clamp(MIN_SIDEBAR_WIDTH_POINTS, MAX_SIDEBAR_WIDTH_POINTS);
        self.pinned_places_height_points = self.pinned_places_height_points.clamp(
            MIN_PINNED_PLACES_HEIGHT_POINTS,
            MAX_PINNED_PLACES_HEIGHT_POINTS,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn sidebar_layout_roundtrips_and_preserves_old_or_invalid_preferences() {
        for panel in SidebarPanel::ALL {
            let layout = SidebarLayout {
                panel,
                open: false,
                places_pinned: true,
                width_points: 345.0,
                pinned_places_height_points: 420.0,
            };
            let settings = Settings {
                sidebar: layout.clone(),
                ..Settings::default()
            };
            let mut restored = Settings::default();
            restored
                .apply_json(&serde_json::to_vec(&settings).unwrap())
                .unwrap();
            assert_eq!(restored.sidebar, layout);
            for bytes in [br#"{}"#.as_slice(), br#"{"sidebar":null}"#, br#"{"sidebar":42}"#, br#"{"sidebar":{"panel":"future","open":"yes","rail_visible":42,"places_pinned":[],"width_points":"wide","pinned_places_height_points":null}}"#] {
                restored.apply_json(bytes).unwrap();
                assert_eq!(restored.sidebar, layout);
            }
        }
        let mut restored = Settings::default();
        restored
            .apply_json(br#"{"sidebar":{"width_points":1e200,"pinned_places_height_points":-10}}"#)
            .unwrap();
        assert_eq!(restored.sidebar.width_points, MAX_SIDEBAR_WIDTH_POINTS);
        assert_eq!(
            restored.sidebar.pinned_places_height_points,
            MIN_PINNED_PLACES_HEIGHT_POINTS
        );
        restored.apply_json(br#"{"sidebar":{"panel":"layers","open":false,"rail_visible":"bad","places_pinned":true}}"#).unwrap();
        assert_eq!(restored.sidebar.panel, SidebarPanel::Layers);
        assert!(!restored.sidebar.open);
        assert!(
            serde_json::to_value(&restored.sidebar)
                .unwrap()
                .get("rail_visible")
                .is_none()
        );
        assert!(restored.sidebar.places_pinned);
    }
}
