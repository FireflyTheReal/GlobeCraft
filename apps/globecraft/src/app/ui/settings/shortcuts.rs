//! Settings › Files & shortcuts: every tool with a keyboard shortcut, to change it, and
//! the reset of the ribbon's tools.
use crate::app::App;
use crate::app::commands::{SHORTCUT_COMMANDS, tool_shortcut_text};
use crate::app::ui::ribbon_tools::tool_name;
use eframe::egui;
use serde_json::{Value, json};

impl App {
    pub(super) fn settings_shortcuts(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        let mut tools: Vec<String> = SHORTCUT_COMMANDS.iter().map(|id| id.to_string()).collect();
        tools.extend(self.settings.workspace.shortcuts.keys().cloned());
        tools.sort();
        tools.dedup();
        let mut rows: Vec<(String, String)> = tools
            .into_iter()
            .map(|tool| (tool_name(self, &tool), tool))
            .collect();
        rows.sort_by_key(|row| row.0.to_lowercase());
        let mut edit = None;
        egui::Grid::new("settings-shortcuts")
            .num_columns(3)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for (name, tool) in &rows {
                    ui.label(name);
                    let keys = tool_shortcut_text(tool);
                    if keys.is_empty() {
                        ui.weak(lg.text("shortcut-dialog-none"));
                    } else {
                        ui.monospace(keys);
                    }
                    if ui
                        .small_button(lg.text("settings-shortcut-change"))
                        .clicked()
                    {
                        edit = Some(tool.clone());
                    }
                    ui.end_row();
                }
            });
        ui.weak(lg.text("settings-shortcuts-help"));
        if ui.button(lg.text("cmd-ui-ribbon-reset-tools")).clicked() {
            self.run_reporting("ui.ribbon.resetTools", Value::Null);
        }
        if let Some(tool) = edit {
            self.run_reporting("ui.shortcut.edit", json!({ "tool": tool }));
        }
    }
}
