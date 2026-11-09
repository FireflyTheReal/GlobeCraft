//! Sources to add in one click (roadmap 2.5), in the side menus of Map › My data ▾:
//! starred and recently added sources, and the library by category. (They were the
//! Data pane; the user's own layers are in the Layers pane.)
use super::icons;
use super::theme::Tokens;
use crate::app::App;
use eframe::egui::{self, CornerRadius, Sense, Ui, vec2};
use globecraft_data::user_data::{
    UserLayer, UserSource,
    presets::{self, Category},
};

/// Width of the source lists' side menus.
const SIDE_MENU_WIDTH_POINTS: f32 = 340.0;

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

/// File-name filter of exported source lists.
const SOURCES_FILTER: (&str, &[&str]) = ("GlobeCraft sources (JSON)", &["json", "JSON"]);

impl App {
    fn pick_sources_export(&mut self) {
        let sources = self.settings.saved_sources.favorites.clone();
        if sources.is_empty() {
            self.status = self.lang.text("data-pane-export-empty").to_owned();
            return;
        }
        let dialog = rfd::FileDialog::new()
            .add_filter(SOURCES_FILTER.0, SOURCES_FILTER.1)
            .set_file_name("globecraft-sources.json")
            .set_title(self.lang.text("data-pane-export-help"));
        if let Some(path) = dialog.save_file()
            && let Err(error) = self.file_jobs.save_sources(path, sources)
        {
            self.status = format!("Unable to export sources: {error}");
        }
    }

    fn pick_sources_import(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter(SOURCES_FILTER.0, SOURCES_FILTER.1)
            .set_title(self.lang.text("data-pane-import-help"));
        if let Some(path) = dialog.pick_file()
            && let Err(error) = self.file_jobs.open_sources(path)
        {
            self.status = format!("Unable to import sources: {error}");
        }
    }

    /// Rows of sources: a click on Add adds one, the star keeps it in Favorites.
    fn source_rows(&mut self, ui: &mut Ui, layers: &[(UserLayer, String)]) {
        let t = Tokens::get(ui.ctx());
        ui.set_min_width(SIDE_MENU_WIDTH_POINTS);
        let mut add: Option<UserLayer> = None;
        let mut toggle: Option<UserLayer> = None;
        for (layer, note) in layers {
            match source_row(ui, &t, self, layer, Some(note)) {
                RowAction::Add => add = Some(layer.clone()),
                RowAction::Star => toggle = Some(layer.clone()),
                RowAction::None => {}
            }
        }
        if let Some(layer) = toggle {
            self.settings.saved_sources.toggle_favorite(&layer);
        }
        if let Some(layer) = add {
            let name = layer.name.clone();
            self.add_user_layer(layer);
            self.status = self
                .lang
                .format("add-data-added-status", &[("name", name.into())]);
        }
    }

    /// A dim line where a list is empty.
    fn empty_note(&self, ui: &mut Ui, key: &str) {
        let t = Tokens::get(ui.ctx());
        ui.set_min_width(SIDE_MENU_WIDTH_POINTS);
        ui.label(
            egui::RichText::new(self.lang.text(key))
                .size(11.5)
                .color(t.text_dim),
        );
    }

    /// Favorites ▸: the starred sources, and their export and import.
    pub(in crate::app) fn favorites_menu(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let rows: Vec<_> = self
            .settings
            .saved_sources
            .favorites
            .iter()
            .map(|layer| (layer.clone(), location(layer)))
            .collect();
        if rows.is_empty() {
            self.empty_note(ui, "data-pane-favorites-empty");
        }
        self.source_rows(ui, &rows);
        ui.separator();
        if ui.button(lang.text("data-pane-export-help")).clicked() {
            self.pick_sources_export();
            ui.close();
        }
        if ui.button(lang.text("data-pane-import-help")).clicked() {
            self.pick_sources_import();
            ui.close();
        }
    }

    /// Recent ▸: the sources added lately.
    pub(in crate::app) fn recent_menu(&mut self, ui: &mut Ui) {
        let rows: Vec<_> = self
            .settings
            .saved_sources
            .recent
            .iter()
            .map(|layer| (layer.clone(), location(layer)))
            .collect();
        if rows.is_empty() {
            self.empty_note(ui, "data-pane-recent-empty");
        }
        self.source_rows(ui, &rows);
    }

    /// A library category ▸: its sources.
    pub(in crate::app) fn library_menu(&mut self, ui: &mut Ui, category: Category) {
        let lang = self.lang;
        let rows: Vec<_> = presets::PRESETS
            .iter()
            .filter(|p| p.category == category)
            .map(|preset| {
                (
                    UserLayer {
                        id: 0,
                        name: lang.text(preset.name_key).to_owned(),
                        source: preset.source(),
                        visible: true,
                        opacity: preset.opacity,
                        attribution: preset.attribution.to_owned(),
                        color: preset.color.to_owned(),
                        preset: Some(preset.code.to_owned()),
                    },
                    preset.licence.to_owned(),
                )
            })
            .collect();
        self.source_rows(ui, &rows);
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
            value_field: None,
        };
        assert_eq!(location(&layer), "ride.gpx");
    }
}
