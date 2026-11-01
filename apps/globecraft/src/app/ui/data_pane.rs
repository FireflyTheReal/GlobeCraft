//! The Data pane (roadmap 2.5), the Add Data browser beside the globe: starred and
//! recently added sources, the service library and the user's own layers, each
//! added again in one click.
use super::icons;
use super::theme::{Tokens, semibold};
use super::widgets::{primary_button, section_heading};
use crate::app::App;
use eframe::egui::{self, CornerRadius, Sense, Ui, vec2};
use globecraft_data::settings::sidebar::SidebarPanel;
use globecraft_data::user_data::{
    UserLayer, UserSource,
    presets::{self, Category},
};
use serde_json::json;

/// What a row asks for.
enum RowAction {
    None,
    Add,
    Star,
}

/// The star (favorite) toggle: the bookmark icon, in the accent colour when on.
fn star(ui: &mut Ui, t: &Tokens, on: bool, help: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, help));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    // Off: one muted colour (the icon's accent parts too), so on and off differ.
    let ink = if on { t.accent } else { t.text_dim };
    icons::paint(ui.painter(), rect.shrink(4.0), "bookmark", ink, ink);
    response.on_hover_text(help)
}

/// One source: its name and a note, the star and Add (or "Added").
fn source_row(
    ui: &mut Ui,
    t: &Tokens,
    app: &App,
    layer: &UserLayer,
    note: Option<&str>,
) -> RowAction {
    let lang = app.lang;
    let present = app
        .settings
        .user_layers
        .iter()
        .any(|existing| existing.source == layer.source);
    let favorite = app.settings.saved_sources.is_favorite(&layer.source);
    let mut action = RowAction::None;
    ui.horizontal(|ui| {
        let feature = matches!(layer.source, UserSource::Features { .. });
        let (icon_rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(
            ui.painter(),
            icon_rect,
            if feature {
                "point_layer"
            } else {
                "raster_layer"
            },
            t.text_dim,
            t.accent,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = lang.text(if present {
                "add-data-added-label"
            } else {
                "add-data-add-action"
            });
            if ui
                .add_enabled(!present, egui::Button::new(label).small())
                .clicked()
            {
                action = RowAction::Add;
            }
            let help = lang.text(if favorite {
                "data-pane-unstar-help"
            } else {
                "data-pane-star-help"
            });
            if star(ui, t, favorite, help).clicked() {
                action = RowAction::Star;
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&layer.name).strong()).truncate());
                if let Some(note) = note {
                    ui.add(
                        egui::Label::new(egui::RichText::new(note).size(11.0).color(t.text_dim))
                            .truncate(),
                    );
                }
            });
        });
    });
    action
}

/// Where a source's data comes from, shortened for a row.
fn location(layer: &UserLayer) -> String {
    let text = match &layer.source {
        UserSource::Tiles { template, .. } => template,
        UserSource::Features { location, .. } => location,
    };
    let host = text
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .filter(|host| !host.is_empty());
    match host {
        Some(host) => host.to_owned(),
        None => std::path::Path::new(text)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(text)
            .to_owned(),
    }
}

impl App {
    pub(in crate::app) fn data_pane(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        if primary_button(ui, lang.text("cmd-ui-add-data"), true, vec2(0.0, 0.0)).clicked() {
            self.run_reporting("ui.addData", serde_json::Value::Null);
        }
        ui.add_space(6.0);
        let mut add: Option<UserLayer> = None;
        let mut toggle: Option<UserLayer> = None;
        let mut handle = |action: RowAction, layer: &UserLayer| match action {
            RowAction::Add => add = Some(layer.clone()),
            RowAction::Star => toggle = Some(layer.clone()),
            RowAction::None => {}
        };

        section_heading(ui, lang.text("data-pane-favorites-title"));
        if self.settings.saved_sources.favorites.is_empty() {
            ui.label(
                egui::RichText::new(lang.text("data-pane-favorites-empty"))
                    .size(11.5)
                    .color(t.text_dim),
            );
        }
        for layer in &self.settings.saved_sources.favorites {
            handle(
                source_row(ui, &t, self, layer, Some(&location(layer))),
                layer,
            );
        }

        ui.add_space(6.0);
        section_heading(ui, lang.text("data-pane-recent-title"));
        if self.settings.saved_sources.recent.is_empty() {
            ui.label(
                egui::RichText::new(lang.text("data-pane-recent-empty"))
                    .size(11.5)
                    .color(t.text_dim),
            );
        }
        for layer in &self.settings.saved_sources.recent {
            handle(
                source_row(ui, &t, self, layer, Some(&location(layer))),
                layer,
            );
        }

        ui.add_space(6.0);
        section_heading(ui, lang.text("data-pane-services-title"));
        for category in Category::ALL {
            ui.label(
                egui::RichText::new(lang.text(category.label_key()))
                    .font(semibold(11.5))
                    .color(t.text_dim),
            );
            for preset in presets::PRESETS.iter().filter(|p| p.category == category) {
                let layer = UserLayer {
                    id: 0,
                    name: lang.text(preset.name_key).to_owned(),
                    source: preset.source(),
                    visible: true,
                    opacity: preset.opacity,
                    attribution: preset.attribution.to_owned(),
                    color: preset.color.to_owned(),
                    preset: Some(preset.code.to_owned()),
                };
                handle(
                    source_row(ui, &t, self, &layer, Some(preset.licence)),
                    &layer,
                );
            }
        }

        ui.add_space(6.0);
        section_heading(ui, lang.text("data-pane-mine-title"));
        if self.settings.user_layers.is_empty() {
            ui.label(
                egui::RichText::new(lang.text("layers-user-empty"))
                    .size(11.5)
                    .color(t.text_dim),
            );
        }
        let mut show: Option<u64> = None;
        for layer in self.settings.user_layers.iter().rev() {
            let mut visible = layer.visible;
            if ui.checkbox(&mut visible, &layer.name).changed() {
                show = Some(layer.id);
            }
        }
        if !self.settings.user_layers.is_empty()
            && ui.link(lang.text("data-pane-open-layers-action")).clicked()
        {
            self.settings.sidebar.panel = SidebarPanel::Layers;
        }

        if let Some(id) = show {
            self.run_reporting("data.show", json!({"id": id}));
        }
        if let Some(layer) = toggle {
            self.settings.saved_sources.toggle_favorite(&layer);
        }
        if let Some(layer) = add {
            let name = layer.name.clone();
            self.add_user_layer(layer);
            self.status = lang.format("add-data-added-status", &[("name", name.into())]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_name_a_source_by_its_host_or_file() {
        let mut layer = UserLayer {
            id: 0,
            name: "x".into(),
            source: UserSource::Tiles {
                template: "https://tiles.example.org/{z}/{x}/{y}.png".into(),
                min_zoom: 0,
                max_zoom: 10,
                tile_px: 256,
            },
            visible: true,
            opacity: 1.0,
            attribution: String::new(),
            color: "#ffffff".into(),
            preset: None,
        };
        assert_eq!(location(&layer), "tiles.example.org");
        layer.source = UserSource::Features {
            location: "/home/me/tracks/ride.gpx".into(),
            format: globecraft_data::user_data::FeatureFormat::Gpx,
            refresh_minutes: 0,
        };
        assert_eq!(location(&layer), "ride.gpx");
    }
}
