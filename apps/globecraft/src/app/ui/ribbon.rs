//! The ribbon: a File button, tabs in the order of the work (Home, Layers, Live, View, Places,
//! Settings, Help) and their groups of command buttons.
//!
//! The tab strip, collapse/peek behaviour and scrolling follow RustGIS's ribbon
//! (`crates/ui-egui/src/ribbon.rs`, Apache-2.0); see CREDITS.md.
use super::icons;
use super::theme::{BRAND, BRAND_DARK, Tokens, regular, semibold};
use super::widgets::{
    CONTENT_H, LABEL_H, Look, big, big_menu, big_with, col, group, menu_item, menu_item_with,
    small, small_menu, small_with, text_w,
};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::{
    commands::Tab, coordinates::CoordinateFormat, language::LanguagePreference, layers::MapLayer,
    units::RulerUnit,
};
use serde_json::{Value, json};

const SCROLL_BUTTON_W: f32 = 18.0;
pub(in crate::app) const COLLAPSE_W: f32 = 24.0;

impl App {
    pub(in crate::app) fn ribbon(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        // The tabs live in the header row (`chrome.rs`); this is only the ribbon body.
        if self.settings.workspace.ribbon_minimized && !self.ribbon_peek {
            return;
        }
        // A peeked ribbon closes on the next click outside it and the tab strip. Popups opened
        // from the ribbon draw outside it, so a click in one must not close it.
        let peek_click = self.ribbon_peek
            && ui.input(|i| i.pointer.any_click())
            && !egui::Popup::is_any_open(ui.ctx());
        let ribbon = egui::Panel::top("ribbon")
            .exact_size(CONTENT_H + LABEL_H + 12.0)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 5,
                bottom: 3,
            }))
            .show(ui, |ui| {
                let r = ui.max_rect();
                let line = Rect::from_min_max(
                    pos2(r.min.x - 8.0, r.max.y + 3.0),
                    pos2(r.max.x + 8.0, r.max.y + 4.0),
                );
                ui.painter().rect_filled(line, 0.0, t.border);
                let scroll = egui::ScrollArea::horizontal()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                            match self.settings.workspace.ribbon_tab {
                                Tab::Home => self.home_tab(ui),
                                Tab::Layers => self.layers_tab(ui),
                                Tab::Live => self.live_tab(ui),
                                Tab::View => self.view_tab(ui),
                                Tab::Places => self.places_tab(ui),
                                Tab::Settings => self.settings_tab(ui),
                                Tab::Help => self.help_tab(ui),
                            }
                        });
                    });
                scroll_buttons(ui, &t, &scroll, self.lang);
            });
        if peek_click {
            let pos = ui.input(|i| i.pointer.interact_pos());
            let inside = pos.is_some_and(|p| {
                ribbon.response.rect.contains(p)
                    || self.tab_strip_rect.is_some_and(|r| r.contains(p))
            });
            if !inside {
                self.ribbon_peek = false;
            }
        }
    }

    /// The File button, which opens the file menu as the Project button does in RustGIS.
    pub(in crate::app) fn file_button(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let file = lang.text("ribbon-file");
        let pw = text_w(ui, file, semibold(12.5)) + 26.0;
        let (pr, presp) = ui.allocate_exact_size(vec2(pw, 26.0), Sense::click());
        presp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, file));
        let fill = if presp.hovered() { BRAND_DARK } else { BRAND };
        ui.painter().rect_filled(pr, CornerRadius::same(6), fill);
        ui.painter().text(
            pr.center(),
            Align2::CENTER_CENTER,
            file,
            semibold(12.5),
            egui::Color32::WHITE,
        );
        let presp = presp.on_hover_text(lang.text("ribbon-file-help"));
        egui::Popup::menu(&presp).show(|ui| {
            ui.set_min_width(220.0);
            for id in ["places.open", "places.save", "places.saveAs"] {
                menu_item(ui, self, id, Value::Null);
            }
            ui.separator();
            menu_item(ui, self, "file.screenshot", Value::Null);
            ui.separator();
            menu_item(ui, self, "ui.settings", Value::Null);
            menu_item(ui, self, "ui.about", Value::Null);
            ui.separator();
            menu_item(ui, self, "app.quit", Value::Null);
        });
    }

    /// Sum of the tab captions' widths at the tab font, for the header's layout.
    pub(in crate::app) fn tab_captions_width(&self, ui: &Ui) -> f32 {
        Tab::ALL
            .into_iter()
            .map(|tab| text_w(ui, self.lang.text(tab.label_key()), semibold(12.5)))
            .sum()
    }

    /// The ribbon tabs, in `room` points of the header row; they scroll when it is short.
    pub(in crate::app) fn tab_buttons(&mut self, ui: &mut Ui, t: &Tokens, room: f32) {
        let lang = self.lang;
        let room = room.max(60.0);
        let strip_h = ui.max_rect().height();
        let captions: Vec<(Tab, &str)> = Tab::ALL
            .into_iter()
            .map(|tab| (tab, lang.text(tab.label_key())))
            .collect();
        let text: f32 = captions
            .iter()
            .map(|(_, c)| text_w(ui, c, semibold(12.5)))
            .sum();
        let pad = ((room - text) / captions.len() as f32).clamp(8.0, 22.0);
        let scroll = egui::ScrollArea::horizontal()
            .id_salt("tab_strip")
            .max_width(room)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                    for (tab, caption) in &captions {
                        let active = self.settings.workspace.ribbon_tab == *tab;
                        let font = if active {
                            semibold(12.5)
                        } else {
                            regular(12.5)
                        };
                        let w = text_w(ui, caption, font.clone()) + pad;
                        let (r, resp) = ui.allocate_exact_size(vec2(w, strip_h), Sense::click());
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                active,
                                *caption,
                            )
                        });
                        let body = Rect::from_min_max(pos2(r.min.x, r.min.y + 3.0), r.max);
                        let top = CornerRadius {
                            nw: 6,
                            ne: 6,
                            sw: 0,
                            se: 0,
                        };
                        let ribbon_open =
                            !self.settings.workspace.ribbon_minimized || self.ribbon_peek;
                        if active && ribbon_open {
                            ui.painter().rect_filled(body, top, t.ribbon);
                            ui.painter().rect_stroke(
                                body,
                                top,
                                Stroke::new(1.0, t.border),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().rect_filled(
                                Rect::from_min_max(
                                    pos2(r.min.x + 1.0, r.max.y - 1.5),
                                    pos2(r.max.x - 1.0, r.max.y + 1.0),
                                ),
                                0.0,
                                t.ribbon,
                            );
                            ui.painter().rect_filled(
                                Rect::from_min_max(
                                    pos2(r.min.x + 8.0, r.max.y - 3.0),
                                    pos2(r.max.x - 8.0, r.max.y - 1.0),
                                ),
                                CornerRadius::same(1),
                                t.accent,
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                body.shrink2(vec2(1.0, 2.0)),
                                CornerRadius::same(5),
                                t.hover,
                            );
                        }
                        let color = if active { t.text } else { t.text_dim };
                        ui.painter().text(
                            pos2(r.center().x, r.center().y + 1.5),
                            Align2::CENTER_CENTER,
                            *caption,
                            font,
                            color,
                        );
                        if resp.double_clicked() {
                            self.run_reporting("ui.ribbon", Value::Null);
                        } else if resp.clicked() {
                            if self.settings.workspace.ribbon_minimized {
                                self.ribbon_peek = !(self.ribbon_peek && active);
                            }
                            self.settings.workspace.ribbon_tab = *tab;
                        }
                    }
                });
            });
        scroll_buttons(ui, t, &scroll, lang);
    }

    /// The button that collapses or expands the ribbon body, at `cr`.
    pub(in crate::app) fn collapse_button(&mut self, ui: &mut Ui, t: &Tokens, cr: Rect) {
        let lang = self.lang;
        let cresp = ui.interact(cr, egui::Id::new("ribbon_collapse"), Sense::click());
        if cresp.hovered() {
            ui.painter().rect_filled(cr, CornerRadius::same(5), t.hover);
        }
        let minimized = self.settings.workspace.ribbon_minimized;
        let icon = if minimized { "chevron_down" } else { "up" };
        icons::paint(ui.painter(), cr.shrink(5.0), icon, t.text_dim, t.accent);
        let tip = lang.text(if minimized {
            "ribbon-expand-tooltip"
        } else {
            "ribbon-collapse-tooltip"
        });
        cresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        if cresp.on_hover_text(tip).clicked() {
            self.run_reporting("ui.ribbon", Value::Null);
        }
    }

    fn layer_big(&mut self, ui: &mut Ui, layer: MapLayer) {
        let label = short_layer_label(self.lang, layer);
        big_with(
            ui,
            self,
            "layer.show",
            json!({"layer": layer.code()}),
            Look {
                icon: Some(layer.icon()),
                label: Some(label),
                help: Some(self.lang.text(layer.label_key())),
                ..Look::default()
            },
        );
    }

    fn layer_small(&mut self, ui: &mut Ui, layer: MapLayer) {
        let label = short_layer_label(self.lang, layer);
        small_with(
            ui,
            self,
            "layer.show",
            json!({"layer": layer.code()}),
            Look {
                icon: Some(layer.icon()),
                label: Some(label),
                help: Some(self.lang.text(layer.label_key())),
                ..Look::default()
            },
        );
    }

    fn home_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("ribbon-group-navigate"), |ui| {
            big(ui, self, "search.place", Value::Null);
            col(ui, |ui| {
                small(ui, self, "camera.zoomIn", Value::Null);
                small(ui, self, "camera.zoomOut", Value::Null);
                small(ui, self, "camera.globalView", Value::Null);
            });
            col(ui, |ui| {
                small(ui, self, "camera.resetNorth", Value::Null);
                small(ui, self, "camera.autoRotate", Value::Null);
                small(ui, self, "ui.explorer", Value::Null);
            });
        });
        group(ui, lang.text("ribbon-group-places"), |ui| {
            big(ui, self, "places.addAtCenter", Value::Null);
            col(ui, |ui| {
                small(ui, self, "places.addFolder", Value::Null);
                small(ui, self, "places.open", Value::Null);
                small(ui, self, "ui.pane.places", Value::Null);
            });
        });
        group(ui, lang.text("ribbon-group-measure"), |ui| {
            big(ui, self, "ruler.enable", Value::Null);
            col(ui, |ui| {
                small(ui, self, "ruler.editPoints", Value::Null);
                small(ui, self, "ruler.clear", Value::Null);
                small(ui, self, "ui.pane.measure", Value::Null);
            });
            col(ui, |ui| {
                small(ui, self, "edit.undo", Value::Null);
                small(ui, self, "edit.redo", Value::Null);
            });
        });
        group(ui, lang.text("ribbon-group-capture"), |ui| {
            big(ui, self, "file.screenshot", Value::Null);
            col(ui, |ui| self.screenshot_quality_menu(ui));
        });
        group(ui, lang.text("ribbon-group-assistant"), |ui| {
            big(ui, self, "ui.pane.assistant", Value::Null);
        });
    }

    fn layers_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("ribbon-group-basemap"), |ui| {
            for (code, icon, key) in BASEMAPS {
                big_with(
                    ui,
                    self,
                    "layer.basemap",
                    json!({"basemap": code}),
                    Look {
                        icon: Some(icon),
                        label: Some(lang.text(key)),
                        help: Some(provider_help(lang, code)),
                        ..Look::default()
                    },
                );
            }
            col(ui, |ui| {
                self.wayback_stepper(ui);
            });
        });
        group(ui, lang.text("sidebar-geography-title"), |ui| {
            col(ui, |ui| {
                self.layer_small(ui, MapLayer::CountryBorders);
                self.layer_small(ui, MapLayer::StateBorders);
                self.layer_small(ui, MapLayer::PlaceLabels);
            });
            col(ui, |ui| {
                self.layer_small(ui, MapLayer::MunicipalBorders);
            });
        });
        group(ui, lang.text("layers-globe-title"), |ui| {
            self.layer_big(ui, MapLayer::Terrain);
            col(ui, |ui| {
                self.terrain_slider(ui);
                self.layer_small(ui, MapLayer::Grid);
                self.layer_small(ui, MapLayer::GridLabels);
            });
        });
        group(ui, lang.text("cmd-world-earth-model"), |ui| {
            col(ui, |ui| {
                for model in globecraft_core::earth_model::EarthModel::ALL {
                    small_with(
                        ui,
                        self,
                        "world.earthModel",
                        json!({"model": model.code()}),
                        Look {
                            icon: Some("globe_view"),
                            label: Some(lang.text(model.name_id())),
                            ..Look::default()
                        },
                    );
                }
            });
        });
        group(ui, lang.text("ribbon-group-pane"), |ui| {
            big(ui, self, "ui.pane.layers", Value::Null);
        });
    }

    fn wayback_stepper(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.add_space(14.0);
        ui.label(egui::RichText::new(lang.text("ribbon-imagery-date")).size(11.0));
        ui.horizontal(|ui| {
            let loaded = !self.wayback_list.is_empty();
            if ui
                .add_enabled(loaded, egui::Button::new("◀").small())
                .on_hover_text(lang.text("wayback-older-tooltip"))
                .clicked()
            {
                self.run_reporting("layer.waybackRelease", json!({"step": 1}));
            }
            if let Some(index) = self.wayback_date_menu(ui, "ribbon_wayback_date", self.wayback_idx)
            {
                let date = self.wayback_list[index].date.clone();
                self.run_reporting("layer.waybackRelease", json!({"date": date}));
            }
            if ui
                .add_enabled(loaded, egui::Button::new("▶").small())
                .on_hover_text(lang.text("wayback-newer-tooltip"))
                .clicked()
            {
                self.run_reporting("layer.waybackRelease", json!({"step": -1}));
            }
            let comparing = self.active_compare().is_some();
            if ui
                .add_enabled(
                    self.wayback_list.len() > 1,
                    egui::Button::new("⇆").small().selected(comparing),
                )
                .on_hover_text(lang.text("wayback-compare-tooltip"))
                .clicked()
            {
                self.run_reporting("layer.waybackCompare", json!({"enabled": !comparing}));
            }
        });
    }

    /// A drop-down of the Wayback release dates: a menu of years, each opening its own
    /// list of that year's releases (one year open at a time). Returns the release
    /// picked this frame, if any.
    pub(in crate::app) fn wayback_date_menu(
        &self,
        ui: &mut Ui,
        id: &str,
        current: usize,
    ) -> Option<usize> {
        let selected = self
            .wayback_list
            .get(current)
            .map_or_else(|| "—".to_string(), |r| r.date.clone());
        let mut picked = None;
        let year_of = |date: &str| date.get(..4).unwrap_or("").to_owned();
        // The list runs newest first, so the years do too.
        let mut years: Vec<String> = Vec::new();
        for release in &self.wayback_list {
            let year = year_of(&release.date);
            if years.last() != Some(&year) {
                years.push(year);
            }
        }
        ui.add_enabled_ui(!self.wayback_list.is_empty(), |ui| {
            ui.push_id(id, |ui| {
                let button = egui::RichText::new(format!("{selected} ⏷"))
                    .monospace()
                    .size(11.5);
                ui.menu_button(button, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(WAYBACK_MENU_MAX_HEIGHT_POINTS)
                        .show(ui, |ui| {
                            for year in &years {
                                let mut label = egui::RichText::new(year).monospace();
                                if selected.starts_with(year.as_str()) {
                                    label = label.strong();
                                }
                                ui.menu_button(label, |ui| {
                                    let releases = self
                                        .wayback_list
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, r)| year_of(&r.date) == *year);
                                    for (index, release) in releases {
                                        let date = egui::RichText::new(&release.date).monospace();
                                        if ui.selectable_label(index == current, date).clicked() {
                                            picked = Some(index);
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                });
            });
        });
        picked
    }

    fn terrain_slider(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let enabled = self.layer(MapLayer::Terrain);
        let mut value = self.settings.layers.terrain_exaggeration;
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(lang.text("cmd-layer-terrain-exaggeration")).size(11.5),
                );
                let r = ui.add(
                    egui::Slider::new(&mut value, 0.5..=3.0)
                        .step_by(0.1)
                        .suffix("×"),
                );
                if r.changed() {
                    self.settings.layers.terrain_exaggeration = value;
                }
            });
        });
    }

    fn live_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("sidebar-weather-title"), |ui| {
            self.layer_big(ui, MapLayer::Radar);
            self.layer_big(ui, MapLayer::Clouds);
        });
        group(ui, lang.text("sidebar-live-objects-title"), |ui| {
            self.layer_big(ui, MapLayer::Flights);
            self.layer_big(ui, MapLayer::Vessels);
            self.layer_big(ui, MapLayer::Satellites);
        });
        group(ui, lang.text("sidebar-transport-title"), |ui| {
            self.layer_big(ui, MapLayer::Roads);
            self.layer_big(ui, MapLayer::Traffic);
            col(ui, |ui| {
                ui.add_space(14.0);
                ui.label(egui::RichText::new(lang.text("layers-traffic-key")).size(11.0));
                ui.add(
                    egui::TextEdit::singleline(&mut self.tomtom_key)
                        .password(true)
                        .desired_width(150.0),
                );
            });
        });
        group(ui, lang.text("ribbon-group-pane"), |ui| {
            big_with(
                ui,
                self,
                "ui.pane.layers",
                Value::Null,
                Look {
                    label: Some(lang.text("ribbon-sources-status")),
                    ..Look::default()
                },
            );
        });
    }

    fn view_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("ribbon-group-worlds"), |ui| {
            for (body, icon, key) in [
                ("earth", "earth", "top-earth"),
                ("moon", "moon", "top-moon"),
                ("custom", "planet", "top-custom-planet"),
            ] {
                big_with(
                    ui,
                    self,
                    "world.body",
                    json!({"body": body}),
                    Look {
                        icon: Some(icon),
                        label: Some(lang.text(key)),
                        ..Look::default()
                    },
                );
            }
            big(ui, self, "ui.explorer", Value::Null);
        });
        group(ui, lang.text("sidebar-sky-title"), |ui| {
            col(ui, |ui| {
                self.layer_small(ui, MapLayer::Atmosphere);
                self.layer_small(ui, MapLayer::Sunlight);
                self.layer_small(ui, MapLayer::Stars);
            });
            col(ui, |ui| {
                self.layer_small(ui, MapLayer::CinematicSky);
            });
        });
        group(ui, lang.text("ribbon-group-panes"), |ui| {
            col(ui, |ui| {
                small(ui, self, "ui.pane.layers", Value::Null);
                small(ui, self, "ui.pane.places", Value::Null);
                small(ui, self, "ui.pane.explore", Value::Null);
            });
            col(ui, |ui| {
                small(ui, self, "ui.pane.assistant", Value::Null);
                small(ui, self, "ui.pane.measure", Value::Null);
                small(ui, self, "ui.leftPane", Value::Null);
            });
        });
        group(ui, lang.text("ribbon-group-window"), |ui| {
            big(ui, self, "ui.navigationControls", Value::Null);
            col(ui, |ui| {
                small(ui, self, "ui.fullscreen", Value::Null);
                small(ui, self, "ui.ribbon", Value::Null);
                let theme = lang.text("cmd-ui-theme");
                let help = lang.text("cmd-ui-theme-help");
                let mut picked = None;
                small_menu(ui, "dark", theme, help, |ui| {
                    for (code, key) in THEMES {
                        let on = self
                            .command_state("ui.theme", &json!({"theme": code}))
                            .checked;
                        if ui.radio(on, lang.text(key)).clicked() {
                            picked = Some(code);
                            ui.close();
                        }
                    }
                });
                if let Some(code) = picked {
                    self.run_reporting("ui.theme", json!({"theme": code}));
                }
            });
        });
    }

    fn places_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("ribbon-group-kml"), |ui| {
            big(ui, self, "places.open", Value::Null);
            big(ui, self, "places.save", Value::Null);
            big(ui, self, "places.saveAs", Value::Null);
        });
        group(ui, lang.text("ribbon-group-add"), |ui| {
            big(ui, self, "places.addAtCenter", Value::Null);
            big(ui, self, "places.addFolder", Value::Null);
        });
        group(ui, lang.text("ribbon-group-manage"), |ui| {
            big(ui, self, "ui.pane.places", Value::Null);
            big(ui, self, "places.clear", Value::Null);
        });
    }

    fn settings_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("ribbon-group-preferences"), |ui| {
            big(ui, self, "ui.settings", Value::Null);
        });
        group(ui, lang.text("cmd-settings-units"), |ui| {
            col(ui, |ui| {
                for (code, key) in [
                    ("system", "ribbon-units-system"),
                    ("metric", "ribbon-units-metric"),
                    ("imperial", "ribbon-units-imperial"),
                ] {
                    small_with(
                        ui,
                        self,
                        "settings.units",
                        json!({"units": code}),
                        Look {
                            icon: Some("ruler_units"),
                            label: Some(lang.text(key)),
                            ..Look::default()
                        },
                    );
                }
            });
        });
        group(ui, lang.text("ribbon-group-display"), |ui| {
            col(ui, |ui| {
                let mut picked: Option<(&str, Value)> = None;
                small_menu(
                    ui,
                    "goto_xy",
                    lang.text("cmd-settings-coordinate-format"),
                    lang.text("cmd-settings-coordinate-format-help"),
                    |ui| {
                        for format in CoordinateFormat::ALL {
                            let params = json!({"format": format.code()});
                            let on = self
                                .command_state("settings.coordinateFormat", &params)
                                .checked;
                            if ui.radio(on, lang.text(format.name_id())).clicked() {
                                picked = Some(("settings.coordinateFormat", params));
                                ui.close();
                            }
                        }
                    },
                );
                small_menu(
                    ui,
                    "globe_lang",
                    lang.text("cmd-settings-language"),
                    lang.text("cmd-settings-language-help"),
                    |ui| {
                        for choice in LanguagePreference::ALL {
                            let params = json!({"language": choice.code()});
                            let on = self.command_state("settings.language", &params).checked;
                            if ui
                                .radio(on, choice.label(lang, &self.system_locale))
                                .clicked()
                            {
                                picked = Some(("settings.language", params));
                                ui.close();
                            }
                        }
                    },
                );
                small_menu(
                    ui,
                    "measure",
                    lang.text("cmd-ruler-unit"),
                    lang.text("cmd-ruler-unit-help"),
                    |ui| {
                        for unit in RulerUnit::ALL {
                            let params = json!({"unit": unit.code()});
                            let on = self.command_state("ruler.unit", &params).checked;
                            if ui
                                .radio(on, unit.selector_label(lang, self.us_locale))
                                .clicked()
                            {
                                picked = Some(("ruler.unit", params));
                                ui.close();
                            }
                        }
                    },
                );
                if let Some((id, params)) = picked {
                    self.run_reporting(id, params);
                }
            });
        });
        group(ui, lang.text("cmd-ui-theme"), |ui| {
            col(ui, |ui| {
                for (code, key) in THEMES {
                    small_with(
                        ui,
                        self,
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
        });
        group(ui, lang.text("ribbon-group-pane"), |ui| {
            big_menu(
                ui,
                self,
                "assistant",
                lang.text("ribbon-assistant-provider"),
                lang.text("ribbon-assistant-provider-help"),
                |ui, app| {
                    menu_item_with(ui, app, "ui.pane.assistant", Value::Null, Look::default());
                    if ui.button(lang.text("assistant-settings-title")).clicked() {
                        app.settings
                            .workspace
                            .open_right(globecraft_data::settings::workspace::RightPane::Assistant);
                        app.assistant.show_settings = true;
                        ui.close();
                    }
                },
            );
        });
    }

    fn help_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(ui, lang.text("ribbon-tab-help"), |ui| {
            big(ui, self, "ui.shortcuts", Value::Null);
            big(ui, self, "help.documentation", Value::Null);
            big(ui, self, "help.reportIssue", Value::Null);
            big(ui, self, "ui.about", Value::Null);
        });
        group(ui, lang.text("ribbon-group-commands"), |ui| {
            big(ui, self, "ui.commandSearch", Value::Null);
        });
    }
}

const BASEMAPS: [(&str, &str, &str); 4] = [
    ("satellite", "basemap", "ribbon-basemap-satellite"),
    ("wayback", "history", "ribbon-basemap-wayback"),
    ("streets", "road", "ribbon-basemap-streets"),
    ("topo", "terrain", "ribbon-basemap-topo"),
];

const THEMES: [(&str, &str); 3] = [
    ("system", "settings-theme-system"),
    ("dark", "settings-theme-dark"),
    ("light", "settings-theme-light"),
];

fn provider_help(lang: globecraft_core::language::Lang, code: &str) -> &'static str {
    globecraft_data::tiles::Provider::from_code(code).map_or("", |p| p.name(lang))
}

/// The ribbon's short caption of a layer (the Layers pane shows the long one).
fn short_layer_label(lang: globecraft_core::language::Lang, layer: MapLayer) -> &'static str {
    lang.text(match layer {
        MapLayer::CountryBorders => "ribbon-layer-countries",
        MapLayer::StateBorders => "ribbon-layer-states",
        MapLayer::PlaceLabels => "ribbon-layer-labels",
        MapLayer::MunicipalBorders => "ribbon-layer-municipal",
        MapLayer::Terrain => "ribbon-layer-terrain",
        MapLayer::Grid => "ribbon-layer-grid",
        MapLayer::GridLabels => "ribbon-layer-grid-labels",
        MapLayer::Atmosphere => "ribbon-layer-atmosphere",
        MapLayer::Sunlight => "ribbon-layer-sunlight",
        MapLayer::Stars => "ribbon-layer-stars",
        MapLayer::CinematicSky => "ribbon-layer-cinematic",
        MapLayer::Radar => "ribbon-layer-radar",
        MapLayer::Clouds => "ribbon-layer-clouds",
        MapLayer::Flights => "ribbon-layer-flights",
        MapLayer::Vessels => "ribbon-layer-vessels",
        MapLayer::Satellites => "ribbon-layer-satellites",
        MapLayer::Roads => "ribbon-layer-roads",
        MapLayer::Traffic => "ribbon-layer-traffic",
    })
}

/// Arrow buttons at the edges of a horizontal scroll area whose content is wider than the
/// window (the scroll bar is hidden: it would cover group labels and tabs).
fn scroll_buttons<R>(
    ui: &mut Ui,
    t: &Tokens,
    scroll: &egui::scroll_area::ScrollAreaOutput<R>,
    lang: globecraft_core::language::Lang,
) {
    let view = scroll.inner_rect;
    let max = (scroll.content_size.x - view.width()).max(0.0);
    if max < 1.0 {
        return;
    }
    let mut state = scroll.state;
    let offset = state.offset.x;
    let step = (view.width() * 0.6).max(120.0);
    let mut moved = false;
    for (left, show) in [(true, offset > 0.5), (false, offset < max - 0.5)] {
        if !show {
            continue;
        }
        let x = if left {
            view.min.x
        } else {
            view.max.x - SCROLL_BUTTON_W
        };
        let r = Rect::from_min_size(pos2(x, view.min.y), vec2(SCROLL_BUTTON_W, view.height()));
        let resp = ui.interact(r, scroll.id.with(left), Sense::click());
        let fill = if resp.hovered() { t.hover } else { t.ribbon };
        ui.painter().rect_filled(r, CornerRadius::same(4), fill);
        ui.painter().rect_stroke(
            r,
            CornerRadius::same(4),
            Stroke::new(1.0, t.border),
            egui::StrokeKind::Inside,
        );
        let icon = if left {
            "chevron_left"
        } else {
            "chevron_right"
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(r.center(), vec2(16.0, 16.0)),
            icon,
            t.text_dim,
            t.accent,
        );
        let tip = lang.text(if left {
            "ribbon-scroll-left"
        } else {
            "ribbon-scroll-right"
        });
        if resp.on_hover_text(tip).clicked() {
            state.offset.x = if left {
                (offset - step).max(0.0)
            } else {
                (offset + step).min(max)
            };
            moved = true;
        }
    }
    if moved {
        state.store(ui.ctx(), scroll.id);
        ui.ctx().request_repaint();
    }
}

/// The year list of the Wayback date drop-down scrolls beyond this height.
const WAYBACK_MENU_MAX_HEIGHT_POINTS: f32 = 360.0;

impl App {
    /// The screenshot quality drop-down beside the capture button.
    fn screenshot_quality_menu(&mut self, ui: &mut Ui) {
        use globecraft_data::settings::screenshot::ScreenshotQuality;
        let current = self.settings.screenshot_quality;
        let mut chosen = None;
        ui.add_space(4.0);
        let label =
            egui::RichText::new(format!("{} ⏷", self.screenshot_quality_label(current))).size(11.5);
        ui.menu_button(label, |ui| {
            for quality in ScreenshotQuality::ALL {
                if ui
                    .selectable_label(quality == current, self.screenshot_quality_label(quality))
                    .clicked()
                {
                    chosen = Some(quality);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(self.lang.text("cmd-file-screenshot-quality-help"));
        if let Some(quality) = chosen {
            self.run_reporting("file.screenshotQuality", json!({"quality": quality.code()}));
        }
    }
}
