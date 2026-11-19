//! The side pane beside the globe (one since 2026-11-19, on the right by default or on
//! the left, `ui.pane.side`): its tabs are Layers, Places, the AI Assistant, Measure,
//! the Legend, the Explorer and the details of a selected flight, satellite or point,
//! in an order the user drags them into (`ui.pane.moveTab`). The tab header follows
//! RustGIS's panes (`crates/ui-egui/src/panes.rs`, Apache-2.0).
use super::icons;
use super::theme::{Tokens, regular, semibold};
use super::widgets::text_w;
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::layers::MapLayer;
use globecraft_data::settings::{
    sidebar::MIN_PINNED_PLACES_HEIGHT_POINTS,
    workspace::{MAX_RIGHT_PANE_WIDTH_POINTS, MIN_RIGHT_PANE_WIDTH_POINTS, PaneSide, PaneTab},
};
use serde_json::{Value, json};

const HEADER_H: f32 = 32.0;
const PINNED_PLACES_MAX_HEIGHT_FRACTION: f32 = 0.65;
/// Room around a tab's name, its ✕, and a tab shown as its icon.
const TAB_PAD: f32 = 9.0;
const CLOSE_W: f32 = 20.0;
const ICON_TAB_W: f32 = 30.0;
/// The button that moves the pane to the other side, and the Places pin.
const HEADER_BUTTON_W: f32 = 28.0;

fn tab_title(tab: PaneTab) -> &'static str {
    match tab {
        PaneTab::Assistant => "cmd-ui-pane-assistant",
        PaneTab::Measure => "pane-measure-title",
        PaneTab::Legend => "legend-title",
        PaneTab::Explorer => "explorer-pane-title",
        PaneTab::Layers => "layers-section-title",
        PaneTab::Places => "sidebar-places-title",
        PaneTab::Flight => "pane-flight-title",
        PaneTab::Satellite => "pane-satellite-title",
        PaneTab::Here => "here-title",
    }
}

fn tab_icon(tab: PaneTab) -> &'static str {
    match tab {
        PaneTab::Assistant => "assistant",
        PaneTab::Measure => "measure",
        PaneTab::Legend => "symbology",
        PaneTab::Explorer => "explore",
        PaneTab::Layers => "contents",
        PaneTab::Places => "places",
        PaneTab::Flight => "plane",
        PaneTab::Satellite => "satellite",
        PaneTab::Here => "identify",
    }
}

/// Where a tab dropped at `x` goes among the other tabs, whose middles are `centres`.
fn drop_index(centres: &[f32], x: f32) -> usize {
    centres.iter().filter(|centre| **centre < x).count()
}

/// A tab's place in the header: its name (or icon) and the ✕ after it.
#[derive(Clone, Copy)]
struct TabPlace {
    tab: PaneTab,
    label: Rect,
    close_w: f32,
}

impl TabPlace {
    fn centre_x(&self) -> f32 {
        self.label.center().x + self.close_w / 2.0
    }
    fn right(&self) -> f32 {
        self.label.max.x + self.close_w
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

/// A tab in a pane header: hover fill and its name (or icon when narrow). It can be
/// clicked or dragged.
fn header_tab(
    ui: &mut Ui,
    t: &Tokens,
    r: Rect,
    id: egui::Id,
    name: &str,
    icon: Option<&str>,
    on: bool,
) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click_and_drag());
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
    /// Opens or closes the details tabs with their selections (a new selection also
    /// brings its tab forward: `close_other_cards`).
    fn sync_details_tabs(&mut self) {
        let selected = [
            (PaneTab::Flight, self.picked.flight.selected().is_some()),
            (
                PaneTab::Satellite,
                self.layer(MapLayer::Satellites) && self.selected_satellite().is_some(),
            ),
            (PaneTab::Here, self.pointer.here_card.is_some()),
        ];
        let workspace = &mut self.settings.workspace;
        for (tab, on) in selected {
            match (on, workspace.right_is_open(tab)) {
                (true, false) => workspace.add_right(tab),
                (false, true) => workspace.close_right(tab),
                _ => {}
            }
        }
    }

    /// Closes `tab`; a details tab clears its selection with it.
    fn close_tab(&mut self, tab: PaneTab) {
        // Clearing cannot fail.
        match tab {
            PaneTab::Flight => drop(self.run("flight.clear", json!({}))),
            PaneTab::Satellite => drop(self.run("satellite.clear", json!({}))),
            PaneTab::Here => self.pointer.here_card = None,
            _ => {}
        }
        self.settings.workspace.close_right(tab);
    }

    pub(in crate::app) fn side_pane(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        // A placemark or ruler revealed from elsewhere brings its tab forward.
        if self.measure.reveal_requested {
            self.settings.workspace.open_right(PaneTab::Measure);
            self.measure.reveal_requested = false;
        }
        if self.places.reveal_requested {
            if !self.settings.sidebar.places_pinned {
                self.settings.workspace.open_right(PaneTab::Places);
            } else if self.settings.workspace.right_pane.is_none() {
                self.settings.workspace.toggle_pane();
            }
            self.places.reveal_requested = false;
        }
        self.sync_details_tabs();
        let Some(pane) = self.settings.workspace.right_pane else {
            return;
        };
        let panel = match self.settings.workspace.pane_side {
            PaneSide::Left => egui::Panel::left("side_pane"),
            PaneSide::Right => egui::Panel::right("side_pane"),
        };
        let response = panel
            .resizable(true)
            .default_size(self.settings.workspace.right_width_points)
            .size_range(MIN_RIGHT_PANE_WIDTH_POINTS..=MAX_RIGHT_PANE_WIDTH_POINTS)
            .frame(egui::Frame::NONE.fill(t.pane))
            .show(ui, |ui| {
                self.pane_header(ui, pane, &t);
                let rect = ui.available_rect_before_wrap();
                let mut body =
                    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
                // The content follows the pane's width while its edge is dragged.
                body.set_max_width(rect.width());
                self.pinned_places(&mut body, pane, &t);
                self.pane_body(&mut body, pane, &t);
                ui.advance_cursor_after_rect(rect);
            });
        if !ui.input(|i| i.pointer.any_down()) {
            self.settings.workspace.right_width_points = response.response.rect.width();
        }
    }

    /// Places pinned under the other tabs.
    fn pinned_places(&mut self, ui: &mut Ui, pane: PaneTab, t: &Tokens) {
        if !self.settings.sidebar.places_pinned || pane == PaneTab::Places {
            return;
        }
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
            self.settings.sidebar.pinned_places_height_points = pinned.response.rect.height();
        }
    }

    fn pane_body(&mut self, body: &mut Ui, pane: PaneTab, t: &Tokens) {
        let scrolled = |body: &mut Ui, id: &str, app: &mut App, content: fn(&mut App, &mut Ui)| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(body, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| content(app, ui));
                });
        };
        match pane {
            #[cfg(feature = "assistant")]
            PaneTab::Assistant => self.assistant_pane(body, t),
            #[cfg(not(feature = "assistant"))]
            PaneTab::Assistant => {
                let _ = t;
                body.weak("This build of GlobeCraft has no assistant.");
            }
            PaneTab::Layers => {
                // The search stays above the scroll area, so it remains reachable
                // while a long layer list scrolls.
                egui::Frame::NONE
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 10,
                        top: 8,
                        bottom: 0,
                    })
                    .show(body, |ui| self.layers_search(ui));
                scrolled(body, "layers-scroll", self, |app, ui| app.layers_pane(ui));
            }
            PaneTab::Places => {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(body, |ui| self.places_pane(ui));
            }
            PaneTab::Legend => scrolled(body, "legend-scroll", self, |app, ui| app.legend_pane(ui)),
            PaneTab::Explorer => scrolled(body, "explorer-scroll", self, |app, ui| {
                app.explorer_pane(ui)
            }),
            PaneTab::Measure => {
                scrolled(body, "measure-scroll", self, |app, ui| app.measure_pane(ui))
            }
            PaneTab::Flight => scrolled(body, "flight-scroll", self, |app, ui| {
                app.flight_details(ui)
            }),
            PaneTab::Satellite => scrolled(body, "satellite-scroll", self, |app, ui| {
                app.satellite_details(ui)
            }),
            PaneTab::Here => scrolled(body, "here-scroll", self, |app, ui| app.here_details(ui)),
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

    /// Where each tab goes in a header `r` wide: named, or as icons when the names do
    /// not fit (then only the tab in view keeps its ✕, and the icons narrow if need be).
    fn tab_places(&self, ui: &Ui, r: Rect, current: PaneTab, room: f32) -> (Vec<TabPlace>, bool) {
        let tabs = &self.settings.workspace.right_open;
        let named: Vec<f32> = tabs
            .iter()
            .map(|tab| text_w(ui, self.lang.text(tab_title(*tab)), semibold(12.5)) + 2.0 * TAB_PAD)
            .collect();
        let icons_only = named.iter().sum::<f32>() + CLOSE_W * tabs.len() as f32 > room;
        let icon_w = ICON_TAB_W.min((room - CLOSE_W) / tabs.len().max(1) as f32 - 2.0);
        let mut x = r.min.x + 6.0;
        let places = tabs
            .iter()
            .zip(named)
            .map(|(&tab, name_w)| {
                let (label_w, close_w) = match (icons_only, tab == current) {
                    (false, _) => (name_w, CLOSE_W),
                    (true, true) => (icon_w, CLOSE_W),
                    (true, false) => (icon_w, 0.0),
                };
                let place = TabPlace {
                    tab,
                    label: Rect::from_min_size(pos2(x, r.min.y), vec2(label_w, r.height())),
                    close_w,
                };
                x = place.right() + 2.0;
                place
            })
            .collect();
        (places, icons_only)
    }

    /// The tabs, each with its ✕; dragged along the header they change places. At the
    /// end: the Places pin and the button that moves the pane to the other side.
    fn pane_header(&mut self, ui: &mut Ui, current: PaneTab, t: &Tokens) {
        let r = header_strip(ui, t);
        let lang = self.lang;
        let pin_w = if current == PaneTab::Places {
            HEADER_BUTTON_W
        } else {
            0.0
        };
        let room = r.width() - 6.0 - HEADER_BUTTON_W - pin_w;
        let (places, icons_only) = self.tab_places(ui, r, current, room);
        let mut dragging: Option<(PaneTab, f32)> = None;
        let mut dropped: Option<(PaneTab, f32)> = None;
        for place in &places {
            let tab = place.tab;
            let name = lang.text(tab_title(tab));
            let on = tab == current;
            let resp = header_tab(
                ui,
                t,
                place.label,
                super::focus::pane_tab_id(tab),
                name,
                icons_only.then_some(tab_icon(tab)),
                on,
            );
            if on {
                ui.painter().line_segment(
                    [
                        pos2(place.label.min.x + 4.0, r.max.y - 1.5),
                        pos2(place.right() - 4.0, r.max.y - 1.5),
                    ],
                    Stroke::new(2.0, t.accent),
                );
            }
            // egui reports a drag only past its click distance: a click stays a click.
            let pointer_x = ui.input(|i| i.pointer.latest_pos()).map(|p| p.x);
            if resp.dragged()
                && let Some(x) = pointer_x
            {
                dragging = Some((tab, x));
            }
            if resp.drag_stopped()
                && let Some(x) = pointer_x
            {
                dropped = Some((tab, x));
            }
            if place.close_w > 0.0 {
                let tip = lang.format("pane-close-named-tooltip", &[("name", name.into())]);
                if close_cross(
                    ui,
                    t,
                    pos2(place.right() - CLOSE_W / 2.0 - 1.0, r.center().y),
                    egui::Id::new(("side_pane_close", tab.code())),
                    &tip,
                ) {
                    self.close_tab(tab);
                    continue;
                }
            }
            if resp.clicked() && !on {
                self.settings.workspace.open_right(tab);
            }
        }
        let others = |tab: PaneTab| -> Vec<TabPlace> {
            places.iter().copied().filter(|p| p.tab != tab).collect()
        };
        let centres =
            |others: &[TabPlace]| -> Vec<f32> { others.iter().map(TabPlace::centre_x).collect() };
        // Where a dragged tab would land: a line between the tabs.
        if let Some((tab, x)) = dragging {
            let others = others(tab);
            let index = drop_index(&centres(&others), x);
            let line_x = match others.get(index) {
                Some(place) => place.label.min.x - 1.0,
                None => others.last().map_or(r.min.x + 6.0, |p| p.right() + 1.0),
            };
            ui.painter().line_segment(
                [pos2(line_x, r.min.y + 5.0), pos2(line_x, r.max.y - 5.0)],
                Stroke::new(2.0, t.accent),
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if let Some((tab, x)) = dropped {
            let to = drop_index(&centres(&others(tab)), x);
            self.run_reporting("ui.pane.moveTab", json!({"tab": tab.code(), "to": to}));
        }
        if current == PaneTab::Places {
            let mut pin = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        pos2(r.max.x - HEADER_BUTTON_W - pin_w, r.min.y),
                        pos2(r.max.x - HEADER_BUTTON_W, r.max.y),
                    ))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            self.pin_toggle(&mut pin);
        }
        self.side_button(ui, t, r);
    }

    /// Moves the pane to the other side of the globe; its menu docks it on either.
    fn side_button(&mut self, ui: &mut Ui, t: &Tokens, header: Rect) {
        let lang = self.lang;
        let side = self.settings.workspace.pane_side;
        let br = Rect::from_center_size(
            pos2(header.max.x - HEADER_BUTTON_W / 2.0, header.center().y),
            vec2(22.0, 22.0),
        );
        let resp = ui.interact(br, egui::Id::new("side_pane_side"), Sense::click());
        let tip = lang.text("pane-side-switch-tooltip");
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        if resp.hovered() {
            ui.painter().rect_filled(br, CornerRadius::same(4), t.hover);
        }
        let icon = match side {
            PaneSide::Right => "chevron_left",
            PaneSide::Left => "chevron_right",
        };
        icons::paint(ui.painter(), br.shrink(4.0), icon, t.text_dim, t.accent);
        let resp = resp.on_hover_text(tip);
        if resp.clicked() {
            self.run_reporting("ui.pane.side", Value::Null);
        }
        resp.context_menu(|ui| {
            for (option, key) in [
                (PaneSide::Left, "pane-side-left"),
                (PaneSide::Right, "pane-side-right"),
            ] {
                if ui.radio(side == option, lang.text(key)).clicked() {
                    self.run_reporting("ui.pane.side", json!({"side": option.code()}));
                    ui.close();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_tab_lands_between_the_tabs_it_passed() {
        let centres = [50.0, 150.0, 250.0];
        assert_eq!(drop_index(&centres, 10.0), 0);
        assert_eq!(drop_index(&centres, 120.0), 1);
        assert_eq!(drop_index(&centres, 900.0), 3);
    }
}
