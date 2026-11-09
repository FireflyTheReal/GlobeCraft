//! The ribbon: a File button, the tabs (Explore, Map, Live, View; their groups are in
//! `ribbon_tabs.rs`), collapsing and scrolling.
//!
//! The tab strip, collapse/peek behaviour and scrolling follow RustGIS's ribbon
//! (`crates/ui-egui/src/ribbon.rs`, Apache-2.0); see CREDITS.md.
use super::icons;
use super::theme::{BRAND, BRAND_DARK, Tokens, regular, semibold};
use super::widgets::{CONTENT_H, LABEL_H, menu_item, text_w};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::commands::Tab;
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
                                Tab::Explore | Tab::File | Tab::Help => self.explore_tab(ui),
                                Tab::Map => self.map_tab(ui),
                                Tab::Live => self.live_tab(ui),
                                Tab::View => self.view_tab(ui),
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
            menu_item(ui, self, "ui.addData", Value::Null);
            menu_item(ui, self, "file.screenshot", Value::Null);
            menu_item(ui, self, "view.copyLink", Value::Null);
            ui.separator();
            menu_item(ui, self, "ui.settings", Value::Null);
            ui.separator();
            menu_item(ui, self, "app.quit", Value::Null);
        });
    }

    /// Sum of the tab captions' widths at the tab font, for the header's layout.
    pub(in crate::app) fn tab_captions_width(&self, ui: &Ui) -> f32 {
        Tab::RIBBON
            .into_iter()
            .map(|tab| text_w(ui, self.lang.text(tab.label_key()), semibold(12.5)))
            .sum()
    }

    /// The ribbon tabs, in `room` points of the header row; they scroll when it is short.
    pub(in crate::app) fn tab_buttons(&mut self, ui: &mut Ui, t: &Tokens, room: f32) {
        let lang = self.lang;
        let room = room.max(60.0);
        let strip_h = ui.max_rect().height();
        let captions: Vec<(Tab, &str)> = Tab::RIBBON
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

    pub(super) fn wayback_stepper(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.add_space(14.0);
        ui.label(egui::RichText::new(lang.text("ribbon-imagery-date")).size(11.0));
        ui.horizontal(|ui| {
            let loaded = !self.wayback.releases.is_empty();
            if ui
                .add_enabled(loaded, egui::Button::new("◀").small())
                .on_hover_text(lang.text("wayback-older-tooltip"))
                .clicked()
            {
                self.run_reporting("layer.waybackRelease", json!({"step": 1}));
            }
            if let Some(index) =
                self.wayback_date_menu(ui, "ribbon_wayback_date", self.wayback.index)
            {
                let date = self.wayback.releases[index].date.clone();
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
                    self.wayback.releases.len() > 1,
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
            .wayback
            .releases
            .get(current)
            .map_or_else(|| "—".to_string(), |r| r.date.clone());
        let mut picked = None;
        let year_of = |date: &str| date.get(..4).unwrap_or("").to_owned();
        // The list runs newest first, so the years do too.
        let mut years: Vec<String> = Vec::new();
        for release in &self.wayback.releases {
            let year = year_of(&release.date);
            if years.last() != Some(&year) {
                years.push(year);
            }
        }
        ui.add_enabled_ui(!self.wayback.releases.is_empty(), |ui| {
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
                                        .wayback
                                        .releases
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
    pub(super) fn screenshot_quality_menu(&mut self, ui: &mut Ui) {
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
