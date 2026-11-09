//! The four ribbon tabs and their groups (`docs/plans/2026-11-09-ribbon-organization.md`).
//! Each group shows at most one large button and two columns of small ones; its other
//! tools are in the group's menu (`tools::EXTRAS`). A set of choices is one button.
use super::widgets::{
    Choice, Look, big, big_with, choice_big, col, group, small, small_menu, small_with,
};
use crate::app::App;
use eframe::egui::Ui;
use globecraft_core::{layers::MapLayer, units::RulerUnit};
use serde_json::{Value, json};

const BASEMAPS: [(&str, &str, &str); 4] = [
    ("satellite", "basemap", "ribbon-basemap-satellite"),
    ("streets", "road", "ribbon-basemap-streets"),
    ("topo", "terrain", "ribbon-basemap-topo"),
    ("wayback", "history", "ribbon-basemap-wayback"),
];

const WORLDS: [(&str, &str, &str); 3] = [
    ("earth", "earth", "top-earth"),
    ("moon", "moon", "top-moon"),
    ("custom", "planet", "top-custom-planet"),
];

impl App {
    pub(super) fn layer_big(&mut self, ui: &mut Ui, layer: MapLayer) {
        let label = short_layer_label(self.lang, layer);
        big_with(
            ui,
            self,
            "layer.show",
            json!({"layer": layer.code()}),
            Look {
                icon: Some(layer.icon()),
                label: Some(label),
                help: Some(self.lang.text(layer.label_key())),
                ..Look::default()
            },
        );
    }

    pub(super) fn layer_small(&mut self, ui: &mut Ui, layer: MapLayer) {
        let label = short_layer_label(self.lang, layer);
        small_with(
            ui,
            self,
            "layer.show",
            json!({"layer": layer.code()}),
            Look {
                icon: Some(layer.icon()),
                label: Some(label),
                help: Some(self.lang.text(layer.label_key())),
                ..Look::default()
            },
        );
    }

    /// Go somewhere, mark it, measure it, capture it, ask about it.
    pub(super) fn explore_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(
            ui,
            self,
            "explore.navigate",
            lang.text("ribbon-group-navigate"),
            |ui, app| {
                big(ui, app, "search.place", Value::Null);
                col(ui, |ui| {
                    small(ui, app, "camera.globalView", Value::Null);
                    small(ui, app, "camera.resetNorth", Value::Null);
                    small(ui, app, "camera.topDown", Value::Null);
                });
            },
        );
        group(
            ui,
            self,
            "explore.places",
            lang.text("ribbon-group-places"),
            |ui, app| {
                big(ui, app, "places.addAtCenter", Value::Null);
                col(ui, |ui| {
                    small(ui, app, "places.addFolder", Value::Null);
                    small(ui, app, "places.open", Value::Null);
                });
            },
        );
        group(
            ui,
            self,
            "explore.measure",
            lang.text("ribbon-group-measure"),
            |ui, app| {
                big(ui, app, "ruler.enable", Value::Null);
                col(ui, |ui| {
                    small(ui, app, "ruler.editPoints", Value::Null);
                    small(ui, app, "ruler.clear", Value::Null);
                    app.ruler_unit_menu(ui);
                });
            },
        );
        group(
            ui,
            self,
            "explore.capture",
            lang.text("ribbon-group-capture"),
            |ui, app| {
                big(ui, app, "file.screenshot", Value::Null);
                col(ui, |ui| {
                    app.screenshot_quality_menu(ui);
                    // Large screenshots take a while: they can be stopped.
                    if app.screenshot_running() {
                        small(ui, app, "file.cancelScreenshot", Value::Null);
                    }
                });
            },
        );
        group(
            ui,
            self,
            "explore.assistant",
            lang.text("ribbon-group-assistant"),
            |ui, app| {
                big(ui, app, "ui.pane.assistant", Value::Null);
            },
        );
    }

    /// What the map shows: imagery, borders and names, nature, transport, reference
    /// lines and the user's own data.
    pub(super) fn map_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(
            ui,
            self,
            "map.imagery",
            lang.text("ribbon-group-imagery"),
            |ui, app| {
                let choices: Vec<Choice> = BASEMAPS
                    .iter()
                    .map(|&(code, icon, key)| Choice {
                        id: "layer.basemap",
                        params: json!({"basemap": code}),
                        icon,
                        label: lang.text(key),
                    })
                    .collect();
                choice_big(ui, app, lang.text("cmd-layer-basemap-help"), &choices);
                if app.settings.layers.basemap == globecraft_data::tiles::Provider::Wayback {
                    col(ui, |ui| app.wayback_stepper(ui));
                }
            },
        );
        group(
            ui,
            self,
            "map.borders",
            lang.text("ribbon-group-borders-names"),
            |ui, app| {
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::CountryBorders);
                    app.layer_small(ui, MapLayer::StateBorders);
                    app.layer_small(ui, MapLayer::MunicipalBorders);
                });
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::PlaceLabels);
                    app.layer_small(ui, MapLayer::SeaNames);
                    app.layer_small(ui, MapLayer::PointsOfInterest);
                });
            },
        );
        group(
            ui,
            self,
            "map.nature",
            lang.text("ribbon-group-nature"),
            |ui, app| {
                app.layer_big(ui, MapLayer::Terrain);
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::Waters);
                    app.layer_small(ui, MapLayer::Parks);
                    app.layer_small(ui, MapLayer::Peaks);
                });
            },
        );
        group(
            ui,
            self,
            "map.transport",
            lang.text("sidebar-transport-title"),
            |ui, app| {
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::Roads);
                    app.layer_small(ui, MapLayer::Railways);
                    app.layer_small(ui, MapLayer::Airports);
                });
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::Ferries);
                });
            },
        );
        group(
            ui,
            self,
            "map.reference",
            lang.text("ribbon-group-reference"),
            |ui, app| {
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::Grid);
                    app.layer_small(ui, MapLayer::GridLabels);
                    app.layer_small(ui, MapLayer::TimeZones);
                });
            },
        );
        group(
            ui,
            self,
            "map.data",
            lang.text("layers-user-title"),
            |ui, app| {
                big(ui, app, "ui.addData", Value::Null);
                col(ui, |ui| {
                    small(ui, app, "ui.pane.layers", Value::Null);
                });
            },
        );
    }

    /// What is happening now, and when.
    pub(super) fn live_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        group(
            ui,
            self,
            "live.weather",
            lang.text("sidebar-weather-title"),
            |ui, app| {
                app.layer_big(ui, MapLayer::Radar);
                app.layer_big(ui, MapLayer::Clouds);
                app.layer_big(ui, MapLayer::Aurora);
            },
        );
        group(
            ui,
            self,
            "live.tracking",
            lang.text("ribbon-group-tracking"),
            |ui, app| {
                app.layer_big(ui, MapLayer::Flights);
                app.layer_big(ui, MapLayer::Vessels);
                app.layer_big(ui, MapLayer::Satellites);
                app.layer_big(ui, MapLayer::Traffic);
            },
        );
        group(
            ui,
            self,
            "live.time",
            lang.text("ribbon-group-time"),
            |ui, app| {
                big(ui, app, "ui.timeline", Value::Null);
                col(ui, |ui| {
                    small_with(
                        ui,
                        app,
                        "time.set",
                        json!({"time": "now"}),
                        Look {
                            label: Some(lang.text("timeline-now-action")),
                            help: Some(lang.text("timeline-now-help")),
                            checked: Some(false),
                            ..Look::default()
                        },
                    );
                    small(ui, app, "time.play", Value::Null);
                    small(ui, app, "time.pause", Value::Null);
                });
            },
        );
    }

    /// How the globe, the sky and the window look.
    pub(super) fn view_tab(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        if self.settings.experience.is_advanced() {
            group(
                ui,
                self,
                "view.worlds",
                lang.text("ribbon-group-worlds"),
                |ui, app| {
                    let choices: Vec<Choice> = WORLDS
                        .iter()
                        .map(|&(body, icon, key)| Choice {
                            id: "world.body",
                            params: json!({"body": body}),
                            icon,
                            label: lang.text(key),
                        })
                        .collect();
                    choice_big(ui, app, lang.text("cmd-world-body-help"), &choices);
                },
            );
        }
        group(
            ui,
            self,
            "view.sky",
            lang.text("ribbon-group-sky"),
            |ui, app| {
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::Atmosphere);
                    app.layer_small(ui, MapLayer::Sunlight);
                    app.layer_small(ui, MapLayer::NightLights);
                });
                col(ui, |ui| {
                    app.layer_small(ui, MapLayer::Stars);
                    app.layer_small(ui, MapLayer::MilkyWay);
                });
            },
        );
        group(
            ui,
            self,
            "view.panes",
            lang.text("ribbon-group-panes"),
            |ui, app| {
                col(ui, |ui| {
                    small(ui, app, "ui.leftPane", Value::Null);
                    small(ui, app, "ui.legend", Value::Null);
                    small(ui, app, "ui.pane.measure", Value::Null);
                });
                col(ui, |ui| {
                    small(ui, app, "ui.pane.assistant", Value::Null);
                });
            },
        );
        group(
            ui,
            self,
            "view.window",
            lang.text("ribbon-group-window"),
            |ui, app| {
                big(ui, app, "ui.navigationControls", Value::Null);
                col(ui, |ui| {
                    small(ui, app, "ui.fullscreen", Value::Null);
                });
            },
        );
    }

    /// The ruler's unit, a small menu in the Measure group.
    fn ruler_unit_menu(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let mut picked = None;
        small_menu(
            ui,
            "ruler_units",
            lang.text("cmd-ruler-unit"),
            lang.text("cmd-ruler-unit-help"),
            |ui| {
                for unit in RulerUnit::ALL {
                    let params = json!({"unit": unit.code()});
                    let on = self.command_state("ruler.unit", &params).checked;
                    if ui
                        .radio(on, unit.selector_label(lang, self.us_locale))
                        .clicked()
                    {
                        picked = Some(params);
                        ui.close();
                    }
                }
            },
        );
        if let Some(params) = picked {
            self.run_reporting("ruler.unit", params);
        }
    }
}

/// The ribbon's short caption of a layer (the Layers pane shows the long one).
fn short_layer_label(lang: globecraft_core::language::Lang, layer: MapLayer) -> &'static str {
    lang.text(match layer {
        MapLayer::CountryBorders => "ribbon-layer-countries",
        MapLayer::StateBorders => "ribbon-layer-states",
        MapLayer::PlaceLabels => "ribbon-layer-labels",
        MapLayer::MunicipalBorders => "ribbon-layer-municipal",
        MapLayer::Terrain => "ribbon-layer-terrain",
        MapLayer::Grid => "ribbon-layer-grid",
        MapLayer::GridLabels => "ribbon-layer-grid-labels",
        MapLayer::TimeZones => "ribbon-layer-timezones",
        MapLayer::Atmosphere => "ribbon-layer-atmosphere",
        MapLayer::Sunlight => "ribbon-layer-sunlight",
        MapLayer::Stars => "ribbon-layer-stars",
        MapLayer::MilkyWay => "ribbon-layer-milky-way",
        MapLayer::CinematicSky => "ribbon-layer-cinematic",
        MapLayer::Radar => "ribbon-layer-radar",
        MapLayer::Clouds => "ribbon-layer-clouds",
        MapLayer::Aurora => "ribbon-layer-aurora",
        MapLayer::Flights => "ribbon-layer-flights",
        MapLayer::Vessels => "ribbon-layer-vessels",
        MapLayer::Satellites => "ribbon-layer-satellites",
        MapLayer::Roads => "ribbon-layer-roads",
        MapLayer::Traffic => "ribbon-layer-traffic",
        MapLayer::NightLights => "ribbon-layer-night-lights",
        MapLayer::Railways => "ribbon-layer-railways",
        MapLayer::Ferries => "ribbon-layer-ferries",
        MapLayer::Airports => "ribbon-layer-airports",
        MapLayer::PointsOfInterest => "ribbon-layer-poi",
        MapLayer::Peaks => "ribbon-layer-peaks",
        MapLayer::Waters => "ribbon-layer-waters",
        MapLayer::SeaNames => "ribbon-layer-sea-names",
        MapLayer::Parks => "ribbon-layer-parks",
    })
}
