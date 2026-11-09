//! Tools: a command with its parameters, as the ribbon's group menus, the quick-access row
//! and keyboard shortcuts know it. The user's pins and shortcuts are saved in the workspace
//! settings; this module turns them into what the interface asks for each frame.
use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

/// A tool's key: the command id, then a space and its parameters as JSON when it has any.
pub(in crate::app) fn tool_key(id: &str, params: &Value) -> String {
    let empty = params.is_null() || params.as_object().is_some_and(|o| o.is_empty());
    if empty {
        id.to_string()
    } else {
        format!("{id} {params}")
    }
}

/// The command id and parameters of a tool key.
pub(in crate::app) fn parse_tool(key: &str) -> Option<(String, Value)> {
    match key.split_once(' ') {
        None => (!key.is_empty()).then(|| (key.to_string(), Value::Null)),
        Some((id, json)) => Some((id.to_string(), serde_json::from_str(json).ok()?)),
    }
}

/// Keys as saved in the settings: `command+shift+E`. `command` is ⌘ on macOS and Ctrl
/// elsewhere; `ctrl` is the Control key where it differs from `command` (macOS).
pub(in crate::app) fn keys_text(shortcut: &KeyboardShortcut) -> String {
    let m = shortcut.modifiers;
    let mut parts: Vec<&str> = Vec::new();
    if m.command || m.mac_cmd {
        parts.push("command");
    } else if m.ctrl {
        parts.push("ctrl");
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    parts.push(shortcut.logical_key.name());
    parts.join("+")
}

/// Keys saved by [`keys_text`], or `None` when they don't name a key.
pub(in crate::app) fn parse_keys(text: &str) -> Option<KeyboardShortcut> {
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key = Key::from_name(parts.pop()?)?;
    let mut modifiers = Modifiers::NONE;
    for part in parts {
        modifiers |= match part {
            "command" => Modifiers::COMMAND,
            "ctrl" => Modifiers::CTRL,
            "alt" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            _ => return None,
        };
    }
    Some(KeyboardShortcut::new(modifiers, key))
}

/// Whether keys may become a shortcut: a letter, digit or other key with Ctrl, Alt or ⌘,
/// or a function key alone. Plain keys stay with typing and the globe.
pub(in crate::app) fn allowed(shortcut: &KeyboardShortcut) -> bool {
    let m = shortcut.modifiers;
    let function = matches!(
        shortcut.logical_key,
        Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
    );
    function || m.command || m.mac_cmd || m.ctrl || m.alt
}

/// The keys of a key event as a shortcut, in the form [`parse_keys`] reads back.
pub(in crate::app) fn pressed(key: Key, m: Modifiers) -> KeyboardShortcut {
    let mut modifiers = Modifiers::NONE;
    if m.command || m.mac_cmd {
        modifiers |= Modifiers::COMMAND;
    }
    if m.ctrl && cfg!(target_os = "macos") {
        modifiers |= Modifiers::CTRL;
    }
    if m.alt {
        modifiers |= Modifiers::ALT;
    }
    if m.shift {
        modifiers |= Modifiers::SHIFT;
    }
    KeyboardShortcut::new(modifiers, key)
}

struct Overrides {
    /// The settings these were read from, to notice changes.
    source: BTreeMap<String, String>,
    keys: HashMap<String, Option<KeyboardShortcut>>,
}

thread_local! {
    /// The user's shortcuts. Thread-local: the interface runs on one thread, and tests that
    /// change shortcuts don't see each other's.
    static OVERRIDES: RefCell<Overrides> = RefCell::new(Overrides {
        source: BTreeMap::new(),
        keys: HashMap::new(),
    });
}

/// Takes the user's shortcuts from the settings when they changed.
pub(in crate::app) fn sync_shortcuts(saved: &BTreeMap<String, String>) {
    OVERRIDES.with_borrow_mut(|o| {
        if &o.source == saved {
            return;
        }
        o.source = saved.clone();
        o.keys = saved
            .iter()
            .filter_map(|(tool, keys)| {
                if keys.is_empty() {
                    Some((tool.clone(), None))
                } else {
                    parse_keys(keys).map(|s| (tool.clone(), Some(s)))
                }
            })
            .collect();
    });
}

/// The user's shortcut of a tool: `Some(None)` when they removed it, `None` when unchanged.
pub(in crate::app) fn user_shortcut(key: &str) -> Option<Option<KeyboardShortcut>> {
    OVERRIDES.with_borrow(|o| o.keys.get(key).copied())
}

/// Tools the user gave keys to.
pub(in crate::app) fn user_shortcuts() -> Vec<(String, KeyboardShortcut)> {
    OVERRIDES.with_borrow(|o| {
        o.keys
            .iter()
            .filter_map(|(tool, keys)| keys.map(|k| (tool.clone(), k)))
            .collect()
    })
}

/// Ribbon tools that are in their group's menu only until the user pins them: the
/// ribbon shows the tools used most, and the menu all of them.
pub(in crate::app) const MENU_ONLY: &[(&str, &str)] = &[
    ("home.navigate", "camera.zoomIn"),
    ("home.navigate", "camera.zoomOut"),
    ("home.measure", "edit.undo"),
    ("home.measure", "edit.redo"),
    ("home.measure", "ruler.closeShape"),
];

/// Tools a group's menu offers beyond its ribbon buttons; pinned, they join the ribbon.
pub(in crate::app) const EXTRAS: &[(&str, &str)] = &[
    ("home.navigate", "camera.topDown"),
    ("home.measure", "ruler.profile"),
    ("home.capture", "view.copyLink"),
    ("home.places", "places.save"),
    ("home.places", "places.saveAs"),
    ("live.objects", "flight.clear"),
    ("live.objects", "satellite.clear"),
    ("view.window", "ui.minimize"),
    ("view.window", "ui.tileDiagnostics"),
    ("view.panes", "ui.timeline"),
    ("settings.preferences", "ui.ribbon.resetTools"),
];

/// Whether a tool is on the ribbon by default.
pub(in crate::app) fn pinned_by_default(group: &str, key: &str) -> bool {
    let listed = |list: &[(&str, &str)]| list.iter().any(|&(g, k)| g == group && k == key);
    !listed(MENU_ONLY) && !listed(EXTRAS)
}

/// The settings entry of a tool in a group.
pub(in crate::app) fn group_entry(group: &str, key: &str) -> String {
    format!("{group}/{key}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_and_tools_read_back() {
        for text in ["command+shift+E", "ctrl+F1", "alt+3", "command+Comma", "F5"] {
            let keys = parse_keys(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(keys_text(&keys), text);
        }
        assert!(parse_keys("hyper+E").is_none());
        assert!(parse_keys("command+").is_none());
        assert!(allowed(&parse_keys("F5").unwrap()));
        assert!(!allowed(&parse_keys("shift+E").unwrap()));
        let params = json!({"layer": "radar"});
        let key = tool_key("layer.show", &params);
        assert_eq!(key, r#"layer.show {"layer":"radar"}"#);
        assert_eq!(parse_tool(&key), Some(("layer.show".into(), params)));
        assert_eq!(tool_key("ui.legend", &json!({})), "ui.legend");
        assert!(pinned_by_default("home.navigate", "search.place"));
        assert!(!pinned_by_default("home.navigate", "camera.zoomIn"));
    }

    #[test]
    fn user_shortcuts_follow_the_settings() {
        let mut saved = BTreeMap::new();
        saved.insert("ui.legend".to_string(), "command+L".to_string());
        saved.insert("ui.settings".to_string(), String::new());
        saved.insert("bad".to_string(), "nonsense+".to_string());
        sync_shortcuts(&saved);
        assert_eq!(
            user_shortcut("ui.legend"),
            Some(Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::L)))
        );
        assert_eq!(user_shortcut("ui.settings"), Some(None));
        assert_eq!(user_shortcut("bad"), None);
        assert_eq!(user_shortcuts().len(), 1);
        sync_shortcuts(&BTreeMap::new());
        assert_eq!(user_shortcut("ui.legend"), None);
    }
}
