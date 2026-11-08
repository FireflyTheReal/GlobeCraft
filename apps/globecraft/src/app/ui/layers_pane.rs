//! The Layers pane: the basemap and every toggleable layer as a table of contents, grouped,
//! with each live layer's status and data credits under it while it is shown.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::language::fold_name;
use globecraft_core::layers::{LayerGroup, MapLayer};
use globecraft_data::tiles::Provider;
use serde_json::json;

const ROW_H: f32 = 24.0;
const INDENT: f32 = 18.0;

/// The Layers pane's search text and active-state filter. Not saved: it only narrows
/// the pane for the session.
#[derive(Default)]
pub(in crate::app) struct LayersUi {
    query: String,
    active_only: bool,
}

/// Whether a row is shown: its name or its group's name contains the folded query,
/// and it is on when the active-only filter is on.
fn matches_filter(query: &str, names: &[&str], active_only: bool, active: bool) -> bool {
    (!active_only || active)
        && (query.is_empty() || names.iter().any(|name| fold_name(name).contains(query)))
}

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

/// A group heading with a disclosure arrow; returns whether it is open. A search
/// keeps every group with matches open without changing the stored state.
fn group_heading(ui: &mut Ui, t: &Tokens, id: &str, title: &str, force_open: bool) -> bool {
    let key = egui::Id::new(("layers_group_open", id));
    let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(true) || force_open;
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
    if resp.clicked() && !force_open {
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

/// A feature layer's legend: its marker and line in the layer colour, and how many
/// points and lines it holds.
fn feature_legend(
    ui: &mut Ui,
    t: &Tokens,
    [r, g, b]: [u8; 3],
    features: &globecraft_data::user_data::FeatureSet,
    lang: globecraft_core::language::Lang,
) {
    let (points, lines) = (features.points.len(), features.lines.len());
    const SWATCH: egui::Vec2 = vec2(26.0, 12.0);
    ui.horizontal(|ui| {
        ui.add_space(INDENT + 24.0);
        let (swatch, _) = ui.allocate_exact_size(SWATCH, Sense::hover());
        let color = egui::Color32::from_rgb(r, g, b);
        let painter = ui.painter();
        if lines > 0 {
            painter.line_segment(
                [
                    swatch.left_center() + vec2(1.0, 3.0),
                    swatch.right_center() + vec2(-1.0, -3.0),
                ],
                Stroke::new(2.0, color),
            );
        }
        if points > 0 {
            painter.circle_filled(swatch.center(), 4.0, color);
            painter.circle_stroke(swatch.center(), 4.0, Stroke::new(1.0, t.text_dim));
        }
        ui.label(
            egui::RichText::new(lang.format(
                "layers-user-legend",
                &[("points", points.into()), ("lines", lines.into())],
            ))
            .size(11.0)
            .color(t.text_dim),
        );
    });
    // Sized by a value: a small and a large marker with the smallest and largest.
    if let (Some(field), Some((low, high))) = (&features.value_field, features.value_range) {
        ui.horizontal(|ui| {
            ui.add_space(INDENT + 24.0);
            let (swatch, _) = ui.allocate_exact_size(vec2(26.0, 14.0), Sense::hover());
            let color = egui::Color32::from_rgb(r, g, b);
            ui.painter()
                .circle_filled(swatch.left_center() + vec2(5.0, 0.0), 2.5, color);
            ui.painter()
                .circle_filled(swatch.right_center() - vec2(8.0, 0.0), 6.0, color);
            let number = |v: f64| {
                if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{}", v as i64)
                } else {
                    format!("{v:.1}")
                }
            };
            ui.label(
                egui::RichText::new(lang.format(
                    "layers-user-legend-size",
                    &[
                        ("field", field.clone().into()),
                        ("low", number(low).into()),
                        ("high", number(high).into()),
                    ],
                ))
                .size(11.0)
                .color(t.text_dim),
            );
        });
    }
}

/// A web map's legend image (from its WMS), at most as wide as the pane allows
/// and `MAX_LEGEND_HEIGHT` tall, never enlarged.
fn image_legend(ui: &mut Ui, texture: &egui::TextureHandle) {
    const MAX_LEGEND_HEIGHT: f32 = 220.0;
    ui.horizontal(|ui| {
        ui.add_space(INDENT + 24.0);
        // Legends are drawn for 96 dpi screens: one pixel to one point.
        let size = texture.size_vec2();
        let room = vec2((ui.available_width() - 4.0).max(1.0), MAX_LEGEND_HEIGHT);
        let fit = (room.x / size.x).min(room.y / size.y).min(1.0);
        ui.image((texture.id(), size * fit));
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
    /// The search field and active-state filter above the Layers list.
    pub(in crate::app) fn layers_search(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.horizontal(|ui| {
            let clear_width = if self.layers_ui.query.is_empty() {
                0.0
            } else {
                ui.spacing().interact_size.y + ui.spacing().item_spacing.x
            };
            let width = (ui.available_width() - clear_width).max(80.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.layers_ui.query)
                    .id(egui::Id::new("layers-search"))
                    .hint_text(lang.text("layers-search-placeholder"))
                    .desired_width(width),
            );
            if !self.layers_ui.query.is_empty()
                && icons::icon_button(ui, "close", lang.text("layers-search-clear")).clicked()
            {
                self.layers_ui.query.clear();
            }
            if response.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.layers_ui.query.clear();
            }
        });
        ui.checkbox(
            &mut self.layers_ui.active_only,
            lang.text("layers-search-active-only"),
        )
        .on_hover_text(lang.text("layers-search-active-help"));
    }

    /// Sets the Layers pane search from a test harness.
    #[cfg(test)]
    pub(in crate::app) fn set_layers_search(&mut self, query: &str, active_only: bool) {
        self.layers_ui.query = query.to_owned();
        self.layers_ui.active_only = active_only;
    }

    pub(in crate::app) fn layers_pane(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        let earth = self.explorer.active == globecraft_core::body::Body::Earth;
        let raw_query = self.layers_ui.query.clone();
        let query = fold_name(&raw_query);
        let active_only = self.layers_ui.active_only;
        let filtering = !query.is_empty() || active_only;
        let mut shown_any = false;
        if let Some(layer) = self.soloed_layer() {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(INDENT);
                ui.label(
                    egui::RichText::new(lang.format(
                        "layers-solo-note",
                        &[("layer", lang.text(layer.label_key()).into())],
                    ))
                    .size(11.5)
                    .color(t.text_dim),
                );
                if ui.small_button(lang.text("layers-solo-show-all")).clicked() {
                    self.run_reporting("layer.solo", serde_json::Value::Null);
                }
            });
        }
        if !earth {
            ui.label(
                egui::RichText::new(lang.text("layers-earth-only"))
                    .size(11.5)
                    .color(t.amber),
            );
        }
        let basemap_title = lang.text("sidebar-imagery-title");
        let providers: Vec<_> = Provider::basemaps()
            .filter(|provider| {
                matches_filter(
                    &query,
                    &[basemap_title, provider.name(lang)],
                    active_only,
                    self.settings.layers.basemap == *provider,
                )
            })
            .collect();
        if !providers.is_empty() {
            shown_any = true;
            if group_heading(ui, &t, "basemap", basemap_title, filtering) {
                for provider in providers {
                    let on = self.settings.layers.basemap == provider;
                    let icon = match provider {
                        Provider::Satellite => "basemap",
                        Provider::Wayback => "history",
                        Provider::Streets => "road",
                        _ => "terrain",
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
        }
        let user_title = lang.text("layers-user-title");
        let user_indices: Vec<usize> = (0..self.settings.user_layers.len())
            .rev()
            .filter(|&index| {
                let layer = &self.settings.user_layers[index];
                matches_filter(
                    &query,
                    &[user_title, layer.name.as_str()],
                    active_only,
                    layer.visible,
                )
            })
            .collect();
        let show_user_group = !user_indices.is_empty()
            || (!active_only
                && self.settings.user_layers.is_empty()
                && matches_filter(&query, &[user_title], active_only, true));
        if show_user_group {
            shown_any = true;
            if group_heading(ui, &t, "user_data", user_title, filtering) {
                self.user_layers_section(ui, &t, earth, &user_indices);
            }
        }
        for group in LayerGroup::ALL {
            let group_title = lang.text(group.label_key());
            let layers: Vec<_> = MapLayer::ALL
                .into_iter()
                .filter(|layer| layer.group() == group && layer.listed())
                .filter(|layer| {
                    matches_filter(
                        &query,
                        &[group_title, lang.text(layer.label_key())],
                        active_only,
                        self.layer(*layer),
                    )
                })
                .collect();
            if layers.is_empty() {
                continue;
            }
            shown_any = true;
            if !group_heading(ui, &t, &format!("{group:?}"), group_title, filtering) {
                continue;
            }
            for layer in layers {
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
                // Alt-click (Option-click) shows this layer alone (roadmap 6.1).
                let solo = ui.input(|input| input.modifiers.alt);
                if resp.clicked() && earth && parent_on {
                    if solo && layer.soloable() {
                        self.run_reporting("layer.solo", json!({"layer": layer.code()}));
                    } else {
                        self.run_reporting("layer.show", json!({"layer": layer.code()}));
                    }
                }
                if on {
                    if layer.has_opacity() {
                        self.layer_opacity_control(ui, layer);
                    }
                    self.layer_details(ui, &t, layer);
                }
            }
            if group == LayerGroup::Geography && !filtering {
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
        if filtering && !shown_any {
            ui.add_space(8.0);
            let message = if query.is_empty() {
                lang.text("layers-search-empty-active").to_owned()
            } else {
                lang.format(
                    "layers-search-empty",
                    &[("query", raw_query.as_str().into())],
                )
            };
            ui.label(egui::RichText::new(message).size(11.5).color(t.text_dim));
        } else if !filtering {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(lang.text("layers-live-overlays-help"))
                    .size(11.0)
                    .color(t.text_dim),
            );
        }
    }

    /// The layers the user added, top of the drawing order first, each with its status,
    /// opacity, order, zoom and remove controls. `indices` are the rows the search
    /// keeps, already in display order.
    fn user_layers_section(&mut self, ui: &mut Ui, t: &Tokens, earth: bool, indices: &[usize]) {
        let lang = self.lang;
        if self.settings.user_layers.is_empty() {
            note(ui, t, lang.text("layers-user-empty"));
        }
        let count = self.settings.user_layers.len();
        for &index in indices {
            let layer = self.settings.user_layers[index].clone();
            let feature = matches!(
                layer.source,
                globecraft_data::user_data::UserSource::Features { .. }
            );
            let resp = toc_row(
                ui,
                t,
                TocRow {
                    icon: if feature {
                        "point_layer"
                    } else {
                        "raster_layer"
                    },
                    label: &layer.name,
                    checked: layer.visible,
                    enabled: earth,
                    radio: false,
                    indent: 0.0,
                },
            );
            if resp.clicked() && earth {
                self.run_reporting("data.show", json!({"id": layer.id}));
            }
            if !layer.visible {
                continue;
            }
            match self.user_layers.status(layer.id) {
                crate::app::user_layers::LayerStatus::Loading => {
                    loading_note(ui, t, lang.text("layers-user-loading"));
                }
                crate::app::user_layers::LayerStatus::Failed(error) => note(
                    ui,
                    t,
                    &lang.format("layers-user-failed", &[("error", error.into())]),
                ),
                crate::app::user_layers::LayerStatus::Ready => {}
            }
            ui.horizontal(|ui| {
                ui.add_space(INDENT + 24.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut opacity = layer.opacity;
                ui.spacing_mut().slider_width = 76.0;
                if ui
                    .add(egui::Slider::new(&mut opacity, 0.0..=1.0).show_value(false))
                    .on_hover_text(lang.text("layers-user-opacity-label"))
                    .changed()
                {
                    self.run_reporting("data.opacity", json!({"id": layer.id, "opacity": opacity}));
                }
                let small = |ui: &mut Ui, icon: &str, help: &str, enabled: bool| {
                    ui.add_enabled_ui(enabled, |ui| icons::icon_button(ui, icon, help))
                        .inner
                        .clicked()
                };
                if small(
                    ui,
                    "up",
                    lang.text("layers-user-up-help"),
                    index + 1 < count,
                ) {
                    self.run_reporting("data.move", json!({"id": layer.id, "direction": "up"}));
                }
                if small(ui, "down", lang.text("layers-user-down-help"), index > 0) {
                    self.run_reporting("data.move", json!({"id": layer.id, "direction": "down"}));
                }
                if feature
                    && small(
                        ui,
                        "zoom_full",
                        lang.text("layers-user-zoom-help"),
                        self.user_layers.bounds(layer.id).is_some(),
                    )
                {
                    self.run_reporting("data.zoomTo", json!({"id": layer.id}));
                }
                if small(ui, "delete", lang.text("layers-user-remove-help"), true) {
                    self.run_reporting("data.remove", json!({"id": layer.id}));
                }
            });
            if feature && let Some(features) = self.user_layers.feature_set(layer.id) {
                feature_legend(ui, t, layer.rgb(), &features, lang);
            }
            if !feature
                && let Some(texture) = self
                    .user_layers
                    .legends
                    .get(ui.ctx(), &self.runtime, &layer)
            {
                image_legend(ui, &texture);
            }
            if !layer.attribution.is_empty() {
                note(ui, t, &layer.attribution);
            }
        }
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            if icons::button(ui, "add_data", lang.text("layers-user-add-action")).clicked() {
                self.run_reporting("ui.addData", serde_json::Value::Null);
            }
        });
    }

    fn wayback_controls(&mut self, ui: &mut Ui, t: &Tokens) {
        let lang = self.lang;
        ui.indent("wayback_ui", |ui| {
            if self.wayback.releases.is_empty() {
                loading_note(ui, t, lang.text("wayback-loading-status"));
            } else {
                let n = self.wayback.releases.len();
                let mut idx = self.wayback.index;
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
                        .selected_text(self.wayback.releases[idx].date.clone())
                        .height(320.0)
                        .show_ui(ui, |ui| {
                            super::selection::size_menu(
                                ui,
                                self.wayback.releases.iter().map(|r| r.date.as_str()),
                            );
                            for (i, r) in self.wayback.releases.iter().enumerate() {
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
                        let date = self.wayback.releases[index].date.clone();
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
            if let Some(e) = &self.wayback.error {
                ui.colored_label(t.red, e);
            }
        });
    }

    /// Settings, status and credits of a visible layer.
    /// The opacity slider of a layer that has one (roadmap 6.1).
    fn layer_opacity_control(&mut self, ui: &mut Ui, layer: MapLayer) {
        let lang = self.lang;
        ui.horizontal(|ui| {
            ui.add_space(INDENT + 24.0);
            ui.spacing_mut().slider_width = 110.0;
            let mut opacity = self.settings.layers.opacity.get(layer);
            if ui
                .add(
                    egui::Slider::new(&mut opacity, 0.0..=1.0)
                        .custom_formatter(|value, _| format!("{:.0} %", value * 100.0))
                        .text(lang.text("layers-opacity-label")),
                )
                .changed()
            {
                self.run_reporting(
                    "layer.opacity",
                    json!({"layer": layer.code(), "opacity": opacity}),
                );
            }
        });
    }

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
            MapLayer::PointsOfInterest | MapLayer::Peaks | MapLayer::Waters | MapLayer::Parks => {
                let status = if self.cam.alt > globecraft_data::poi::MAX_ALTITUDE_KM {
                    Some(
                        lang.format(
                            "transport-zoom-help",
                            &[(
                                "altitude",
                                self.distance_units()
                                    .altitude(globecraft_data::poi::MAX_ALTITUDE_KM)
                                    .into(),
                            )],
                        ),
                    )
                } else if let Some(error) = self.poi_tiles.error() {
                    Some(lang.format("transport-error-status", &[("error", error.into())]))
                } else if self.poi_tiles.loading() {
                    Some(lang.text("transport-loading-status").into())
                } else {
                    None
                };
                if let Some(status) = status {
                    note(ui, t, &status);
                }
                credit(
                    ui,
                    "© OpenStreetMap contributors (ODbL), © OpenMapTiles",
                    "https://www.openstreetmap.org/copyright",
                );
            }
            MapLayer::Railways | MapLayer::Ferries | MapLayer::Airports => {
                let status = if self.cam.alt > globecraft_data::transport::MAX_ALTITUDE_KM {
                    Some(
                        lang.format(
                            "transport-zoom-help",
                            &[(
                                "altitude",
                                self.distance_units()
                                    .altitude(globecraft_data::transport::MAX_ALTITUDE_KM)
                                    .into(),
                            )],
                        ),
                    )
                } else if let Some(error) = self.transport_tiles.error() {
                    Some(lang.format("transport-error-status", &[("error", error.into())]))
                } else if self.transport_tiles.loading() {
                    Some(lang.text("transport-loading-status").into())
                } else {
                    None
                };
                if let Some(status) = status {
                    note(ui, t, &status);
                }
                credit(
                    ui,
                    "© OpenStreetMap contributors (ODbL), © OpenMapTiles",
                    "https://www.openstreetmap.org/copyright",
                );
            }
            MapLayer::Roads => {
                let status = if self.cam.alt > globecraft_data::roads::MAX_ALTITUDE_KM {
                    lang.format(
                        "roads-zoom-help",
                        &[(
                            "altitude",
                            self.distance_units()
                                .altitude(globecraft_data::roads::MAX_ALTITUDE_KM)
                                .into(),
                        )],
                    )
                } else if let Some(error) = self.roads.tiles.error() {
                    lang.format("transport-error-status", &[("error", error.into())])
                } else if self.roads.tiles.loading() {
                    lang.text("transport-loading-status").into()
                } else {
                    lang.text("roads-legend").into()
                };
                if !status.is_empty() {
                    note(ui, t, &status);
                }
                credit(
                    ui,
                    "© OpenStreetMap contributors (ODbL)",
                    "https://www.openstreetmap.org/copyright",
                );
            }
            MapLayer::Traffic => {
                ui.horizontal(|ui| {
                    ui.add_space(INDENT + 46.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.tomtom_key)
                                .password(true)
                                .hint_text(lang.text("layers-traffic-key")),
                        )
                        .changed()
                    {
                        self.store_key(globecraft_data::keys::TOMTOM);
                    }
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
                self.flat_live_objects_control(ui);
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
            MapLayer::SeaNames => {
                if let Some(error) = self.live_marine_names.error() {
                    note(ui, t, &error);
                } else {
                    note(ui, t, lang.text("sea-names-help"));
                }
                credit(
                    ui,
                    lang.text("sea-names-source"),
                    "https://www.marineregions.org/",
                );
            }
            MapLayer::TimeZones => {
                note(ui, t, &self.live_time_zones.status(lang));
                credit(
                    ui,
                    lang.text("timezones-source"),
                    "https://github.com/evansiroky/timezone-boundary-builder",
                );
            }
            MapLayer::Aurora => {
                note(ui, t, &self.live_aurora.status(lang));
                credit(
                    ui,
                    lang.text("aurora-source"),
                    "https://www.swpc.noaa.gov/products/aurora-30-minute-forecast",
                );
            }
            MapLayer::Satellites => {
                self.flat_live_objects_control(ui);
                note(ui, t, &self.live_satellites.status(lang));
                credit(ui, "CelesTrak", "https://celestrak.org/");
            }
            MapLayer::MilkyWay if self.sky.image.is_none() => {
                note(ui, t, lang.text("sky-loading"));
            }
            _ => {}
        }
    }

    /// Aircraft and satellites at their height, or on the ground below them.
    fn flat_live_objects_control(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let mut flat = self.settings.layers.flat_live_objects;
        ui.horizontal(|ui| {
            ui.add_space(INDENT + 46.0);
            if ui
                .checkbox(&mut flat, lang.text("cmd-layer-flat-live-objects"))
                .on_hover_text(lang.text("cmd-layer-flat-live-objects-help"))
                .changed()
            {
                self.run_reporting("layer.flatLiveObjects", json!({"on": flat}));
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::language::Lang;

    #[test]
    fn search_matches_names_groups_and_active_state() {
        let en = Lang::En;
        let pt = Lang::PtBr;
        let country = en.text(MapLayer::CountryBorders.label_key());
        assert!(matches_filter(
            &fold_name("country"),
            &[country],
            false,
            false
        ));
        assert!(!matches_filter(
            &fold_name("traffic"),
            &[country],
            false,
            true
        ));
        assert!(matches_filter("", &[country], false, false));
        assert!(matches_filter("", &[country], true, true));
        assert!(!matches_filter("", &[country], true, false));
        // Group headings are searchable, in both catalogs.
        assert!(matches_filter(
            &fold_name("borders"),
            &[en.text(LayerGroup::Geography.label_key())],
            false,
            false
        ));
        assert!(matches_filter(
            &fold_name("limites"),
            &[pt.text(LayerGroup::Geography.label_key())],
            false,
            false
        ));
        // Folding makes accents optional.
        assert!(matches_filter(
            &fold_name("paises"),
            &[pt.text(MapLayer::CountryBorders.label_key())],
            false,
            false
        ));
    }
}
