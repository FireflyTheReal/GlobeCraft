//! The ribbon and the right pane, kept across launches (the left pane is `SidebarLayout`).
use globecraft_core::commands::Tab;
use serde::{Serialize, Serializer};

pub const DEFAULT_RIGHT_PANE_WIDTH_POINTS: f32 = 340.0;
pub const MIN_RIGHT_PANE_WIDTH_POINTS: f32 = 260.0;
pub const MAX_RIGHT_PANE_WIDTH_POINTS: f32 = 720.0;

/// Panes on the right of the globe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RightPane {
    Assistant,
    Measure,
}

impl RightPane {
    pub const ALL: [Self; 2] = [Self::Assistant, Self::Measure];
    fn code(self) -> &'static str {
        match self {
            Self::Assistant => "assistant",
            Self::Measure => "measure",
        }
    }
    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|pane| pane.code() == code)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspaceLayout {
    #[serde(serialize_with = "serialize_tab")]
    pub ribbon_tab: Tab,
    pub ribbon_minimized: bool,
    /// The right pane in view, if any.
    pub right_pane: Option<RightPane>,
    /// Right panes open as tabs, in the order they were opened.
    pub right_open: Vec<RightPane>,
    pub right_width_points: f32,
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        Self {
            ribbon_tab: Tab::Home,
            ribbon_minimized: false,
            right_pane: None,
            right_open: Vec::new(),
            right_width_points: DEFAULT_RIGHT_PANE_WIDTH_POINTS,
        }
    }
}

fn serialize_tab<S: Serializer>(tab: &Tab, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(tab.code())
}

impl WorkspaceLayout {
    /// Shows `pane`, opening its tab.
    pub fn open_right(&mut self, pane: RightPane) {
        if !self.right_open.contains(&pane) {
            self.right_open.push(pane);
        }
        self.right_pane = Some(pane);
    }
    /// Closes `pane`'s tab; the next open tab, if any, comes into view.
    pub fn close_right(&mut self, pane: RightPane) {
        self.right_open.retain(|p| *p != pane);
        if self.right_pane == Some(pane) {
            self.right_pane = self.right_open.last().copied();
        }
    }
    /// Shows `pane`, or closes it when it is already in view.
    pub fn toggle_right(&mut self, pane: RightPane) {
        if self.right_pane == Some(pane) {
            self.close_right(pane);
        } else {
            self.open_right(pane);
        }
    }

    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        if let Some(tab) = value
            .get("ribbon_tab")
            .and_then(|v| v.as_str())
            .and_then(Tab::from_code)
        {
            self.ribbon_tab = tab;
        }
        if let Some(minimized) = value.get("ribbon_minimized").and_then(|v| v.as_bool()) {
            self.ribbon_minimized = minimized;
        }
        if let Some(open) = value.get("right_open").and_then(|v| v.as_array()) {
            self.right_open = open
                .iter()
                .filter_map(|v| v.as_str().and_then(RightPane::from_code))
                .fold(Vec::new(), |mut panes, pane| {
                    if !panes.contains(&pane) {
                        panes.push(pane);
                    }
                    panes
                });
        }
        match value.get("right_pane") {
            Some(serde_json::Value::Null) => self.right_pane = None,
            Some(v) => {
                if let Some(pane) = v.as_str().and_then(RightPane::from_code) {
                    self.open_right(pane);
                }
            }
            None => {}
        }
        if self
            .right_pane
            .is_some_and(|p| !self.right_open.contains(&p))
        {
            self.right_pane = None;
        }
        if let Some(width) = value
            .get("right_width_points")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
        {
            self.right_width_points =
                (width as f32).clamp(MIN_RIGHT_PANE_WIDTH_POINTS, MAX_RIGHT_PANE_WIDTH_POINTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_roundtrips_and_keeps_tabs_consistent() {
        let mut layout = WorkspaceLayout::default();
        layout.open_right(RightPane::Measure);
        layout.open_right(RightPane::Assistant);
        layout.ribbon_tab = Tab::Live;
        layout.ribbon_minimized = true;
        let mut restored = WorkspaceLayout::default();
        restored.apply(&serde_json::to_value(&layout).unwrap());
        assert_eq!(restored, layout);
        restored.close_right(RightPane::Assistant);
        assert_eq!(restored.right_pane, Some(RightPane::Measure));
        restored.toggle_right(RightPane::Measure);
        assert_eq!(restored.right_pane, None);
        restored.apply(&serde_json::json!({"ribbon_tab": "nope", "right_width_points": 1e9, "right_open": ["x", "measure", "measure"]}));
        assert_eq!(restored.ribbon_tab, Tab::Live);
        assert_eq!(restored.right_width_points, MAX_RIGHT_PANE_WIDTH_POINTS);
        assert_eq!(restored.right_open, [RightPane::Measure]);
    }
}
