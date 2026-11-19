//! Stored layout of the Places list pinned under the side pane's other tabs; invalid
//! fields retain their last valid preference. (The pane itself, its tabs, side and
//! width, is `WorkspaceLayout`; its Layers and Places tabs were a left pane of their
//! own before 2026-11-19, whose `open`, `panel` and `width_points` a file may still hold.)
use serde::{Deserialize, Serialize};

pub const DEFAULT_PINNED_PLACES_HEIGHT_POINTS: f32 = 300.0;
pub const MIN_PINNED_PLACES_HEIGHT_POINTS: f32 = 180.0;
pub const MAX_PINNED_PLACES_HEIGHT_POINTS: f32 = 1000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarLayout {
    pub places_pinned: bool,
    pub pinned_places_height_points: f32,
}
impl Default for SidebarLayout {
    fn default() -> Self {
        Self {
            places_pinned: false,
            pinned_places_height_points: DEFAULT_PINNED_PLACES_HEIGHT_POINTS,
        }
    }
}
impl SidebarLayout {
    pub(super) fn apply_json(&mut self, value: &serde_json::Value) {
        super::lenient::apply(self, value, &[]);
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
    use crate::settings::workspace::PaneTab;

    #[test]
    fn sidebar_layout_roundtrips_and_preserves_old_or_invalid_preferences() {
        let layout = SidebarLayout {
            places_pinned: true,
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
        for bytes in [
            br#"{}"#.as_slice(),
            br#"{"sidebar":null}"#,
            br#"{"sidebar":42}"#,
            br#"{"sidebar":{"places_pinned":[],"pinned_places_height_points":null}}"#,
        ] {
            restored.apply_json(bytes).unwrap();
            assert_eq!(restored.sidebar, layout);
        }
        restored
            .apply_json(br#"{"sidebar":{"pinned_places_height_points":-10}}"#)
            .unwrap();
        assert_eq!(
            restored.sidebar.pinned_places_height_points,
            MIN_PINNED_PLACES_HEIGHT_POINTS
        );
    }

    #[test]
    fn the_old_left_pane_becomes_the_side_panes_first_tabs() {
        let mut restored = Settings::default();
        restored
            .apply_json(br#"{"sidebar":{"panel":"layers","open":true},"workspace":{"right_open":["measure"],"right_pane":null}}"#)
            .unwrap();
        let workspace = &restored.workspace;
        assert_eq!(
            workspace.right_open,
            [PaneTab::Layers, PaneTab::Places, PaneTab::Measure]
        );
        assert_eq!(workspace.right_pane, Some(PaneTab::Layers));
        // Closed, it stays closed; the right pane's tab stays in view.
        let mut restored = Settings::default();
        restored
            .apply_json(br#"{"sidebar":{"open":false},"workspace":{"right_open":["legend"],"right_pane":"legend"}}"#)
            .unwrap();
        assert_eq!(restored.workspace.right_open, [PaneTab::Legend]);
        assert_eq!(restored.workspace.right_pane, Some(PaneTab::Legend));
        // A file written since keeps its tabs as they are.
        let saved = serde_json::to_vec(&restored).unwrap();
        let mut again = Settings::default();
        again.apply_json(&saved).unwrap();
        assert_eq!(again.workspace, restored.workspace);
    }
}
