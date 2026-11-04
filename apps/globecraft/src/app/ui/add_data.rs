//! The Add Data window (roadmap 2.1–2.3): the library of checked sources, a form for a
//! web map (XYZ or WMTS REST tiles, or a WMS with its layer list), and a form for
//! points and lines (GeoJSON, GPX, CSV) from a file or a URL. Each form can fetch a
//! sample first ("Test") so a broken address shows before it becomes a layer.
use crate::app::App;
use eframe::egui;
use globecraft_core::language::Lang;
use globecraft_data::user_data::{
    FeatureFormat, UserLayer, UserSource, WmsLayer, is_url,
    presets::{self, Category, Longevity},
    wms_template,
};
use std::sync::{Arc, Mutex};

/// The level a web map's test tile is fetched at.
const TEST_ZOOM: u8 = 2;
const DEFAULT_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 176, 0);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Library,
    WebMap,
    Features,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WebKind {
    #[default]
    Xyz,
    Wms,
}

/// Where a background request leaves its answer.
type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// A background request and, once it answered, its result.
struct Pending<T> {
    slot: Option<Slot<T>>,
    result: Option<Result<T, String>>,
}

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self {
            slot: None,
            result: None,
        }
    }
}

impl<T: Send + 'static> Pending<T> {
    fn start(
        &mut self,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        ctx: &egui::Context,
        work: impl Future<Output = Result<T, String>> + Send + 'static,
    ) {
        let slot = Arc::new(Mutex::new(None));
        self.slot = Some(slot.clone());
        self.result = None;
        let ctx = ctx.clone();
        runtime.spawn(async move {
            let result = work.await;
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(result);
            }
            ctx.request_repaint();
        });
    }
    /// Takes the answer when it has come.
    fn poll(&mut self) {
        let done = self
            .slot
            .as_ref()
            .and_then(|slot| slot.lock().ok().and_then(|mut s| s.take()));
        if let Some(result) = done {
            self.result = Some(result);
            self.slot = None;
        }
    }
}

impl<T> Pending<T> {
    fn running(&self) -> bool {
        self.slot.is_some()
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
}

pub(in crate::app) struct AddDataDialog {
    tab: Tab,
    web_kind: WebKind,
    url: String,
    tile_512: bool,
    max_zoom: u8,
    wms_layers: Pending<Vec<WmsLayer>>,
    wms_choice: usize,
    name: String,
    attribution: String,
    web_test: Pending<(u32, u32)>,
    location: String,
    /// `None`: from the file name.
    format: Option<FeatureFormat>,
    refresh_minutes: u32,
    /// The property points are sized by; empty finds the usual ones.
    size_by: String,
    color: egui::Color32,
    feature_test: Pending<(usize, usize)>,
}

impl Default for AddDataDialog {
    fn default() -> Self {
        Self {
            tab: Tab::default(),
            web_kind: WebKind::default(),
            url: String::new(),
            tile_512: false,
            max_zoom: 18,
            wms_layers: Pending::default(),
            wms_choice: 0,
            name: String::new(),
            attribution: String::new(),
            web_test: Pending::default(),
            location: String::new(),
            format: None,
            refresh_minutes: 0,
            size_by: String::new(),
            color: DEFAULT_COLOR,
            feature_test: Pending::default(),
        }
    }
}

/// What the window asks the application to do.
enum Action {
    None,
    Close,
    Add(UserLayer),
    OpenKml,
}

fn format_name(format: Option<FeatureFormat>) -> &'static str {
    match format {
        None => "add-data-format-auto",
        Some(FeatureFormat::GeoJson) => "add-data-format-geojson",
        Some(FeatureFormat::Gpx) => "add-data-format-gpx",
        Some(FeatureFormat::Csv) => "add-data-format-csv",
    }
}

fn result_line<T>(ui: &mut egui::Ui, pending: &Pending<T>, lang: Lang, ok: impl Fn(&T) -> String) {
    if pending.running() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(lang.text("add-data-testing-status"));
        });
    }
    match &pending.result {
        Some(Ok(value)) => {
            ui.colored_label(egui::Color32::from_rgb(90, 190, 110), ok(value));
        }
        Some(Err(error)) => {
            ui.colored_label(
                egui::Color32::from_rgb(235, 100, 90),
                lang.format("add-data-test-failed", &[("error", error.as_str().into())]),
            );
        }
        None => {}
    }
}

impl AddDataDialog {
    /// The web map form's layer, when its fields make one.
    fn web_layer(&self, lang: Lang) -> Option<UserLayer> {
        let url = self.url.trim();
        if !is_url(url) {
            return None;
        }
        let template = match self.web_kind {
            WebKind::Xyz => {
                let tiled = ["{z}", "{x}"].iter().all(|p| url.contains(p))
                    && (url.contains("{y}") || url.contains("{-y}"));
                (tiled || url.contains("{bbox}")).then(|| url.to_owned())?
            }
            WebKind::Wms => {
                let layers = self.wms_layers.result.as_ref()?.as_ref().ok()?;
                wms_template(url, &layers.get(self.wms_choice)?.name)
            }
        };
        let fallback = match self.web_kind {
            WebKind::Wms => self
                .wms_layers
                .result
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .and_then(|layers| layers.get(self.wms_choice))
                .map(|layer| layer.title.clone()),
            WebKind::Xyz => None,
        };
        Some(UserLayer {
            id: 0,
            name: Some(self.name.trim().to_owned())
                .filter(|n| !n.is_empty())
                .or(fallback)
                .unwrap_or_else(|| lang.text("add-data-web-default-name").to_owned()),
            source: UserSource::Tiles {
                template,
                min_zoom: 0,
                max_zoom: self.max_zoom,
                tile_px: if self.tile_512 { 512 } else { 256 },
            },
            visible: true,
            opacity: 1.0,
            attribution: self.attribution.trim().to_owned(),
            color: "#ffb000".into(),
            preset: None,
        })
    }

    fn feature_format(&self) -> Option<FeatureFormat> {
        self.format
            .or_else(|| FeatureFormat::guess(self.location.trim()))
    }

    /// The features form's layer, when its fields make one.
    fn feature_layer(&self) -> Option<UserLayer> {
        let location = self.location.trim();
        if location.is_empty() {
            return None;
        }
        let format = self.feature_format()?;
        let stem = location
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(location)
            .split(['?', '#'])
            .next()
            .unwrap_or(location);
        let stem = stem.rsplit_once('.').map_or(stem, |(stem, _)| stem);
        let [r, g, b, _] = self.color.to_array();
        Some(UserLayer {
            id: 0,
            name: Some(self.name.trim().to_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| stem.to_owned()),
            source: UserSource::Features {
                location: location.to_owned(),
                format,
                refresh_minutes: if is_url(location) {
                    self.refresh_minutes
                } else {
                    0
                },
                value_field: Some(self.size_by.trim().to_owned()).filter(|f| !f.is_empty()),
            },
            visible: true,
            opacity: 1.0,
            attribution: self.attribution.trim().to_owned(),
            color: format!("#{r:02x}{g:02x}{b:02x}"),
            preset: None,
        })
    }

    fn show(
        &mut self,
        ctx: &egui::Context,
        lang: Lang,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        added_presets: &[String],
        advanced: bool,
    ) -> Action {
        self.wms_layers.poll();
        self.web_test.poll();
        self.feature_test.poll();
        let mut action = Action::None;
        let mut open = true;
        egui::Window::new(lang.text("add-data-title"))
            .id(egui::Id::new("add_data_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                // Basic (Settings) offers the library; the forms stay reachable
                // when a command opened one.
                if advanced || self.tab != Tab::Library {
                    ui.horizontal(|ui| {
                        for (tab, key) in [
                            (Tab::Library, "add-data-tab-library"),
                            (Tab::WebMap, "add-data-tab-web"),
                            (Tab::Features, "add-data-tab-features"),
                        ] {
                            ui.selectable_value(&mut self.tab, tab, lang.text(key));
                        }
                    });
                } else {
                    ui.weak(lang.text("add-data-advanced-hint"));
                }
                ui.separator();
                match self.tab {
                    Tab::Library => self.library(ui, lang, added_presets, &mut action),
                    Tab::WebMap => self.web_form(ui, ctx, lang, runtime, &mut action),
                    Tab::Features => self.feature_form(ui, ctx, lang, runtime, &mut action),
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.weak(lang.text("add-data-kml-help"));
                    if ui.button(lang.text("add-data-kml-action")).clicked() {
                        action = Action::OpenKml;
                    }
                });
            });
        if !open {
            return Action::Close;
        }
        action
    }

    fn library(&mut self, ui: &mut egui::Ui, lang: Lang, added: &[String], action: &mut Action) {
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                for category in Category::ALL {
                    ui.strong(lang.text(category.label_key()));
                    for preset in presets::PRESETS.iter().filter(|p| p.category == category) {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() - 90.0);
                                ui.label(egui::RichText::new(lang.text(preset.name_key)).strong());
                                ui.label(lang.text(preset.help_key));
                                let commercial = if preset.commercial_use {
                                    "add-data-commercial-yes"
                                } else {
                                    "add-data-commercial-no"
                                };
                                let longevity = match preset.longevity {
                                    Longevity::Institutional => "add-data-longevity-institutional",
                                    Longevity::Goodwill => "add-data-longevity-goodwill",
                                };
                                ui.weak(format!(
                                    "{} · {} · {}",
                                    preset.licence,
                                    lang.text(commercial),
                                    lang.text(longevity)
                                ));
                            });
                            let present = added.iter().any(|code| code == preset.code);
                            let label = if present {
                                "add-data-added-label"
                            } else {
                                "add-data-add-action"
                            };
                            if ui
                                .add_enabled(!present, egui::Button::new(lang.text(label)))
                                .clicked()
                            {
                                *action = Action::Add(UserLayer {
                                    id: 0,
                                    name: lang.text(preset.name_key).to_owned(),
                                    source: preset.source(),
                                    visible: true,
                                    opacity: preset.opacity,
                                    attribution: preset.attribution.to_owned(),
                                    color: preset.color.to_owned(),
                                    preset: Some(preset.code.to_owned()),
                                });
                            }
                        });
                        ui.add_space(6.0);
                    }
                }
            });
    }

    fn web_form(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        lang: Lang,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        action: &mut Action,
    ) {
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut self.web_kind,
                WebKind::Xyz,
                lang.text("add-data-web-xyz"),
            );
            ui.radio_value(
                &mut self.web_kind,
                WebKind::Wms,
                lang.text("add-data-web-wms"),
            );
        });
        egui::Grid::new("add_data_web_form")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(lang.text(match self.web_kind {
                    WebKind::Xyz => "add-data-web-template-label",
                    WebKind::Wms => "add-data-web-service-label",
                }));
                let hint = match self.web_kind {
                    WebKind::Xyz => "https://…/{z}/{x}/{y}.png",
                    WebKind::Wms => "https://…/wms",
                };
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.url)
                            .hint_text(hint)
                            .desired_width(380.0),
                    )
                    .changed()
                {
                    self.web_test.clear();
                    self.wms_layers.clear();
                }
                ui.end_row();
                match self.web_kind {
                    WebKind::Xyz => {
                        ui.label(lang.text("add-data-web-tile-size-label"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.tile_512, false, "256 px");
                            ui.radio_value(&mut self.tile_512, true, "512 px");
                        });
                        ui.end_row();
                        ui.label(lang.text("add-data-web-max-zoom-label"));
                        ui.add(egui::Slider::new(&mut self.max_zoom, 1..=22));
                        ui.end_row();
                    }
                    WebKind::Wms => {
                        ui.label(lang.text("add-data-web-layer-label"));
                        ui.horizontal(|ui| {
                            let layers = self
                                .wms_layers
                                .result
                                .as_ref()
                                .and_then(|r| r.as_ref().ok());
                            match layers {
                                Some(layers) if !layers.is_empty() => {
                                    let selected = layers
                                        .get(self.wms_choice)
                                        .map_or("", |layer| layer.title.as_str());
                                    egui::ComboBox::from_id_salt("add_data_wms_layer")
                                        .selected_text(selected)
                                        .width(300.0)
                                        .show_ui(ui, |ui| {
                                            for (index, layer) in layers.iter().enumerate() {
                                                ui.selectable_value(
                                                    &mut self.wms_choice,
                                                    index,
                                                    format!("{} ({})", layer.title, layer.name),
                                                );
                                            }
                                        });
                                }
                                _ => {
                                    ui.weak(lang.text("add-data-web-no-layers"));
                                }
                            }
                            if ui
                                .add_enabled(
                                    is_url(self.url.trim()) && !self.wms_layers.running(),
                                    egui::Button::new(lang.text("add-data-web-load-layers")),
                                )
                                .clicked()
                            {
                                let base = self.url.trim().to_owned();
                                self.wms_choice = 0;
                                self.wms_layers.start(runtime, ctx, async move {
                                    globecraft_data::user_data::wms_layers(base)
                                        .await
                                        .map_err(|e| e.to_string())
                                });
                            }
                        });
                        ui.end_row();
                    }
                }
                ui.label(lang.text("add-data-name-label"));
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(380.0));
                ui.end_row();
                ui.label(lang.text("add-data-attribution-label"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.attribution)
                        .hint_text(lang.text("add-data-attribution-hint"))
                        .desired_width(380.0),
                );
                ui.end_row();
            });
        if let Some(Err(error)) = &self.wms_layers.result {
            ui.colored_label(
                egui::Color32::from_rgb(235, 100, 90),
                lang.format("add-data-test-failed", &[("error", error.as_str().into())]),
            );
        }
        let layer = self.web_layer(lang);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    layer.is_some() && !self.web_test.running(),
                    egui::Button::new(lang.text("add-data-test-action")),
                )
                .clicked()
                && let Some(UserSource::Tiles {
                    template, max_zoom, ..
                }) = layer.as_ref().map(|l| l.source.clone())
            {
                self.web_test.start(runtime, ctx, async move {
                    globecraft_data::user_data::test_tile(template, TEST_ZOOM.min(max_zoom))
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            if ui
                .add_enabled(
                    layer.is_some(),
                    egui::Button::new(lang.text("add-data-add-action")),
                )
                .clicked()
                && let Some(layer) = layer.clone()
            {
                *action = Action::Add(layer);
            }
        });
        result_line(ui, &self.web_test, lang, |(w, h)| {
            lang.format(
                "add-data-web-test-ok",
                &[("width", (*w).into()), ("height", (*h).into())],
            )
        });
        ui.weak(lang.text("add-data-web-help"));
    }

    fn feature_form(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        lang: Lang,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        action: &mut Action,
    ) {
        egui::Grid::new("add_data_feature_form")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(lang.text("add-data-location-label"));
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.location)
                                .hint_text(lang.text("add-data-location-hint"))
                                .desired_width(300.0),
                        )
                        .changed()
                    {
                        self.feature_test.clear();
                    }
                    if ui.button(lang.text("add-data-browse-action")).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                lang.text("add-data-file-filter"),
                                &["geojson", "json", "gpx", "csv", "tsv", "txt"],
                            )
                            .pick_file()
                    {
                        self.location = path.to_string_lossy().into_owned();
                        self.feature_test.clear();
                    }
                });
                ui.end_row();
                ui.label(lang.text("add-data-format-label"));
                egui::ComboBox::from_id_salt("add_data_format")
                    .selected_text(lang.text(format_name(self.format)))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.format, None, lang.text(format_name(None)));
                        for format in FeatureFormat::ALL {
                            ui.selectable_value(
                                &mut self.format,
                                Some(format),
                                lang.text(format_name(Some(format))),
                            );
                        }
                    });
                ui.end_row();
                if is_url(self.location.trim()) {
                    ui.label(lang.text("add-data-refresh-label"));
                    ui.add(
                        egui::DragValue::new(&mut self.refresh_minutes)
                            .range(0..=1440)
                            .suffix(" min"),
                    )
                    .on_hover_text(lang.text("add-data-refresh-help"));
                    ui.end_row();
                }
                ui.label(lang.text("add-data-size-by-label"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.size_by)
                        .hint_text(lang.text("add-data-size-by-hint"))
                        .desired_width(300.0),
                );
                ui.end_row();
                ui.label(lang.text("add-data-name-label"));
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(300.0));
                ui.end_row();
                ui.label(lang.text("add-data-color-label"));
                egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut self.color,
                    egui::color_picker::Alpha::Opaque,
                );
                ui.end_row();
                ui.label(lang.text("add-data-attribution-label"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.attribution)
                        .hint_text(lang.text("add-data-attribution-hint"))
                        .desired_width(300.0),
                );
                ui.end_row();
            });
        if !self.location.trim().is_empty() && self.feature_format().is_none() {
            ui.colored_label(
                egui::Color32::from_rgb(235, 170, 70),
                lang.text("add-data-format-unknown"),
            );
        }
        let layer = self.feature_layer();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    layer.is_some() && !self.feature_test.running(),
                    egui::Button::new(lang.text("add-data-test-action")),
                )
                .clicked()
                && let Some(UserSource::Features {
                    location,
                    format,
                    value_field,
                    ..
                }) = layer.as_ref().map(|l| l.source.clone())
            {
                self.feature_test.start(runtime, ctx, async move {
                    globecraft_data::user_data::load_features(location, format, value_field)
                        .await
                        .map(|set| (set.points.len(), set.lines.len()))
                        .map_err(|e| e.to_string())
                });
            }
            if ui
                .add_enabled(
                    layer.is_some(),
                    egui::Button::new(lang.text("add-data-add-action")),
                )
                .clicked()
                && let Some(layer) = layer.clone()
            {
                *action = Action::Add(layer);
            }
        });
        result_line(ui, &self.feature_test, lang, |(points, lines)| {
            lang.format(
                "add-data-features-test-ok",
                &[("points", (*points).into()), ("lines", (*lines).into())],
            )
        });
    }
}

impl App {
    pub(in crate::app) fn add_data_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.add_data else {
            return;
        };
        let added: Vec<String> = self
            .settings
            .user_layers
            .iter()
            .filter_map(|layer| layer.preset.clone())
            .collect();
        let advanced = self.settings.experience.is_advanced();
        match dialog.show(ctx, self.lang, &self.runtime, &added, advanced) {
            Action::None => {}
            Action::Close => self.add_data = None,
            Action::Add(layer) => {
                let name = layer.name.clone();
                self.add_user_layer(layer);
                self.status = self
                    .lang
                    .format("add-data-added-status", &[("name", name.into())]);
                // The library stays open for more; a form is done.
                if self
                    .add_data
                    .as_ref()
                    .is_some_and(|d| d.tab != Tab::Library)
                {
                    self.add_data = None;
                }
                self.settings.sidebar.open = true;
                self.settings.sidebar.panel =
                    globecraft_data::settings::sidebar::SidebarPanel::Layers;
            }
            Action::OpenKml => {
                self.add_data = None;
                self.run_reporting("places.open", serde_json::Value::Null);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_form_makes_a_layer_only_from_a_tile_address() {
        let mut dialog = AddDataDialog {
            url: "https://tiles.example/{z}/{x}/{y}.png".into(),
            ..Default::default()
        };
        let layer = dialog.web_layer(Lang::En).unwrap();
        assert!(matches!(
            layer.source,
            UserSource::Tiles { tile_px: 256, .. }
        ));
        dialog.url = "https://tiles.example/map.png".into();
        assert!(dialog.web_layer(Lang::En).is_none());
        dialog.url = "/home/me/{z}/{x}/{y}.png".into();
        assert!(dialog.web_layer(Lang::En).is_none());
    }

    #[test]
    fn the_features_form_names_the_layer_after_the_file() {
        let dialog = AddDataDialog {
            location: "/home/me/Walks/Alps 2026.gpx".into(),
            ..Default::default()
        };
        let layer = dialog.feature_layer().unwrap();
        assert_eq!(layer.name, "Alps 2026");
        assert!(matches!(
            layer.source,
            UserSource::Features {
                format: FeatureFormat::Gpx,
                refresh_minutes: 0,
                ..
            }
        ));
        let unknown = AddDataDialog {
            location: "/home/me/photo.jpeg".into(),
            ..Default::default()
        };
        assert!(unknown.feature_layer().is_none());
    }
}
