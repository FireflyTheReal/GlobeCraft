//! Window chrome: the header row (File, ribbon tabs, Quick Access Toolbar, place search,
//! command search, ribbon collapse and the Windows/Linux window controls) and the status bar
//! (cursor, camera, altitude, heading, tiles, updates and status).
//!
//! The layout follows RustGIS's chrome (`crates/ui-egui/src/chrome.rs`, Apache-2.0).
use super::ribbon::COLLAPSE_W;
use super::theme::{Tokens, regular};
use super::widgets::{icon_command, menu_item, text_w};
use super::{icons, status};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::commands::Tab;
use serde_json::Value;

pub(in crate::app) const HEADER_HEIGHT: f32 = 38.0;
/// The elevation level asked for under the cursor when relief is off.
const CURSOR_ELEVATION_ZOOM: u8 = 10;
const HEADER_PAD: f32 = 10.0;
/// The header pad on the right is omitted when drawn window controls reach the edge.
#[cfg(any(target_os = "windows", target_os = "linux"))]
const HEADER_RIGHT_PAD: f32 = 0.0;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const HEADER_RIGHT_PAD: f32 = HEADER_PAD;
const STATUS_HEIGHT: f32 = 26.0;
/// Width of the place search box in the header, before UI scaling.
const PLACE_SEARCH_WIDTH: f32 = 300.0;
/// The place search never shrinks below this; the tabs scroll instead.
const MIN_PLACE_SEARCH_WIDTH: f32 = 140.0;
/// The quick-access row's buttons are 26 points wide with gaps between them; its tools
/// are the user's (`workspace.quick_tools`, pinned from a ribbon group's menu).
const QAT_GAP: f32 = 2.0;
/// The Help and Settings buttons in the header, and the gap after each.
const HEADER_ICON_W: f32 = 26.0;
const HEADER_ICON_GAP: f32 = 2.0;
const QAT_BUTTON: f32 = 26.0;
/// Smallest padding per tab before the tabs scroll.
const TAB_MIN_PAD: f32 = 8.0;
/// Room the cursor readout keeps in the status bar.
const CURSOR_READOUT_WIDTH: f32 = 250.0;

impl App {
    /// The single header row: File, the ribbon tabs, the Quick Access Toolbar, the
    /// place search, the command search and the ribbon collapse button. On macOS it is
    /// also the titlebar, after the native traffic lights; Windows and Linux keep their
    /// native title bar above it.
    pub(in crate::app) fn header(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        let mut field_rect = Rect::NOTHING;
        let left_inset = HEADER_PAD + self.traffic_lights_width(ui.ctx());
        let header = egui::Panel::top("header")
            .exact_size(HEADER_HEIGHT)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(t.tab_strip)
                    .inner_margin(egui::Margin {
                        left: left_inset.round() as i8,
                        right: HEADER_RIGHT_PAD.round() as i8,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show(ui, |ui| {
                let full = ui.max_rect();
                let drag = ui.interact(full, egui::Id::new("header_drag"), Sense::click_and_drag());
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                    self.file_button(ui);
                    ui.add_space(6.0);
                    // Right side, measured from the window edge: collapse button, command
                    // search, place search and the Quick Access Toolbar.
                    let right = full.max.x;
                    let left = ui.cursor().min.x;
                    let tabs_min =
                        self.tab_captions_width(ui) + Tab::RIBBON.len() as f32 * TAB_MIN_PAD;
                    let label = lang.text("cmd-ui-command-search");
                    let hint = if cfg!(target_os = "macos") {
                        "⌘K"
                    } else {
                        "Ctrl+K"
                    };
                    let full_cmd_w =
                        (text_w(ui, label, regular(12.0)) + text_w(ui, hint, regular(11.0)) + 60.0)
                            .clamp(170.0, 230.0);
                    let quick = self.settings.workspace.quick_tools.len() as f32;
                    let qat_width = (quick * (QAT_BUTTON + QAT_GAP) - QAT_GAP).max(0.0);
                    let fixed = COLLAPSE_W
                        + 8.0
                        + 2.0 * (HEADER_ICON_W + HEADER_ICON_GAP)
                        + 10.0
                        + 10.0
                        + qat_width
                        + 8.0;
                    let room = right - left - fixed - tabs_min;
                    let wanted_search = PLACE_SEARCH_WIDTH * self.settings.ui_scale.max(1.0);
                    let cmd_w = if room >= full_cmd_w + wanted_search {
                        full_cmd_w
                    } else {
                        26.0
                    };
                    let search_w = (room - cmd_w).clamp(MIN_PLACE_SEARCH_WIDTH, wanted_search);
                    let collapse = Rect::from_center_size(
                        pos2(right - COLLAPSE_W / 2.0, full.center().y),
                        vec2(COLLAPSE_W, COLLAPSE_W),
                    );
                    self.collapse_button(ui, &t, collapse);
                    // Help and Settings, left of the collapse button.
                    let icon_at = |i: f32| {
                        Rect::from_center_size(
                            pos2(
                                collapse.min.x
                                    - 8.0
                                    - HEADER_ICON_W / 2.0
                                    - i * (HEADER_ICON_W + HEADER_ICON_GAP),
                                full.center().y,
                            ),
                            vec2(HEADER_ICON_W, HEADER_ICON_W),
                        )
                    };
                    let settings_rect = icon_at(0.0);
                    let help_rect = icon_at(1.0);
                    self.settings_button(ui, &t, settings_rect);
                    self.help_button(ui, &t, help_rect);
                    let right = help_rect.min.x - 8.0;
                    let cr = Rect::from_min_max(
                        pos2(right - cmd_w, full.center().y - 12.0),
                        pos2(right, full.center().y + 12.0),
                    );
                    let resp = ui.interact(cr, egui::Id::new("command_search_box"), Sense::click());
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                    });
                    ui.painter().rect_filled(
                        cr,
                        CornerRadius::same(12),
                        if resp.hovered() { t.hover } else { t.input },
                    );
                    ui.painter().rect_stroke(
                        cr,
                        CornerRadius::same(12),
                        Stroke::new(1.0, t.border),
                        egui::StrokeKind::Inside,
                    );
                    let compact = cmd_w < 100.0;
                    let icon_x = if compact {
                        cr.center().x
                    } else {
                        cr.min.x + 16.0
                    };
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(pos2(icon_x, cr.center().y), vec2(14.0, 14.0)),
                        "command",
                        t.text_dim,
                        t.accent,
                    );
                    if !compact {
                        ui.painter().text(
                            pos2(cr.min.x + 30.0, cr.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            regular(12.0),
                            t.text_dim,
                        );
                        ui.painter().text(
                            pos2(cr.max.x - 12.0, cr.center().y),
                            Align2::RIGHT_CENTER,
                            hint,
                            regular(11.0),
                            t.text_disabled,
                        );
                    }
                    let resp = if compact {
                        resp.on_hover_text(format!("{label} ({hint})"))
                    } else {
                        resp
                    };
                    if resp.clicked() {
                        self.run_reporting("ui.commandSearch", Value::Null);
                    }
                    // The place search: a rounded field left of the command search.
                    let sr = Rect::from_min_max(
                        pos2(cr.min.x - 10.0 - search_w, full.center().y - 13.0),
                        pos2(cr.min.x - 10.0, full.center().y + 13.0),
                    );
                    ui.painter()
                        .rect_filled(sr, CornerRadius::same(13), t.input);
                    ui.painter().rect_stroke(
                        sr,
                        CornerRadius::same(13),
                        Stroke::new(1.0, t.input_border),
                        egui::StrokeKind::Inside,
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(sr.shrink2(vec2(10.0, 2.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    field_rect = self.search_bar(&mut child);
                    field_rect = field_rect.union(sr);
                    // Quick Access Toolbar, left of the place search.
                    let qat = Rect::from_min_max(
                        pos2(sr.min.x - 10.0 - qat_width, full.min.y),
                        pos2(sr.min.x - 10.0, full.max.y),
                    );
                    let mut qat_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(qat)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    qat_ui.spacing_mut().item_spacing = vec2(QAT_GAP, 0.0);
                    for key in self.settings.workspace.quick_tools.clone() {
                        if let Some((id, params)) = crate::app::tools::parse_tool(&key) {
                            icon_command(&mut qat_ui, self, &id, params);
                        }
                    }
                    // The tabs take what is left; they scroll when the window is narrow.
                    self.tab_buttons(ui, &t, qat.min.x - 8.0 - left);
                });
                self.header_drag(ui, &drag);
            });
        self.tab_strip_rect = Some(header.response.rect);
        self.suggestion_popup(ui, field_rect);
    }

    /// The ⚙ button: Settings.
    fn settings_button(&mut self, ui: &mut Ui, t: &Tokens, r: Rect) {
        let tip = self.lang.text("ribbon-settings-button");
        let resp = header_icon(ui, t, r, "settings", tip, egui::Id::new("header_settings"));
        if resp.clicked() {
            self.run_reporting("ui.settings", Value::Null);
        }
    }

    /// The ? button: a menu of help, shortcuts and About.
    fn help_button(&mut self, ui: &mut Ui, t: &Tokens, r: Rect) {
        let tip = self.lang.text("ribbon-help-menu");
        let resp = header_icon(ui, t, r, "help", tip, egui::Id::new("header_help"));
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            for id in ["ui.shortcuts", "ui.commandSearch"] {
                menu_item(ui, self, id, Value::Null);
            }
            ui.separator();
            for id in ["help.documentation", "help.reportIssue"] {
                menu_item(ui, self, id, Value::Null);
            }
            ui.separator();
            menu_item(ui, self, "ui.ribbon.resetTools", Value::Null);
            menu_item(ui, self, "ui.about", Value::Null);
        });
    }

    pub(in crate::app) fn status_bar(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        egui::Panel::bottom("status")
            .exact_size(STATUS_HEIGHT)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(t.status_bar)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| {
                let r = ui.max_rect();
                ui.painter().line_segment(
                    [pos2(r.min.x - 10.0, r.min.y), pos2(r.max.x + 10.0, r.min.y)],
                    Stroke::new(1.0, t.border),
                );
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
                    ui.style_mut()
                        .text_styles
                        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(11.5));
                    let start = ui.next_widget_position().x;
                    super::cursor::cursor_readout(
                        ui,
                        self.cursor,
                        start + CURSOR_READOUT_WIDTH,
                        self.settings.coordinate_display,
                        lang,
                    );
                    separator(ui, &t);
                    // The ground height under the cursor (roadmap 6.4). With relief on,
                    // from the elevation tiles the view has loaded; without it, a coarse
                    // elevation tile is requested under the cursor (about 40 km wide at
                    // level 10, so moving the pointer loads few). Padded so the readouts
                    // after it hold still.
                    if self.explorer.active == globecraft_core::body::Body::Earth {
                        let metres = self.cursor.and_then(|(lat, lon)| {
                            if self.terrain_active() {
                                self.cache.loaded_elevation_m(
                                    lat,
                                    lon,
                                    globecraft_data::tiles::Provider::TerrainDem.max_zoom(),
                                )
                            } else {
                                self.cache.elevation_m(lat, lon, CURSOR_ELEVATION_ZOOM)
                            }
                        });
                        let value = metres.map_or_else(
                            || "—".to_owned(),
                            |metres| self.distance_units().elevation(f64::from(metres)),
                        );
                        ui.monospace(lang.format(
                            "statusbar-cursor-elevation",
                            &[("distance", format!("{value:>8}").into())],
                        ))
                        .on_hover_text(
                            globecraft_data::tiles::Provider::TerrainDem.attribution(lang),
                        );
                        separator(ui, &t);
                        // The local time under the cursor (Time zones layer).
                        if self.layer(globecraft_core::layers::MapLayer::TimeZones) {
                            let time = self.cursor_local_time().unwrap_or_else(|| "—".into());
                            ui.monospace(lang.format(
                                "statusbar-cursor-local-time",
                                &[("time", format!("{time:<22}").into())],
                            ));
                            separator(ui, &t);
                        }
                    }
                    match self
                        .settings
                        .coordinate_display
                        .format(self.cam.lat, self.cam.lon)
                    {
                        Ok(coordinates) => {
                            let text = lang.format(
                                "coordinates-camera-readout",
                                &[("coordinates", coordinates.as_str().into())],
                            );
                            super::coordinate_display::copyable_label(ui, text, &coordinates, lang);
                        }
                        Err(error) => {
                            ui.monospace("—").on_hover_text(error.to_string());
                        }
                    }
                    separator(ui, &t);
                    let altitude = lang.format(
                        "statusbar-camera-altitude",
                        &[(
                            "distance",
                            self.distance_units().altitude(self.cam.alt).into(),
                        )],
                    );
                    let altitude_response = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(altitude)
                                    .monospace()
                                    .color(t.accent_text),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(lang.text("altitude-edit-tooltip"));
                    if altitude_response.clicked() {
                        self.altitude_editor = Some(super::altitude::AltitudeEditor::new(
                            self.cam.alt,
                            self.distance_units(),
                            altitude_response.rect.left_top(),
                        ));
                    }
                    separator(ui, &t);
                    let heading = self
                        .renderer
                        .displayed_view()
                        .map_or(self.cam.heading, |(cam, _)| cam.heading);
                    if super::compass::heading_readout(ui, heading, lang) {
                        self.cam.heading = 0.0;
                        self.fly = None;
                        self.touch();
                        ui.request_repaint();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let n = self.cache.inflight();
                        let (text, color) = if n > 0 {
                            (
                                lang.format(
                                    "statusbar-tiles-loading",
                                    &[("count", n.to_string().into())],
                                ),
                                t.amber,
                            )
                        } else {
                            (lang.text("statusbar-tiles-ready").to_string(), t.text_dim)
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .size(11.0)
                                .monospace()
                                .color(color),
                        );
                        separator(ui, &t);
                        let action = super::updater::indicator(
                            ui,
                            self.updater.state(),
                            lang,
                            self.update_restart.error.as_deref(),
                            self.update_restart.is_saving(),
                        );
                        match action {
                            super::updater::Action::InstallOrRetry => self.updater.activate(),
                            super::updater::Action::Restart => self.request_update_restart(ui),
                            super::updater::Action::OpenReleases => {
                                ui.open_url(egui::OpenUrl::new_tab(
                                    globecraft_data::updater::RELEASES_URL,
                                ));
                            }
                            super::updater::Action::None => {}
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
                            status::show(ui, &self.status, self.status_file.as_ref());
                        });
                    });
                });
            });
    }
}

fn separator(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 14.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.border_strong);
}

/// A header icon button at `r`, with a hover fill.
fn header_icon(ui: &mut Ui, t: &Tokens, r: Rect, icon: &str, tip: &str, id: egui::Id) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp))
    {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(5.0), icon, t.text_dim, t.accent);
    resp.on_hover_text(tip)
}
