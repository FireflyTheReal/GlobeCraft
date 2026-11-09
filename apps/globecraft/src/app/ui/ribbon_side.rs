//! Side menus in the ribbon groups' drop-downs, for settings finer than a tool: a group
//! menu's last rows open a menu beside it (the terrain's exaggeration, the theme, the
//! Earth model, the sources to add). The ribbon itself shows tools only.
use super::icons;
use super::theme::{Tokens, regular};
use super::widgets::{Look, menu_item_with};
use crate::app::App;
use eframe::egui::{self, Align2, Rect, Ui, pos2, vec2};
use globecraft_core::layers::MapLayer;
use globecraft_data::user_data::presets::Category;
use serde_json::json;

/// The terrain's exaggerations offered with one click.
const EXAGGERATIONS: [f32; 5] = [1.0, 1.5, 2.0, 2.5, 3.0];
/// Width of the Exaggeration side menu.
const EXAGGERATION_MENU_WIDTH_POINTS: f32 = 200.0;
/// Exaggerations closer than this count as the same choice.
const EXAGGERATION_MATCH: f32 = 0.05;

/// Width of a side menu's row in a group menu (as wide as the tool rows).
const SIDE_ROW_W: f32 = super::ribbon_tools::ROW_W;
const SIDE_ROW_H: f32 = 26.0;

/// Whether `group` has side menus (so its drop-down has something to show even with one
/// tool).
pub(super) fn has_side_menus(group: &str) -> bool {
    matches!(
        group,
        "map.nature" | "map.reference" | "map.data" | "view.window"
    )
}

impl App {
    /// The side menus of `group`'s drop-down, after its tools.
    pub(super) fn group_side_menus(&mut self, ui: &mut Ui, group: &str) {
        if !has_side_menus(group) {
            return;
        }
        let lang = self.lang;
        ui.separator();
        match group {
            "map.nature" => {
                side_menu(
                    ui,
                    self,
                    "terrain",
                    lang.text("cmd-layer-terrain-exaggeration"),
                    |ui, app| app.exaggeration_menu(ui),
                );
            }
            "map.reference" => {
                side_menu(
                    ui,
                    self,
                    "globe_view",
                    lang.text("cmd-world-earth-model"),
                    |ui, app| {
                        for model in globecraft_core::earth_model::EarthModel::ALL {
                            menu_item_with(
                                ui,
                                app,
                                "world.earthModel",
                                json!({"model": model.code()}),
                                Look {
                                    icon: Some("globe_view"),
                                    label: Some(lang.text(model.name_id())),
                                    ..Look::default()
                                },
                            );
                        }
                    },
                );
            }
            "view.window" => {
                side_menu(ui, self, "dark", lang.text("cmd-ui-theme"), |ui, app| {
                    for (code, key) in [
                        ("system", "settings-theme-system"),
                        ("dark", "settings-theme-dark"),
                        ("light", "settings-theme-light"),
                    ] {
                        menu_item_with(
                            ui,
                            app,
                            "ui.theme",
                            json!({"theme": code}),
                            Look {
                                icon: Some("dark"),
                                label: Some(lang.text(key)),
                                ..Look::default()
                            },
                        );
                    }
                });
            }
            "map.data" => {
                side_menu(
                    ui,
                    self,
                    "bookmark",
                    lang.text("data-pane-favorites-title"),
                    |ui, app| app.favorites_menu(ui),
                );
                side_menu(
                    ui,
                    self,
                    "history",
                    lang.text("data-pane-recent-title"),
                    |ui, app| app.recent_menu(ui),
                );
                for (category, icon) in [
                    (Category::Ocean, "river"),
                    (Category::Earth, "earth"),
                    (Category::Hazards, "radar"),
                ] {
                    side_menu(
                        ui,
                        self,
                        icon,
                        lang.text(category.label_key()),
                        |ui, app| app.library_menu(ui, category),
                    );
                }
            }
            _ => {}
        }
    }

    /// Exaggeration ▸: a few values with one click, and a slider for the rest.
    fn exaggeration_menu(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let enabled = self.layer(MapLayer::Terrain);
        let current = self.settings.layers.terrain_exaggeration;
        ui.set_width(EXAGGERATION_MENU_WIDTH_POINTS);
        if !enabled {
            ui.weak(lang.text("exaggeration-needs-terrain"));
        }
        for value in EXAGGERATIONS {
            let on = (current - value).abs() < EXAGGERATION_MATCH;
            let button = egui::Button::new(format!("{value:.1}×"))
                .frame(false)
                .selected(on)
                .min_size(vec2(EXAGGERATION_MENU_WIDTH_POINTS, 24.0));
            if super::widgets::row_button(ui, enabled, button).clicked() {
                self.run_reporting("layer.terrainExaggeration", json!({"value": value}));
                ui.close();
            }
        }
        ui.separator();
        let mut value = current;
        let slider = ui.add_enabled(
            enabled,
            egui::Slider::new(&mut value, 0.5..=3.0)
                .step_by(0.1)
                .suffix("×"),
        );
        if slider.changed() {
            self.settings.layers.terrain_exaggeration = value;
        }
    }
}

/// A row of a group menu that opens `add` in a menu beside it, drawn like the tool rows
/// above it (icon, name) with a ⏵ at the end.
fn side_menu(
    ui: &mut Ui,
    app: &mut App,
    icon: &str,
    label: &str,
    add: impl FnOnce(&mut Ui, &mut App),
) {
    let button = egui::Button::new("")
        .frame(false)
        .min_size(vec2(SIDE_ROW_W, SIDE_ROW_H))
        .right_text("⏵");
    let (resp, _) =
        egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| add(ui, app));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let t = Tokens::get(ui.ctx());
    let rect = resp.rect;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(18.0, 18.0)),
        icon,
        t.icon,
        t.accent,
    );
    ui.painter().text(
        pos2(rect.min.x + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        regular(12.5),
        t.text,
    );
}
