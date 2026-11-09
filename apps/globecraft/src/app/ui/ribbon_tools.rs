//! Ribbon group menus, as in Fusion 360: a group's title opens a list of all its tools with
//! their shortcuts; each row's ⋮ menu pins the tool to the ribbon or to the quick-access
//! row, or changes its keyboard shortcut (in a small dialog that listens for the keys).
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use crate::app::tools::{
    EXTRAS, allowed, group_entry, keys_text, parse_tool, pinned_by_default, pressed, tool_key,
};
use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

/// Width of a group menu's tool rows.
const ROW_W: f32 = 250.0;
const ROW_H: f32 = 26.0;
const MORE_W: f32 = 22.0;

/// A tool of a ribbon group, as its menu lists it.
pub(in crate::app) struct ToolRow {
    pub key: String,
    pub id: String,
    pub params: Value,
    pub icon: String,
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
}

/// The group being drawn and the tools it has listed so far this frame.
#[derive(Default)]
pub(in crate::app) struct RibbonTools {
    group: Option<&'static str>,
    rows: Vec<ToolRow>,
    /// The tool whose keyboard shortcut the dialog is changing.
    pub(in crate::app) capture: Option<String>,
}

impl RibbonTools {
    pub(super) fn begin(&mut self, group: &'static str) {
        self.group = Some(group);
        self.rows.clear();
    }
    pub(super) fn end(&mut self) -> Vec<ToolRow> {
        self.group = None;
        std::mem::take(&mut self.rows)
    }
}

impl App {
    /// Whether a tool of a group is on the ribbon.
    pub(in crate::app) fn tool_pinned(&self, group: &str, key: &str) -> bool {
        let toggled = self
            .settings
            .workspace
            .ribbon_toggled
            .iter()
            .any(|e| *e == group_entry(group, key));
        pinned_by_default(group, key) != toggled
    }

    /// Lists a ribbon tool in the group being drawn; `false` when the user keeps it in the
    /// group's menu only, so the ribbon leaves it out.
    pub(super) fn list_tool(
        &mut self,
        id: &str,
        params: &Value,
        icon: &str,
        label: &str,
        enabled: bool,
        checked: bool,
    ) -> bool {
        let Some(group) = self.ribbon_tools.group else {
            return true;
        };
        let key = tool_key(id, params);
        let shown = self.tool_pinned(group, &key);
        if !self.ribbon_tools.rows.iter().any(|r| r.key == key) {
            self.ribbon_tools.rows.push(ToolRow {
                key,
                id: id.to_string(),
                params: params.clone(),
                icon: icon.to_string(),
                label: label.replace('\n', " "),
                enabled,
                checked,
            });
        }
        shown
    }

    /// The tools a group's menu offers beyond its ribbon buttons: those pinned are drawn
    /// as small buttons at the group's end, the others only listed.
    pub(super) fn group_extras(&mut self, ui: &mut Ui, group: &'static str) {
        let mut pinned = Vec::new();
        for &(g, id) in EXTRAS {
            if g != group {
                continue;
            }
            if self.tool_pinned(group, id) {
                pinned.push(id);
            } else {
                let spec = globecraft_core::commands::find(id);
                let state = self.command_state(id, &Value::Null);
                let label = spec.map_or(id, |s| self.lang.text(s.label));
                self.list_tool(
                    id,
                    &Value::Null,
                    spec.map_or("", |s| s.icon),
                    label,
                    state.enabled,
                    state.checked,
                );
            }
        }
        for chunk in pinned.chunks(3) {
            super::widgets::col(ui, |ui| {
                for id in chunk {
                    super::widgets::small(ui, self, id, Value::Null);
                }
            });
        }
    }

    /// A group menu: the tools on the ribbon in their ribbon order, then, after a
    /// separator, those kept in the menu only.
    pub(super) fn group_menu(&mut self, ui: &mut Ui, group: &'static str, rows: &[ToolRow]) {
        ui.set_min_width(ROW_W);
        let (shown, menu_only): (Vec<&ToolRow>, Vec<&ToolRow>) = rows
            .iter()
            .partition(|row| self.tool_pinned(group, &row.key));
        for row in &shown {
            self.tool_row(ui, group, row);
        }
        if !shown.is_empty() && !menu_only.is_empty() {
            ui.separator();
        }
        for row in &menu_only {
            self.tool_row(ui, group, row);
        }
    }

    /// One tool: icon, name and shortcut, and a ⋮ at the end while the row is hovered
    /// (or its menu open).
    fn tool_row(&mut self, ui: &mut Ui, group: &'static str, row: &ToolRow) {
        let t = Tokens::get(ui.ctx());
        let (rect, resp) = ui.allocate_exact_size(
            vec2(ROW_W, ROW_H),
            if row.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                row.enabled,
                row.checked,
                &row.label,
            )
        });
        let more_rect = Rect::from_min_max(pos2(rect.max.x - MORE_W, rect.min.y), rect.max);
        let open_id = ui.id().with(("tool_more_open", &row.key));
        let more_open = ui.data(|d| d.get_temp::<bool>(open_id).unwrap_or(false));
        let row_hovered = ui.rect_contains_pointer(rect);
        let fill = if row.enabled && resp.is_pointer_button_down_on() {
            Some(t.pressed)
        } else if row_hovered || more_open {
            Some(t.hover)
        } else if row.checked {
            Some(t.checked)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
        }
        let text = if row.enabled { t.text } else { t.text_disabled };
        let ic = Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(18.0, 18.0));
        icons::paint(
            ui.painter(),
            ic,
            &row.icon,
            if row.enabled { t.icon } else { text },
            t.accent,
        );
        ui.painter().text(
            pos2(rect.min.x + 32.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &row.label,
            regular(12.5),
            text,
        );
        let keys = crate::app::commands::tool_shortcut_text(&row.key);
        if !keys.is_empty() {
            ui.painter().text(
                pos2(more_rect.min.x - 4.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                keys,
                regular(11.5),
                t.text_dim,
            );
        }
        // The ⋮ sits on the row; a click on it opens its menu, not the tool.
        let mut more_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(more_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let open = self.tool_more(&mut more_ui, group, row, row_hovered || more_open);
        ui.data_mut(|d| d.insert_temp(open_id, open));
        let on_more = more_rect.contains(ui.ctx().pointer_interact_pos().unwrap_or_default());
        if resp.clicked() && !on_more {
            self.run_reporting(&row.id, row.params.clone());
            ui.close();
        }
    }

    /// The ⋮ menu of a tool row; `true` while it is open.
    fn tool_more(
        &mut self,
        ui: &mut Ui,
        group: &'static str,
        row: &ToolRow,
        visible: bool,
    ) -> bool {
        let lang = self.lang;
        let t = Tokens::get(ui.ctx());
        let colour = if visible {
            t.text
        } else {
            egui::Color32::TRANSPARENT
        };
        let more = egui::RichText::new("⋮").font(semibold(15.0)).color(colour);
        let mut run: Option<(&str, Value)> = None;
        // A sub-menu button without the arrow: the ⋮ is the sign.
        let button = egui::Button::new(more)
            .frame(false)
            .min_size(vec2(MORE_W, ROW_H));
        let response = egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(220.0);
            let mut pinned = self.tool_pinned(group, &row.key);
            if ui
                .checkbox(&mut pinned, lang.text("cmd-ui-ribbon-pin"))
                .clicked()
            {
                run = Some((
                    "ui.ribbon.pin",
                    json!({"group": group, "tool": row.key, "on": pinned}),
                ));
            }
            let mut quick = self.settings.workspace.quick_tools.contains(&row.key);
            if ui
                .checkbox(&mut quick, lang.text("cmd-ui-quick-tools-pin"))
                .clicked()
            {
                run = Some(("ui.quickTools.pin", json!({"tool": row.key, "on": quick})));
            }
            ui.separator();
            if ui.button(lang.text("cmd-ui-shortcut-edit")).clicked() {
                run = Some(("ui.shortcut.edit", json!({"tool": row.key})));
                ui.close();
            }
        });
        let open = response.1.is_some();
        response.0.on_hover_text(lang.text("ribbon-tool-more"));
        if let Some((id, params)) = run {
            self.run_reporting(id, params);
        }
        open
    }

    /// The dialog that listens for a tool's new keyboard shortcut.
    pub(in crate::app) fn shortcut_dialog(&mut self, ctx: &egui::Context) {
        let Some(key) = self.ribbon_tools.capture.clone() else {
            return;
        };
        let lang = self.lang;
        let t = Tokens::get(ctx);
        let name = tool_name(self, &key);
        // The keys pressed this frame, before anything else sees them.
        let mut picked: Option<egui::KeyboardShortcut> = None;
        let mut cancel = false;
        let mut rejected = false;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => {
                    if *key == egui::Key::Escape && !modifiers.any() {
                        cancel = true;
                    } else {
                        let keys = pressed(*key, *modifiers);
                        if allowed(&keys) {
                            picked = Some(keys);
                        } else {
                            rejected = true;
                        }
                    }
                    false
                }
                egui::Event::Key { .. } | egui::Event::Text(_) => false,
                _ => true,
            });
        });
        let mut set: Option<Value> = None;
        let modal = egui::Modal::new(egui::Id::new("shortcut_dialog")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(
                egui::RichText::new(lang.text("shortcut-dialog-title"))
                    .font(semibold(14.0))
                    .color(t.text),
            );
            ui.add_space(6.0);
            ui.label(lang.format("shortcut-dialog-prompt", &[("tool", name.as_str().into())]));
            let current = crate::app::commands::tool_shortcut_text(&key);
            let current = if current.is_empty() {
                lang.text("shortcut-dialog-none").to_string()
            } else {
                current
            };
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
            ui.painter().rect(
                r,
                CornerRadius::same(6),
                t.input,
                Stroke::new(1.0, t.accent),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                lang.format(
                    "shortcut-dialog-current",
                    &[("keys", current.as_str().into())],
                ),
                regular(12.5),
                t.text,
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(lang.text("shortcut-dialog-modifier"))
                    .size(11.0)
                    .color(if rejected { t.accent } else { t.text_dim }),
            );
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(lang.text("shortcut-dialog-cancel")).clicked() {
                    cancel = true;
                }
                if ui.button(lang.text("shortcut-dialog-default")).clicked() {
                    set = Some(json!({"tool": key}));
                }
                if ui.button(lang.text("shortcut-dialog-clear")).clicked() {
                    set = Some(json!({"tool": key, "keys": ""}));
                }
            });
        });
        if let Some(keys) = picked {
            set = Some(json!({"tool": key, "keys": keys_text(&keys)}));
        }
        if modal.should_close() {
            cancel = true;
        }
        if let Some(params) = set {
            self.run_reporting("ui.shortcut.set", params);
            cancel = true;
        }
        if cancel {
            self.ribbon_tools.capture = None;
        }
    }
}

/// A tool's name for messages: its label in the catalog, with the parameters' value.
pub(in crate::app) fn tool_name(app: &App, key: &str) -> String {
    let Some((id, params)) = parse_tool(key) else {
        return key.to_string();
    };
    let lang = app.lang;
    let label =
        globecraft_core::commands::find(&id).map_or(id.clone(), |s| lang.text(s.label).to_string());
    if let Some(code) = params.get("layer").and_then(Value::as_str)
        && let Some(layer) = globecraft_core::layers::MapLayer::from_code(code)
    {
        return lang.text(layer.label_key()).to_string();
    }
    match params
        .as_object()
        .and_then(|o| o.values().next())
        .and_then(Value::as_str)
    {
        Some(value) => format!("{label}: {value}"),
        None => label,
    }
}
