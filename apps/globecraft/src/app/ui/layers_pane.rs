//! The Layers pane: the basemap and every toggleable layer as a table of contents, grouped,
//! with each live layer's status and data credits under it while it is shown.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::layers::{LayerGroup, MapLayer};
use globecraft_data::tiles::Provider;
use serde_json::json;

const ROW_H: f32 = 24.0;
const INDENT: f32 = 18.0;

/// A table-of-contents row: a check box (or radio dot), an icon and a label. Returns the click.
struct TocRow<'a> {
    icon: &'a str,
    label: &'a str,
    checked: bool,
    enabled: bool,
    /// A radio dot (one of several basemaps) rather than a check box.
    radio: bool,
    indent: f32,
}

/// Boundary loader messages shown under Geography; older ones stay in the log only.
const BOUNDARY_LOG_LINES: usize = 12;

fn toc_row(ui: &mut Ui, t: &Tokens, row: TocRow<'_>) -> egui::Response {
    let TocRow {
        icon,
        label,
        checked,
        enabled,
        radio,
        indent,
    } = row;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            if radio {
                egui::WidgetType::RadioButton
            } else {
                egui::WidgetType::Checkbox
            },
            enabled,
            checked,
            label,
        )
    });
    let p = ui.painter();
    if enabled && (resp.hovered() || resp.is_pointer_button_down_on()) {
        p.rect_filled(
            r,
            CornerRadius::same(5),
            if resp.is_pointer_button_down_on() {
                t.pressed
            } else {
                t.hover
            },
        );
    }
    let x0 = r.min.x + 4.0 + indent;
    let bx = Rect::from_center_size(pos2(x0 + 7.0, r.center().y), vec2(14.0, 14.0));
    let ink = if enabled { t.text } else { t.text_disabled };
    if radio {
        p.circle_stroke(
            bx.center(),
            6.5,
            Stroke::new(1.2, if checked { t.accent } else { t.input_border }),
        );
        if checked {
            p.circle_filled(bx.center(), 3.5, t.accent);
        }
    } else {
        p.rect(
            bx,
            CornerRadius::same(3),
            if checked { t.accent } else { t.input },
            Stroke::new(1.0, if checked { t.accent } else { t.input_border }),
            egui::StrokeKind::Inside,
        );
        if checked {
            p.line(
                vec![
                    bx.center() + vec2(-3.5, 0.0),
                    bx.center() + vec2(-1.0, 2.8),
                    bx.center() + vec2(3.8, -2.8),
                ],
                Stroke::new(1.6, egui::Color32::WHITE),
            );
        }
    }
    let ic = Rect::from_center_size(pos2(x0 + 28.0, r.center().y), vec2(17.0, 17.0));
    icons::paint(
        p,
        ic,
        icon,
        if enabled { t.icon } else { t.text_disabled },
        if enabled { t.accent } else { t.text_disabled },
    );
    p.with_clip_rect(r).text(
        pos2(x0 + 42.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        regular(12.5),
        ink,
    );
    let resp = resp.on_hover_text(label);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A group heading with a disclosure arrow; returns whether it is open.
fn group_heading(ui: &mut Ui, t: &Tokens, id: &str, title: &str) -> bool {
    let key = egui::Id::new(("layers_group_open", id));
    let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(true);
    ui.add_space(4.0);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, title));
    let icon = if open {
        "chevron_down"
    } else {
        "chevron_right"
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(r.min.x + 8.0, r.center().y), vec2(12.0, 12.0)),
        icon,
        t.text_dim,
        t.accent,
    );
    ui.painter().text(
        pos2(r.min.x + 18.0, r.center().y),
        Align2::LEFT_CENTER,
        title,
        semibold(12.0),
        t.group_label,
    );
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(key, open));
    }
    open
}

/// Status text and data credits under a layer.
fn note(ui: &mut Ui, t: &Tokens, text: &str) {
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: (INDENT + 46.0) as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).size(11.0).color(t.text_dim)).wrap());
        });
}

/// A note with a spinner before it, for something still loading.
fn loading_note(ui: &mut Ui, t: &Tokens, text: &str) {
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: (INDENT + 46.0) as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(11.0).color(t.text_dim));
                ui.add(
                    egui::Label::new(egui::RichText::new(text).size(11.0).color(t.text_dim)).wrap(),
                );
            });
        });
}

fn credit(ui: &mut Ui, text: &str, url: &str) {
    ui.horizontal(|ui| {
        ui.add_space(INDENT + 46.0);
        ui.hyperlink_to(egui::RichText::new(text).size(11.0), url);
    });
}

impl App {
    pub(in crate::app) fn layers_pane(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        let earth = self.explorer.active == globecraft_core::body::Body::Earth;
        if !earth {
            ui.label(
                egui::RichText::new(lang.text("layers-earth-only"))
                    .size(11.5)
                    .color(t.amber),
            );
        }
        if group_heading(ui, &t, "basemap", lang.text("sidebar-imagery-title")) {
            for provider in Provider::ALL {
                let on = self.settings.layers.basemap == provider;
                let icon = match provider {
                    Provider::Satellite => "basemap",
                    Provider::Wayback => "history",
                    Provider::Streets => "road",
                    Provider::Topo | Provider::TerrainDem => "terrain",
                };
                if toc_row(
                    ui,
                    &t,
                    TocRow {
                        icon,
                        label: provider.name(lang),
                        checked: on,
                        enabled: earth,
                        radio: true,
                        indent: 0.0,
                    },
                )
                .clicked()
                    && earth
                {
                    self.run_reporting("layer.basemap", json!({"basemap": provider.code()}));
                }
                if on && provider == Provider::Wayback {
                    self.wayback_controls(ui, &t);
                }
            }
        }
        for group in LayerGroup::ALL {
            if !group_heading(ui, &t, &format!("{group:?}"), lang.text(group.label_key())) {
                continue;
            }
            for layer in MapLayer::ALL.into_iter().filter(|l| l.group() == group) {
                let parent_on = layer.parent().is_none_or(|p| self.layer(p));
                let indent = if layer.parent().is_some() {
                    INDENT
                } else {
                    0.0
                };
                let on = self.layer(layer);
                let resp = toc_row(
                    ui,
                    &t,
                    TocRow {
                        icon: layer.icon(),
                        label: lang.text(layer.label_key()),
                        checked: on,
                        enabled: earth && parent_on,
                        radio: false,
                        indent,
                    },
                );
                if resp.clicked() && earth && parent_on {
                    self.run_reporting("layer.show", json!({"layer": layer.code()}));
                }
                if on {
                    self.layer_details(ui, &t, layer);
                }
            }
            if group == LayerGroup::Geography {
                if self.boundary_loader.is_loading_base_layers() {
                    loading_note(ui, &t, lang.text("boundaries-loading-status"));
                }
                if self.boundary_loader.is_loading_local() {
                    loading_note(ui, &t, lang.text("boundaries-osm-loading-status"));
                }
                // The loader appends; only the latest lines are worth reading here.
                let log: Vec<String> = self
                    .boundary_log
                    .lock()
                    .map(|l| {
                        let skip = l.len().saturating_sub(BOUNDARY_LOG_LINES);
                        l.iter().skip(skip).cloned().collect()
                    })
                    .unwrap_or_default();
                for line in log {
                    note(ui, &t, &line);
                }
                note(ui, &t, lang.text("layers-boundary-attribution"));
            }
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(lang.text("layers-live-overlays-help"))
                .size(11.0)
                .color(t.text_dim),
        );
    }

    fn wayback_controls(&mut self, ui: &mut Ui, t: &Tokens) {
        let lang = self.lang;
        ui.indent("wayback_ui", |ui| {
            if self.wayback_list.is_empty() {
                loading_note(ui, t, lang.text("wayback-loading-status"));
            } else {
                let n = self.wayback_list.len();
                let mut idx = self.wayback_idx;
                ui.horizontal(|ui| {
                    ui.add_space(INDENT);
                    if ui
                        .small_button("◀")
                        .on_hover_text(lang.text("wayback-older-tooltip"))
                        .clicked()
                        && idx + 1 < n
                    {
                        idx += 1;
                    }
                    egui::ComboBox::from_id_salt("wayback_date")
                        .selected_text(self.wayback_list[idx].date.clone())
                        .height(320.0)
                        .show_ui(ui, |ui| {
                            super::selection::size_menu(
                                ui,
                                self.wayback_list.iter().map(|r| r.date.as_str()),
                            );
                            for (i, r) in self.wayback_list.iter().enumerate() {
                                super::selection::full_width_value(ui, &mut idx, i, &r.date);
                            }
                        });
                    if ui
                        .small_button("▶")
                        .on_hover_text(lang.text("wayback-newer-tooltip"))
                        .clicked()
                        && idx > 0
                    {
                        idx -= 1;
                    }
                });
                let compare = self.active_compare();
                ui.horizontal(|ui| {
                    ui.add_space(INDENT);
                    let mut on = compare.is_some();
                    if ui
                        .checkbox(&mut on, lang.text("wayback-compare-label"))
                        .on_hover_text(lang.text("wayback-compare-tooltip"))
                        .changed()
                    {
                        self.run_reporting("layer.waybackCompare", json!({"enabled": on}));
                    }
                    if let Some(compare) = compare
                        && let Some(index) =
                            self.wayback_date_menu(ui, "pane_wayback_compare", compare.index)
                    {
                        let date = self.wayback_list[index].date.clone();
                        self.run_reporting("layer.waybackCompare", json!({"date": date}));
                    }
                });
                note(
                    ui,
                    t,
                    &lang.format(
                        "wayback-availability-status",
                        &[("count", n.to_string().into())],
                    ),
                );
                note(ui, t, lang.text("wayback-fallback-help"));
                self.set_wayback(idx);
            }
            if let Some(e) = &self.wayback_err {
                ui.colored_label(t.red, e);
            }
        });
    }

    /// Settings, status and credits of a visible layer.
    fn layer_details(&mut self, ui: &mut Ui, t: &Tokens, layer: MapLayer) {
        let lang = self.lang;
        match layer {
            MapLayer::StateBorders => self.state_altitude_control(ui),
            MapLayer::Terrain => {
                ui.horizontal(|ui| {
                    ui.add_space(INDENT + 46.0);
                    let mut value = self.settings.layers.terrain_exaggeration;
                    if ui
                        .add(
                            egui::Slider::new(&mut value, 0.5..=3.0)
                                .step_by(0.1)
                                .suffix("×")
                                .text(lang.text("layers-terrain-exaggeration")),
                        )
                        .changed()
                    {
                        self.settings.layers.terrain_exaggeration = value;
                    }
                });
                note(ui, t, lang.text("layers-terrain-help"));
                credit(ui, "© Mapterhorn", "https://mapterhorn.com/attribution");
            }
            MapLayer::Roads => {
                note(
                    ui,
                    t,
                    &self
                        .roads
                        .service
                        .status(self.cam.alt, lang, self.distance_units()),
                );
                credit(
                    ui,
                    "© OpenStreetMap contributors (ODbL)",
                    "https://www.openstreetmap.org/copyright",
                );
            }
            MapLayer::Traffic => {
                ui.horizontal(|ui| {
                    ui.add_space(INDENT + 46.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.tomtom_key)
                            .password(true)
                            .hint_text(lang.text("layers-traffic-key")),
                    );
                });
                note(
                    ui,
                    t,
                    &self
                        .traffic
                        .service
                        .status(&self.tomtom_key, self.cam.alt, lang),
                );
                credit(ui, "TomTom Traffic", "https://developer.tomtom.com/");
            }
            MapLayer::Radar => credit(
                ui,
                lang.text("sidebar-radar-source"),
                "https://www.rainviewer.com/",
            ),
            MapLayer::Clouds => credit(
                ui,
                lang.text("sidebar-cloud-source"),
                "https://github.com/matteason/live-cloud-maps",
            ),
            MapLayer::Flights => {
                ui.horizontal(|ui| {
                    ui.add_space(INDENT + 46.0);
                    ui.vertical(|ui| {
                        super::aircraft::draw_status(ui, &self.live_aircraft.snapshot(), lang)
                    });
                });
                credit(ui, "OpenSky Network", "https://opensky-network.org/");
                credit(ui, "ADSB.lol (ODbL)", "https://www.adsb.lol/");
            }
            MapLayer::Vessels => {
                note(ui, t, &self.live_vessels.status(lang));
                credit(ui, "OpenSeaFeed AIS", "https://openseafeed.com/");
                credit(
                    ui,
                    lang.text("vessels-fallback-source"),
                    "https://www.digitraffic.fi/en/marine-traffic/",
                );
            }
            MapLayer::Satellites => {
                note(ui, t, &self.live_satellites.status(lang));
                credit(ui, "CelesTrak", "https://celestrak.org/");
            }
            MapLayer::Stars if self.sky.image.is_none() => {
                note(ui, t, lang.text("sky-loading"));
            }
            _ => {}
        }
    }

    /// The altitude below which state lines show, edited in the active distance unit and
    /// stored in kilometres.
    fn state_altitude_control(&mut self, ui: &mut Ui) {
        use globecraft_core::units::{DistanceUnits, KM_PER_MILE};
        let imperial = matches!(self.distance_units(), DistanceUnits::Imperial);
        let per_unit = if imperial { KM_PER_MILE } else { 1.0 };
        let mut shown = self.settings.state_max_altitude_km / per_unit;
        ui.horizontal(|ui| {
            ui.add_space(INDENT + 46.0);
            ui.label(
                egui::RichText::new(self.lang.text("layers-state-altitude-label"))
                    .size(11.5)
                    .color(Tokens::get(ui.ctx()).text_dim),
            );
            let response = ui.add(
                egui::DragValue::new(&mut shown)
                    .speed(10.0)
                    .max_decimals(0)
                    .suffix(if imperial { " mi" } else { " km" }),
            );
            if response.changed() && shown.is_finite() {
                self.settings.state_max_altitude_km = (shown * per_unit)
                    .clamp(0.0, globecraft_data::settings::STATE_MAX_ALTITUDE_MAX_KM);
            }
        });
    }
}
