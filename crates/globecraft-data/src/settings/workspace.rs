//! The ribbon and the side pane, kept across launches.
use globecraft_core::commands::Tab;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

/// The tabs of the side pane (one pane beside the globe since 2026-11-19; the Layers
/// and Places tabs were a pane of their own on the left before).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneTab {
    Assistant,
    Measure,
    Legend,
    /// Places, sky and planets (the Explorer).
    Explorer,
    Layers,
    Places,
    /// The selected flight's details (a card over the globe before).
    Flight,
    /// The selected satellite's details.
    Satellite,
    /// What's here: the point pressed on the globe.
    Here,
}

impl PaneTab {
    pub const ALL: [Self; 9] = [
        Self::Assistant,
        Self::Measure,
        Self::Legend,
        Self::Explorer,
        Self::Layers,
        Self::Places,
        Self::Flight,
        Self::Satellite,
        Self::Here,
    ];
    pub fn code(self) -> &'static str {
        match self {
            Self::Assistant => "assistant",
            Self::Measure => "measure",
            Self::Legend => "legend",
            Self::Explorer => "explorer",
            Self::Layers => "layers",
            Self::Places => "places",
            Self::Flight => "flight",
            Self::Satellite => "satellite",
            Self::Here => "here",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|pane| pane.code() == code)
    }
    /// Tabs showing one selected thing: they open with the selection and close with it.
    pub fn is_details(self) -> bool {
        matches!(self, Self::Flight | Self::Satellite | Self::Here)
    }
}

/// Which side of the globe the side pane is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneSide {
    Left,
    #[default]
    Right,
}

impl PaneSide {
    pub fn code(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceLayout {
    #[serde(serialize_with = "serialize_tab", deserialize_with = "deserialize_tab")]
    pub ribbon_tab: Tab,
    pub ribbon_minimized: bool,
    /// The side pane's tab in view; `None`: the pane is hidden (its tabs stay).
    /// (`right_*`: named when the pane was on the right only; kept for saved files.)
    pub right_pane: Option<PaneTab>,
    /// The side pane's tabs, in their order.
    pub right_open: Vec<PaneTab>,
    pub right_width_points: f32,
    /// The side of the globe the pane is on.
    pub pane_side: PaneSide,
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
            ribbon_tab: Tab::Explore,
            ribbon_minimized: false,
            right_pane: Some(PaneTab::Places),
            right_open: vec![PaneTab::Layers, PaneTab::Places],
            right_width_points: DEFAULT_RIGHT_PANE_WIDTH_POINTS,
            pane_side: PaneSide::Right,
            ribbon_toggled: Vec::new(),
            quick_tools: DEFAULT_QUICK_TOOLS.map(String::from).to_vec(),
            shortcuts: BTreeMap::new(),
        }
    }
}

fn serialize_tab<S: Serializer>(tab: &Tab, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(tab.code())
}

fn deserialize_tab<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Tab, D::Error> {
    super::lenient::coded(deserializer, Tab::from_code)
}

/// Fields `apply` reads by its own rules: a file's tab may carry an older name, the
/// panes depend on each other, and the lists are filtered entry by entry.
const OWN_RULES: [&str; 5] = [
    "ribbon_tab",
    "right_pane",
    "right_open",
    "ribbon_toggled",
    "quick_tools",
];

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
    pub fn open_right(&mut self, pane: PaneTab) {
        if !self.right_open.contains(&pane) {
            self.right_open.push(pane);
        }
        self.right_pane = Some(pane);
    }
    /// Opens `pane`'s tab without taking the view from a pane already shown.
    pub fn add_right(&mut self, pane: PaneTab) {
        if !self.right_open.contains(&pane) {
            self.right_open.push(pane);
        }
        if self.right_pane.is_none() {
            self.right_pane = Some(pane);
        }
    }
    /// Whether `pane` has a tab.
    pub fn right_is_open(&self, pane: PaneTab) -> bool {
        self.right_open.contains(&pane)
    }
    /// Closes `pane`'s tab; the next open tab, if any, comes into view.
    pub fn close_right(&mut self, pane: PaneTab) {
        self.right_open.retain(|p| *p != pane);
        if self.right_pane == Some(pane) {
            self.right_pane = self.right_open.last().copied();
        }
    }
    /// Moves `pane`'s tab to position `to` (clamped to the tabs).
    pub fn move_tab(&mut self, pane: PaneTab, to: usize) {
        if let Some(from) = self.right_open.iter().position(|p| *p == pane) {
            self.right_open.remove(from);
            self.right_open.insert(to.min(self.right_open.len()), pane);
        }
    }
    /// Hides the pane (keeping its tabs), or shows it with the last tab or Places.
    pub fn toggle_pane(&mut self) {
        if self.right_pane.is_some() {
            self.right_pane = None;
        } else {
            self.open_right(self.right_open.last().copied().unwrap_or(PaneTab::Places));
        }
    }
    /// Shows `pane`, or closes it when it is already in view.
    pub fn toggle_right(&mut self, pane: PaneTab) {
        if self.right_pane == Some(pane) {
            self.close_right(pane);
        } else {
            self.open_right(pane);
        }
    }

    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        super::lenient::apply(self, value, &OWN_RULES);
        self.right_width_points = self
            .right_width_points
            .clamp(MIN_RIGHT_PANE_WIDTH_POINTS, MAX_RIGHT_PANE_WIDTH_POINTS);
        if let Some(tab) = value
            .get("ribbon_tab")
            .and_then(|v| v.as_str())
            .and_then(Tab::ribbon_from_code)
        {
            self.ribbon_tab = tab;
        }
        if let Some(open) = value.get("right_open").and_then(|v| v.as_array()) {
            self.right_open = open
                .iter()
                .filter_map(|v| v.as_str().and_then(PaneTab::from_code))
                .fold(Vec::new(), |mut panes, pane| {
                    if !panes.contains(&pane) {
                        panes.push(pane);
                    }
                    panes
                });
        }
        // The selection a details tab shows is not kept across launches.
        self.right_open.retain(|pane| !pane.is_details());
        match value.get("right_pane") {
            Some(serde_json::Value::Null) => self.right_pane = None,
            Some(v) => {
                if let Some(pane) = v
                    .as_str()
                    .and_then(PaneTab::from_code)
                    .filter(|pane| !pane.is_details())
                {
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
        let mut layout = WorkspaceLayout {
            right_open: Vec::new(),
            right_pane: None,
            ..WorkspaceLayout::default()
        };
        layout.open_right(PaneTab::Measure);
        layout.open_right(PaneTab::Assistant);
        layout.ribbon_tab = Tab::Live;
        layout.ribbon_minimized = true;
        let mut restored = WorkspaceLayout::default();
        restored.apply(&serde_json::to_value(&layout).unwrap());
        assert_eq!(restored, layout);
        restored.close_right(PaneTab::Assistant);
        assert_eq!(restored.right_pane, Some(PaneTab::Measure));
        restored.toggle_right(PaneTab::Measure);
        assert_eq!(restored.right_pane, None);
        restored.apply(&serde_json::json!({"ribbon_tab": "nope", "right_width_points": 1e9, "right_open": ["x", "measure", "measure"]}));
        assert_eq!(restored.ribbon_tab, Tab::Live);
        assert_eq!(restored.right_width_points, MAX_RIGHT_PANE_WIDTH_POINTS);
        assert_eq!(restored.right_open, [PaneTab::Measure]);
        // Tabs move; a selection's details are not kept across launches.
        restored.open_right(PaneTab::Layers);
        restored.open_right(PaneTab::Flight);
        restored.move_tab(PaneTab::Flight, 0);
        assert_eq!(
            restored.right_open,
            [PaneTab::Flight, PaneTab::Measure, PaneTab::Layers]
        );
        restored.apply(&serde_json::to_value(restored.clone()).unwrap());
        assert_eq!(restored.right_open, [PaneTab::Measure, PaneTab::Layers]);
        assert_eq!(restored.right_pane, None);
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
