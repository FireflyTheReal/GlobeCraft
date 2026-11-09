//! The ribbon and the right pane, kept across launches (the left pane is `SidebarLayout`).
use globecraft_core::commands::Tab;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

pub const DEFAULT_RIGHT_PANE_WIDTH_POINTS: f32 = 340.0;
pub const MIN_RIGHT_PANE_WIDTH_POINTS: f32 = 260.0;
pub const MAX_RIGHT_PANE_WIDTH_POINTS: f32 = 720.0;
/// The quick-access row holds at most this many tools.
pub const MAX_QUICK_TOOLS: usize = 10;
/// Tools in the quick-access row of a new installation.
pub const DEFAULT_QUICK_TOOLS: [&str; 3] = ["edit.undo", "edit.redo", "file.screenshot"];
/// Caps on the saved lists, so a damaged file cannot grow them without bound.
const MAX_SAVED_TOOLS: usize = 500;
const MAX_TOOL_KEY_LEN: usize = 200;

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
    /// Ribbon tools whose place differs from the default, as `group/tool`: a tool shown on
    /// the ribbon by default and hidden by the user, or one the user pinned to it.
    pub ribbon_toggled: Vec<String>,
    /// Tools in the quick-access row, in order. A tool is a command id, followed by a space
    /// and its parameters as JSON when it has any.
    pub quick_tools: Vec<String>,
    /// Keyboard shortcuts the user changed: tool → keys (`command+shift+E`), empty for none.
    pub shortcuts: BTreeMap<String, String>,
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        Self {
            ribbon_tab: Tab::Home,
            ribbon_minimized: false,
            right_pane: None,
            right_open: Vec::new(),
            right_width_points: DEFAULT_RIGHT_PANE_WIDTH_POINTS,
            ribbon_toggled: Vec::new(),
            quick_tools: DEFAULT_QUICK_TOOLS.map(String::from).to_vec(),
            shortcuts: BTreeMap::new(),
        }
    }
}

fn serialize_tab<S: Serializer>(tab: &Tab, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(tab.code())
}

fn valid_key(key: &str) -> bool {
    !key.trim().is_empty() && key.len() <= MAX_TOOL_KEY_LEN
}

/// The distinct valid strings of a JSON array, in order, at most `max`.
fn string_list(value: &serde_json::Value, max: usize) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for item in value.as_array()? {
        if let Some(s) = item.as_str().filter(|s| valid_key(s))
            && !out.iter().any(|o| o == s)
            && out.len() < max
        {
            out.push(s.to_string());
        }
    }
    Some(out)
}

impl WorkspaceLayout {
    /// Pins `key` to the quick-access row, or takes it off.
    pub fn set_quick_tool(&mut self, key: &str, on: bool) {
        let present = self.quick_tools.iter().any(|k| k == key);
        if on && !present && self.quick_tools.len() < MAX_QUICK_TOOLS {
            self.quick_tools.push(key.to_string());
        } else if !on {
            self.quick_tools.retain(|k| k != key);
        }
    }
    /// Marks `group/tool` as moved from its default place, or back.
    pub fn set_ribbon_toggled(&mut self, entry: &str, toggled: bool) {
        self.ribbon_toggled.retain(|k| k != entry);
        if toggled && self.ribbon_toggled.len() < MAX_SAVED_TOOLS {
            self.ribbon_toggled.push(entry.to_string());
        }
    }
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
        if let Some(list) = value
            .get("ribbon_toggled")
            .and_then(|v| string_list(v, MAX_SAVED_TOOLS))
        {
            self.ribbon_toggled = list;
        }
        if let Some(list) = value
            .get("quick_tools")
            .and_then(|v| string_list(v, MAX_QUICK_TOOLS))
        {
            self.quick_tools = list;
        }
        if let Some(map) = value.get("shortcuts").and_then(|v| v.as_object()) {
            self.shortcuts = map
                .iter()
                .filter(|(tool, _)| valid_key(tool))
                .filter_map(|(tool, keys)| {
                    let keys = keys.as_str().filter(|k| k.len() <= MAX_TOOL_KEY_LEN)?;
                    Some((tool.clone(), keys.to_string()))
                })
                .take(MAX_SAVED_TOOLS)
                .collect();
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

    #[test]
    fn tools_and_shortcuts_roundtrip_and_reject_damage() {
        let mut layout = WorkspaceLayout::default();
        assert_eq!(layout.quick_tools, DEFAULT_QUICK_TOOLS);
        layout.set_quick_tool("ui.legend", true);
        layout.set_quick_tool("edit.undo", false);
        layout.set_quick_tool("ui.legend", true);
        layout.set_ribbon_toggled("home.navigate/camera.zoomIn", true);
        layout
            .shortcuts
            .insert("ui.legend".into(), "command+L".into());
        let mut restored = WorkspaceLayout::default();
        restored.apply(&serde_json::to_value(&layout).unwrap());
        assert_eq!(restored, layout);
        assert_eq!(
            restored.quick_tools,
            ["edit.redo", "file.screenshot", "ui.legend"]
        );
        restored.apply(&serde_json::json!({
            "quick_tools": ["a", "a", "", 3, "b"],
            "ribbon_toggled": "nope",
            "shortcuts": {"x": 1, "y": "", "": "command+K"},
        }));
        assert_eq!(restored.quick_tools, ["a", "b"]);
        assert_eq!(restored.ribbon_toggled, ["home.navigate/camera.zoomIn"]);
        assert_eq!(restored.shortcuts.len(), 1);
        assert_eq!(restored.shortcuts["y"], "");
    }
}
