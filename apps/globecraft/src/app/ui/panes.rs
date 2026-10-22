//! Panes beside the globe: Layers, Places and Globe on the left, the AI Assistant and Measure on
//! the right, each with a tab header like RustGIS's panes (`crates/ui-egui/src/panes.rs`,
//! Apache-2.0).
use super::icons;
use super::theme::{Tokens, regular, semibold};
use super::widgets::text_w;
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use globecraft_data::settings::{
    sidebar::{
        MAX_SIDEBAR_WIDTH_POINTS, MIN_PINNED_PLACES_HEIGHT_POINTS, MIN_SIDEBAR_WIDTH_POINTS,
        SidebarPanel,
    },
    workspace::{MAX_RIGHT_PANE_WIDTH_POINTS, MIN_RIGHT_PANE_WIDTH_POINTS, RightPane},
};
use serde_json::Value;

const HEADER_H: f32 = 32.0;
const PINNED_PLACES_MAX_HEIGHT_FRACTION: f32 = 0.65;

/// The left panes in tab order, with their command and icon.
const LEFT: [(SidebarPanel, &str, &str, &str); 3] = [
    (
        SidebarPanel::Layers,
        "layers-section-title",
        "ui.pane.layers",
        "contents",
    ),
    (
        SidebarPanel::Places,
        "sidebar-places-title",
        "ui.pane.places",
        "places",
    ),
    (
        SidebarPanel::Explore,
        "sidebar-explore-title",
        "ui.pane.explore",
        "compass",
    ),
];

fn right_title(pane: RightPane) -> &'static str {
    match pane {
        RightPane::Assistant => "cmd-ui-pane-assistant",
        RightPane::Measure => "pane-measure-title",
    }
}

/// A pane's header strip; returns its rect.
fn header_strip(ui: &mut Ui, t: &Tokens) -> Rect {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.pane_header);
    ui.painter().line_segment(
        [pos2(r.min.x, r.max.y - 0.5), pos2(r.max.x, r.max.y - 0.5)],
        Stroke::new(1.0, t.border),
    );
    r
}

/// A tab in a pane header: hover fill, label (or icon when narrow) and the accent underline.
fn header_tab(
    ui: &mut Ui,
    t: &Tokens,
    r: Rect,
    id: egui::Id,
    name: &str,
    icon: Option<&str>,
    on: bool,
) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    let p = ui.painter();
    if resp.hovered() && !on {
        p.rect_filled(r.shrink2(vec2(1.0, 5.0)), CornerRadius::same(4), t.hover);
    }
    let ink = if on { t.text } else { t.text_dim };
    match icon {
        Some(icon) => icons::paint(
            p,
            Rect::from_center_size(r.center(), vec2(18.0, 18.0)),
            icon,
            ink,
            t.accent,
        ),
        None => {
            p.with_clip_rect(r).text(
                r.center(),
                Align2::CENTER_CENTER,
                name,
                if on { semibold(12.5) } else { regular(12.5) },
                ink,
            );
        }
    }
    if on {
        p.line_segment(
            [
                pos2(r.min.x + 4.0, r.max.y - 1.5),
                pos2(r.max.x - 4.0, r.max.y - 1.5),
            ],
            Stroke::new(2.0, t.accent),
        );
    }
    if icon.is_some() {
        resp.on_hover_text(name)
    } else {
        resp
    }
}

/// A small ✕ at `center`; returns whether it was clicked.
fn close_cross(ui: &mut Ui, t: &Tokens, center: egui::Pos2, id: egui::Id, tip: &str) -> bool {
    let cr = Rect::from_center_size(center, vec2(18.0, 18.0));
    let resp = ui.interact(cr, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.hovered() {
        ui.painter().rect_filled(cr, CornerRadius::same(3), t.hover);
    }
    icons::paint(ui.painter(), cr.shrink(4.0), "close", t.text_dim, t.accent);
    resp.on_hover_text(tip).clicked()
}

impl App {
    pub(in crate::app) fn left_pane(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        self.kml_drop_rect = Rect::NOTHING;
        // A placemark or ruler revealed from elsewhere brings its pane forward.
        if self.ruler_reveal_requested {
            self.settings.workspace.open_right(RightPane::Measure);
            self.ruler_reveal_requested = false;
        }
        if self.place_reveal_requested {
            self.settings.sidebar.open = true;
            if !self.settings.sidebar.places_pinned {
                self.settings.sidebar.panel = SidebarPanel::Places;
            }
        }
        if !self.settings.sidebar.open {
            return;
        }
        let frame = egui::Frame::NONE.fill(t.pane);
        let response = egui::Panel::left("left_pane")
            .resizable(true)
            .default_size(self.settings.sidebar.width_points)
            .size_range(MIN_SIDEBAR_WIDTH_POINTS..=MAX_SIDEBAR_WIDTH_POINTS)
            .frame(frame)
            .show(ui, |ui| {
                self.left_header(ui, &t);
                let panel = self.settings.sidebar.panel;
                let rect = ui.available_rect_before_wrap();
                let mut body =
                    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
                let ui = &mut body;
                if self.settings.sidebar.places_pinned && panel != SidebarPanel::Places {
                    let max_height = (ui.available_height() * PINNED_PLACES_MAX_HEIGHT_FRACTION)
                        .max(MIN_PINNED_PLACES_HEIGHT_POINTS);
                    let pinned = egui::Panel::bottom("pinned-places")
                        .default_size(self.settings.sidebar.pinned_places_height_points)
                        .size_range(MIN_PINNED_PLACES_HEIGHT_POINTS..=max_height)
                        .resizable(true)
                        .frame(
                            egui::Frame::NONE
                                .fill(t.pane)
                                .inner_margin(egui::Margin::symmetric(10, 6)),
                        )
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(self.lang.text("sidebar-places-title"))
                                        .font(semibold(12.5)),
                                );
                                self.pin_toggle(ui);
                            });
                            self.places_pane(ui);
                        });
                    if !ui.input(|i| i.pointer.any_down()) {
                        self.settings.sidebar.pinned_places_height_points =
                            pinned.response.rect.height();
                    }
                }
                let margin = egui::Margin::symmetric(10, 8);
                match panel {
                    SidebarPanel::Places => {
                        egui::Frame::NONE
                            .inner_margin(margin)
                            .show(ui, |ui| self.places_pane(ui));
                    }
                    SidebarPanel::Layers => {
                        egui::ScrollArea::vertical()
                            .id_salt("layers-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Frame::NONE
                                    .inner_margin(margin)
                                    .show(ui, |ui| self.layers_pane(ui));
                            });
                    }
                    SidebarPanel::Explore => {
                        egui::ScrollArea::vertical()
                            .id_salt("explore-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Frame::NONE
                                    .inner_margin(margin)
                                    .show(ui, |ui| self.explore_pane(ui));
                            });
                    }
                }
            });
        self.place_reveal_requested = false;
        if !ui.input(|i| i.pointer.any_down()) {
            self.settings.sidebar.width_points = response.response.rect.width();
        }
    }

    fn pin_toggle(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let t = Tokens::get(ui.ctx());
        let pinned = self.settings.sidebar.places_pinned;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            let tip = lang.text(if pinned {
                "sidebar-unpin-places-tooltip"
            } else {
                "sidebar-pin-places-tooltip"
            });
            resp.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    true,
                    pinned,
                    lang.text("sidebar-pin-places-action"),
                )
            });
            if pinned || resp.hovered() {
                ui.painter().rect_filled(
                    r,
                    CornerRadius::same(5),
                    if pinned { t.checked } else { t.hover },
                );
            }
            icons::paint(ui.painter(), r.shrink(4.0), "pin", t.icon, t.accent);
            if resp.on_hover_text(tip).clicked() {
                self.settings.sidebar.places_pinned = !pinned;
            }
        });
    }

    fn left_header(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = header_strip(ui, t);
        let lang = self.lang;
        let pad = 9.0;
        let names: Vec<&str> = LEFT.iter().map(|p| lang.text(p.1)).collect();
        let widths: Vec<f32> = names
            .iter()
            .map(|n| text_w(ui, n, semibold(12.5)) + 2.0 * pad)
            .collect();
        let pin_w = if self.settings.sidebar.panel == SidebarPanel::Places {
            28.0
        } else {
            0.0
        };
        let room = r.width() - 6.0 - 32.0 - pin_w;
        let icons_only = widths.iter().sum::<f32>() + 4.0 > room;
        let mut x = r.min.x + 6.0;
        for (i, ((panel, _, command, icon), name)) in LEFT.iter().zip(&names).enumerate() {
            let w = if icons_only { 32.0 } else { widths[i] };
            let tr = Rect::from_min_size(pos2(x, r.min.y), vec2(w, r.height()));
            let on = *panel == self.settings.sidebar.panel;
            let resp = header_tab(
                ui,
                t,
                tr,
                egui::Id::new(("left_pane_tab", i)),
                name,
                icons_only.then_some(*icon),
                on,
            );
            if resp.clicked() && !on {
                self.run_reporting(command, Value::Null);
            }
            x += w + 2.0;
        }
        if self.settings.sidebar.panel == SidebarPanel::Places {
            let mut pin = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        pos2(r.max.x - 30.0 - pin_w, r.min.y),
                        pos2(r.max.x - 30.0, r.max.y),
                    ))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            self.pin_toggle(&mut pin);
        }
        let tip = lang.text("pane-close-tooltip");
        if close_cross(
            ui,
            t,
            pos2(r.max.x - 16.0, r.center().y),
            egui::Id::new("left_pane_close"),
            tip,
        ) {
            self.settings.sidebar.open = false;
        }
    }

    pub(in crate::app) fn right_pane(&mut self, ui: &mut Ui) {
        let Some(pane) = self.settings.workspace.right_pane else {
            return;
        };
        let t = Tokens::get(ui.ctx());
        let response = egui::Panel::right("right_pane")
            .resizable(true)
            .default_size(self.settings.workspace.right_width_points)
            .size_range(MIN_RIGHT_PANE_WIDTH_POINTS..=MAX_RIGHT_PANE_WIDTH_POINTS)
            .frame(egui::Frame::NONE.fill(t.pane))
            .show(ui, |ui| {
                self.right_header(ui, pane, &t);
                let rect = ui.available_rect_before_wrap();
                let mut body =
                    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
                match pane {
                    RightPane::Assistant => self.assistant_pane(&mut body, &t),
                    RightPane::Measure => {
                        egui::ScrollArea::vertical()
                            .id_salt("measure-scroll")
                            .auto_shrink([false, false])
                            .show(&mut body, |ui| {
                                egui::Frame::NONE
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| self.measure_pane(ui));
                            });
                    }
                }
                ui.advance_cursor_after_rect(rect);
            });
        if !ui.input(|i| i.pointer.any_down()) {
            self.settings.workspace.right_width_points = response.response.rect.width();
        }
    }

    fn right_header(&mut self, ui: &mut Ui, current: RightPane, t: &Tokens) {
        let r = header_strip(ui, t);
        let lang = self.lang;
        let mut x = r.min.x + 6.0;
        for pane in self.settings.workspace.right_open.clone() {
            let name = lang.text(right_title(pane));
            let on = pane == current;
            let w = text_w(ui, name, if on { semibold(12.5) } else { regular(12.5) }) + 18.0 + 22.0;
            let tr = Rect::from_min_size(pos2(x, r.min.y), vec2(w, r.height()));
            let label_rect = Rect::from_min_max(tr.min, pos2(tr.max.x - 22.0, tr.max.y));
            let resp = header_tab(
                ui,
                t,
                label_rect,
                egui::Id::new(("right_pane_tab", pane as u8)),
                name,
                None,
                on,
            );
            if on {
                ui.painter().line_segment(
                    [
                        pos2(label_rect.max.x - 4.0, r.max.y - 1.5),
                        pos2(tr.max.x - 4.0, r.max.y - 1.5),
                    ],
                    Stroke::new(2.0, t.accent),
                );
            }
            let tip = lang.format("pane-close-named-tooltip", &[("name", name.into())]);
            if close_cross(
                ui,
                t,
                pos2(tr.max.x - 12.0, tr.center().y),
                egui::Id::new(("right_pane_close", pane as u8)),
                &tip,
            ) {
                self.settings.workspace.close_right(pane);
            } else if resp.clicked() && !on {
                self.settings.workspace.open_right(pane);
            }
            x += w + 2.0;
        }
    }
}
