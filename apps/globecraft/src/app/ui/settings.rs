//! Searchable settings shell; category widgets are separate from persistence.
use super::super::App;
use eframe::egui;
use globecraft_core::language::{Lang, fold_name};
use globecraft_data::settings::Settings;
mod controllers;
mod keys;
mod map_style;
mod network;
mod sections;
mod shortcuts;
mod sky;
pub(super) const MAX_UI_SLIDER_SCALE: f32 = 2.0;
const WINDOW_SIZE_POINTS: egui::Vec2 = egui::vec2(760.0, 560.0);
const WINDOW_MIN_SIZE_POINTS: egui::Vec2 = egui::vec2(600.0, 380.0);
/// The pages' sidebar in the Settings window.
const SIDEBAR_WIDTH_POINTS: f32 = 180.0;
const MIN_BODY_HEIGHT_POINTS: f32 = 160.0;
const FOOTER_HEIGHT_POINTS: f32 = 40.0;

/// The Settings pages, in the sidebar's order (`docs/plans/2026-11-09-ribbon-organization.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Category {
    #[default]
    General,
    Appearance,
    MapStyle,
    Navigation,
    Globe,
    Data,
    Files,
}
impl Category {
    const ALL: [Self; 7] = [
        Self::General,
        Self::Appearance,
        Self::MapStyle,
        Self::Navigation,
        Self::Globe,
        Self::Data,
        Self::Files,
    ];
    fn name_id(self) -> &'static str {
        match self {
            Self::General => "settings-category-general",
            Self::Appearance => "settings-category-appearance",
            Self::MapStyle => "settings-category-map-style",
            Self::Navigation => "settings-category-navigation",
            Self::Globe => "settings-category-globe",
            Self::Data => "settings-category-cache",
            Self::Files => "settings-category-files",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::General => "settings",
            Self::Appearance => "dark",
            Self::MapStyle => "symbology",
            Self::Navigation => "compass",
            Self::Globe => "globe_view",
            Self::Data => "web_data",
            Self::Files => "save",
        }
    }
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::General => &[
                "settings-language-label",
                "settings-experience-label",
                "settings-distance-units-label",
                "coordinates-format-label",
                "coordinates-format-dd",
                "coordinates-format-dms",
                "coordinates-format-ddm",
                "coordinates-format-utm",
                "coordinates-format-mgrs",
                "coordinates-input-help",
                "settings-startup-view-label",
                "settings-startup-help",
            ],
            Self::Appearance => &[
                "settings-theme-label",
                "settings-ui-font-label",
                "settings-map-font-label",
                "settings-ui-scale-label",
                "settings-map-scale-label",
            ],
            Self::MapStyle => map_style::KEYWORDS,
            Self::Navigation => &[
                "settings-hide-navigation-controls-toggle",
                "settings-navigator-style-label",
                "settings-navigator-globe",
                "settings-navigator-classic",
                "settings-pan-sensitivity",
                "settings-rotation-sensitivity",
                "settings-zoom-sensitivity",
                "settings-invert-scroll",
                "settings-category-controllers",
                "controllers-enabled",
                "controllers-help",
                "controllers-deadzone",
                "controllers-invert",
            ],
            Self::Globe => &[
                "settings-earth-model-label",
                "earth-model-wgs84",
                "earth-model-sphere",
                "layers-stars-toggle",
                "layers-milky-way-toggle",
                "sky-help",
                "settings-category-graphics",
                "settings-uncap-fps-toggle",
                "settings-frame-sync-help",
            ],
            Self::Data => &[
                "settings-cache-section-title",
                "settings-cache-clear-help",
                "settings-cache-decoded-label",
                "settings-cache-disk-label",
                "settings-cache-restart-help",
                "settings-category-network",
                "settings-network-intro",
                "settings-network-group-basemaps",
                "settings-network-group-map-data",
                "settings-network-group-search",
                "settings-network-group-live",
                "settings-network-group-updates",
                "settings-network-group-user",
                "settings-network-group-assistant",
                "settings-network-service-wayback",
                "settings-network-service-clouds",
                "settings-network-service-map-data",
                "settings-network-service-updates",
                "settings-network-user-layer-help",
                "settings-network-assistant-help",
                "settings-category-keys",
                "settings-keys-intro",
                "keys-tomtom",
                "layers-traffic-key",
            ],
            Self::Files => &[
                "settings-screenshot-folder-label",
                "settings-kml-folder-label",
                "settings-folder-creation-help",
                "settings-shortcuts-title",
                "cmd-ui-ribbon-reset-tools",
            ],
        }
    }
    fn matches(self, query: &str, lang: Lang) -> bool {
        query.is_empty()
            || std::iter::once(self.name_id())
                .chain(self.keywords().iter().copied())
                .any(|id| fold_name(lang.text(id)).contains(query))
            || (self == Self::Data && network::network_matches(query))
    }
}
#[derive(Default)]
pub(in crate::app) struct SettingsUi {
    category: Category,
    query: String,
    confirm_reset: bool,
    confirm_cache: bool,
}
impl App {
    pub(in crate::app) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.windows.settings {
            return;
        }
        self.refresh_disk_usage(ctx);
        let lang = self.lang;
        let previous_model = self.settings.earth_model;
        let mut open = self.windows.settings;
        let mut close_requested = false;
        let mut language_preference = self.language_preference;
        let mut state = std::mem::take(&mut self.settings_ui);
        let (still_open, _) = super::surface::Surface::dialog(
            "settings-window",
            lang.text("settings-title"),
            super::surface::Width::Large,
        )
        .resizable(WINDOW_MIN_SIZE_POINTS)
        .height(WINDOW_SIZE_POINTS.y)
        .show(ctx, lang, |ui| {
            // A sidebar of pages with the search above it; the page on the right.
            egui::Panel::left("settings-pages")
                .resizable(false)
                .exact_size(SIDEBAR_WIDTH_POINTS)
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    left: 0,
                    right: 8,
                    top: 0,
                    bottom: 0,
                }))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .hint_text(lang.text("settings-search-placeholder"))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    let query = fold_name(&state.query);
                    for category in Category::ALL {
                        let shown = query.is_empty()
                            || category.matches(&query, lang)
                            || (category == Category::Data && self.network_dynamic_matches(&query));
                        if !shown {
                            continue;
                        }
                        if super::surface::sidebar_item(
                            ui,
                            category.icon(),
                            lang.text(category.name_id()),
                            state.category == category,
                        )
                        .clicked()
                        {
                            state.category = category;
                            state.query.clear();
                        }
                    }
                });
            let query = fold_name(&state.query);
            let categories: Vec<_> = Category::ALL
                .into_iter()
                .filter(|category| {
                    if query.is_empty() {
                        *category == state.category
                    } else {
                        category.matches(&query, lang)
                            || (*category == Category::Data && self.network_dynamic_matches(&query))
                    }
                })
                .collect();
            let height = (ui.available_height() - FOOTER_HEIGHT_POINTS).max(MIN_BODY_HEIGHT_POINTS);
            egui::ScrollArea::vertical()
                .id_salt("settings-body")
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if categories.is_empty() {
                        ui.label(lang.format(
                            "settings-search-empty",
                            &[("query", state.query.as_str().into())],
                        ));
                    }
                    for category in categories {
                        ui.push_id(category as u8, |ui| {
                            ui.heading(lang.text(category.name_id()));
                            ui.add_space(4.0);
                            self.settings_page(ui, category, &mut state, &mut language_preference);
                            ui.add_space(ui.spacing().item_spacing.y);
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(lang.text("settings-reset-action")).clicked() {
                    state.confirm_reset = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(lang.text("settings-close-action")).clicked() {
                        close_requested = true;
                    }
                });
            });
            if state.confirm_reset {
                ui.label(lang.text("settings-reset-confirm"));
                ui.horizontal(|ui| {
                    if ui.button(lang.text("settings-confirm-action")).clicked() {
                        self.settings = Settings::default();
                        self.windows.ui_scale_edit = self.settings.ui_scale;
                        state.confirm_reset = false;
                    }
                    if ui.button(lang.text("settings-cancel-action")).clicked() {
                        state.confirm_reset = false;
                    }
                });
            }
        });
        open &= still_open;
        if language_preference != self.language_preference {
            self.set_language_preference(language_preference);
            ctx.request_repaint();
        }
        if previous_model != self.settings.earth_model
            && self.explorer.active == globecraft_core::body::Body::Earth
        {
            self.fly = None;
            self.measure.paths.end_move();
            self.measure.drag = None;
        }
        self.settings_ui = state;
        self.windows.settings = open && !close_requested;
    }
}

impl App {
    /// A page: its sections, each under a subheading when the page has several.
    fn settings_page(
        &mut self,
        ui: &mut egui::Ui,
        category: Category,
        state: &mut SettingsUi,
        language_preference: &mut globecraft_core::language::LanguagePreference,
    ) {
        let lang = self.lang;
        let section = |ui: &mut egui::Ui, title: &str| {
            ui.add_space(8.0);
            ui.strong(title);
            ui.separator();
        };
        match category {
            Category::General => {
                self.settings_general(ui, language_preference);
                section(ui, lang.text("settings-units-section-title"));
                self.settings_units(ui);
                section(ui, lang.text("settings-startup-view-label"));
                self.settings_startup(ui);
            }
            Category::Appearance => self.settings_appearance(ui),
            Category::MapStyle => self.settings_map_style(ui),
            Category::Navigation => {
                self.settings_navigation(ui);
                section(ui, lang.text("settings-category-controllers"));
                self.settings_controllers(ui);
            }
            Category::Globe => {
                self.settings_earth_model(ui);
                section(ui, lang.text("sidebar-sky-title"));
                self.settings_sky(ui);
                section(ui, lang.text("settings-category-graphics"));
                self.settings_graphics(ui);
            }
            Category::Data => {
                self.settings_cache(ui, &mut state.confirm_cache);
                section(ui, lang.text("settings-category-network"));
                self.settings_network(ui);
                section(ui, lang.text("settings-category-keys"));
                self.settings_keys(ui);
            }
            Category::Files => {
                self.settings_files(ui);
                section(ui, lang.text("settings-shortcuts-title"));
                self.settings_shortcuts(ui);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_matches_localized_labels_and_help() {
        assert!(Category::General.matches(&fold_name("MGRS"), Lang::En));
        assert!(Category::Globe.matches(&fold_name("Earth model"), Lang::En));
        assert!(Category::Globe.matches(&fold_name("Modelo da Terra"), Lang::PtBr));
        assert!(Category::General.matches(&fold_name("distância"), Lang::PtBr));
        assert!(Category::Navigation.matches(&fold_name("sensibilidade"), Lang::PtBr));
        assert!(!Category::Files.matches("mgrs", Lang::En));
        assert!(Category::MapStyle.matches(&fold_name("Width"), Lang::En));
        assert!(Category::MapStyle.matches(&fold_name("espessura"), Lang::PtBr));
        assert!(Category::Data.matches(&fold_name("github"), Lang::En));
        assert!(Category::Data.matches(&fold_name("OpenSky"), Lang::En));
        assert!(Category::Data.matches(&fold_name("tile.openstreetmap.org"), Lang::En));
        assert!(Category::Files.matches(&fold_name("keyboard shortcuts"), Lang::En));
        assert!(!Category::Files.matches(&fold_name("github"), Lang::En));
        for lang in [Lang::En, Lang::PtBr] {
            for category in Category::ALL {
                assert!(category.matches(&fold_name(lang.text(category.name_id())), lang));
            }
        }
    }
}
