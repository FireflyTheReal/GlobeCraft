//! Keyboard shortcuts of commands: the defaults, the user's, and running them.
use crate::app::App;
use eframe::egui;
use serde_json::Value;
use serde_json::json;

/// The keyboard shortcut of a command, as text for tooltips and menus.
pub(in crate::app) fn shortcut_text(id: &str) -> String {
    tool_shortcut_text(id)
}

/// The keyboard shortcut of a tool (see [`crate::app::tools::tool_key`]), as text.
pub(in crate::app) fn tool_shortcut_text(key: &str) -> String {
    tool_shortcut(key)
        .map(|s| keys_label(&s))
        .unwrap_or_default()
}

/// Keys as the interface shows them (⌘⇧E on macOS, Ctrl+Shift+E elsewhere).
fn keys_label(shortcut: &egui::KeyboardShortcut) -> String {
    shortcut.format(&egui::ModifierNames::NAMES, cfg!(target_os = "macos"))
}

/// The keyboard shortcut of a command: the user's, or the default.
pub(in crate::app) fn shortcut(id: &str) -> Option<egui::KeyboardShortcut> {
    tool_shortcut(id)
}

/// The keyboard shortcut of a tool: the user's, or the default.
fn tool_shortcut(key: &str) -> Option<egui::KeyboardShortcut> {
    crate::app::tools::user_shortcut(key).unwrap_or_else(|| default_shortcut(key))
}

/// The keyboard shortcut a command has at installation.
fn default_shortcut(id: &str) -> Option<egui::KeyboardShortcut> {
    use egui::{Key, Modifiers};
    let (modifiers, key) = match id {
        "places.open" => (Modifiers::COMMAND, Key::O),
        "places.save" => (Modifiers::COMMAND, Key::S),
        "places.saveAs" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::S),
        "file.screenshot" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::E),
        "app.quit" => (Modifiers::COMMAND, Key::Q),
        "edit.undo" => (Modifiers::COMMAND, Key::Z),
        "edit.redo" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
        "ui.settings" => (Modifiers::COMMAND, Key::Comma),
        "places.addAtCenter" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::P),
        "places.addFolder" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::N),
        "ui.leftPane" => (Modifiers::COMMAND | Modifiers::ALT, Key::B),
        "ui.commandSearch" => (Modifiers::COMMAND, Key::K),
        "ui.pane.assistant" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::A),
        "ui.ribbon" => (Modifiers::CTRL, Key::F1),
        "search.place" => (Modifiers::COMMAND, Key::F),
        "ui.focusNext" => (Modifiers::NONE, Key::F6),
        "ui.focusPrevious" => (Modifiers::SHIFT, Key::F6),
        _ => return None,
    };
    Some(egui::KeyboardShortcut::new(modifiers, key))
}

/// Commands with keyboard shortcuts that the window handles (macOS routes some through its
/// native menu instead).
pub(in crate::app) const SHORTCUT_COMMANDS: &[&str] = &[
    "places.open",
    "places.save",
    "places.saveAs",
    "file.screenshot",
    "app.quit",
    "ui.settings",
    "places.addAtCenter",
    "places.addFolder",
    "ui.leftPane",
    "ui.commandSearch",
    "ui.pane.assistant",
    "ui.ribbon",
    "search.place",
    "ui.focusNext",
    "ui.focusPrevious",
];

impl App {
    /// Gives `tool` the keys `keys` (`None` restores the default, empty removes it). A tool
    /// that had these keys loses them, so each keys run one tool.
    pub(super) fn set_shortcut(&mut self, tool: &str, keys: Option<&str>) -> Result<Value, String> {
        let wanted = match keys {
            None => default_shortcut(tool),
            Some("") => None,
            Some(text) => {
                let keys = crate::app::tools::parse_keys(text).ok_or("unknown keys")?;
                if !crate::app::tools::allowed(&keys) {
                    return Err("use Ctrl, Alt or Command with the key, or a function key".into());
                }
                Some(keys)
            }
        };
        let mut previous = None;
        if let Some(keys) = wanted {
            let mut holders: Vec<String> =
                SHORTCUT_COMMANDS.iter().map(|id| id.to_string()).collect();
            holders.extend(self.settings.workspace.shortcuts.keys().cloned());
            holders.sort();
            holders.dedup();
            for other in holders {
                if other != tool && tool_shortcut(&other) == Some(keys) {
                    self.settings
                        .workspace
                        .shortcuts
                        .insert(other.clone(), String::new());
                    previous = Some(other);
                }
            }
        }
        let shortcuts = &mut self.settings.workspace.shortcuts;
        if wanted == default_shortcut(tool) {
            shortcuts.remove(tool);
        } else {
            shortcuts.insert(
                tool.to_string(),
                wanted
                    .map(|k| crate::app::tools::keys_text(&k))
                    .unwrap_or_default(),
            );
        }
        crate::app::tools::sync_shortcuts(shortcuts);
        if let (Some(keys), Some(previous)) = (wanted, &previous) {
            self.status = self.lang.format(
                "shortcut-taken",
                &[
                    ("keys", keys_label(&keys).into()),
                    (
                        "tool",
                        crate::app::ui::ribbon_tools::tool_name(self, tool).into(),
                    ),
                    (
                        "previous",
                        crate::app::ui::ribbon_tools::tool_name(self, previous).into(),
                    ),
                ],
            );
        }
        Ok(json!({
            "keys": wanted.map(|k| crate::app::tools::keys_text(&k)),
            "removed_from": previous,
        }))
    }

    /// Commands bound to keyboard shortcuts, run when their keys are pressed.
    pub(in crate::app) fn command_shortcuts(&mut self, ctx: &egui::Context) {
        crate::app::tools::sync_shortcuts(&self.settings.workspace.shortcuts);
        let mut bound: Vec<(String, egui::KeyboardShortcut)> = SHORTCUT_COMMANDS
            .iter()
            .filter_map(|id| shortcut(id).map(|s| (id.to_string(), s)))
            .collect();
        for (key, keys) in crate::app::tools::user_shortcuts() {
            if !SHORTCUT_COMMANDS.contains(&key.as_str()) {
                bound.push((key, keys));
            }
        }
        // Keys with more modifiers first: ⌘⇧S must not run ⌘S's command.
        bound.sort_by_key(|(key, keys)| {
            let m = keys.modifiers;
            (
                std::cmp::Reverse(
                    u8::from(m.command) + u8::from(m.ctrl) + u8::from(m.alt) + u8::from(m.shift),
                ),
                key.clone(),
            )
        });
        for (key, keys) in bound {
            #[cfg(target_os = "macos")]
            if self.chrome.native_menu.is_some() && crate::app::menu::native_shortcut(&key) {
                continue;
            }
            if ctx.input_mut(|input| input.consume_shortcut(&keys))
                && let Some((id, params)) = crate::app::tools::parse_tool(&key)
            {
                self.run_reporting(&id, params);
            }
        }
    }
}
