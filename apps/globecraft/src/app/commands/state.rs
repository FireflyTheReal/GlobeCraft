//! Whether each command can run now and whether it is on, for buttons and menus.
use super::{CommandState, str_param};
use crate::app::App;
use globecraft_core::{body::Body, layers::MapLayer};
use globecraft_data::settings::{
    UnitPreference, appearance::ThemePreference, sidebar::SidebarPanel, workspace::RightPane,
};
use serde_json::Value;

impl App {
    /// Whether a command is available and checked, for buttons and menus.
    pub(in crate::app) fn command_state(
        &self,
        id: &str,
        params: &Value,
    ) -> crate::app::commands::CommandState {
        let on = |checked| CommandState {
            enabled: true,
            checked,
        };
        let earth = self.explorer.active == Body::Earth;
        match id {
            "camera.autoRotate" => on(self.motion.auto_rotate),
            "ruler.enable" => on(self.measure.on),
            "ruler.closeShape" => on(self.measure.paths.closed()),
            "edit.undo" => CommandState {
                enabled: self.measure.paths.can_undo(),
                checked: false,
            },
            "edit.redo" => CommandState {
                enabled: self.measure.paths.can_redo(),
                checked: false,
            },
            "file.screenshot" => CommandState {
                enabled: self.frame.image.is_some() && !self.screenshot_running(),
                checked: false,
            },
            "satellite.clear" => CommandState {
                enabled: self.picked.satellite.selected().is_some(),
                checked: false,
            },
            "satellite.select" => CommandState {
                enabled: earth && self.layer(MapLayer::Satellites),
                checked: false,
            },
            "flight.clear" => CommandState {
                enabled: self.picked.flight.selected().is_some(),
                checked: false,
            },
            "flight.select" => CommandState {
                enabled: earth,
                checked: false,
            },
            "file.cancelScreenshot" => CommandState {
                enabled: self.screenshot_running(),
                checked: false,
            },
            "assistant.stop" => CommandState {
                #[cfg(feature = "assistant")]
                enabled: self.assistant.busy(),
                #[cfg(not(feature = "assistant"))]
                enabled: false,
                checked: false,
            },
            "layer.basemap" => CommandState {
                enabled: earth,
                checked: str_param(params, "basemap")
                    .is_some_and(|b| b == self.settings.layers.basemap.code()),
            },
            "layer.show" => {
                let layer = str_param(params, "layer").and_then(MapLayer::from_code);
                CommandState {
                    enabled: earth
                        && layer
                            .and_then(MapLayer::parent)
                            .is_none_or(|parent| self.layer(parent)),
                    checked: layer.is_some_and(|l| self.layer(l)),
                }
            }
            "layer.solo" => CommandState {
                enabled: earth,
                checked: self.solo.is_some(),
            },
            "layer.waybackRelease" => CommandState {
                enabled: !self.wayback.releases.is_empty(),
                checked: false,
            },
            "layer.waybackCompare" => CommandState {
                enabled: self.wayback.releases.len() > 1,
                checked: self.active_compare().is_some(),
            },
            "layer.terrainExaggeration" => CommandState {
                enabled: self.layer(MapLayer::Terrain),
                checked: false,
            },
            "layer.flatLiveObjects" => CommandState {
                enabled: self.layer(MapLayer::Flights) || self.layer(MapLayer::Satellites),
                checked: self.settings.layers.flat_live_objects,
            },
            "layer.stateAltitude" => CommandState {
                enabled: self.layer(MapLayer::StateBorders),
                checked: false,
            },
            "world.earthModel" => {
                on(str_param(params, "model")
                    .is_some_and(|m| m == self.settings.earth_model.code()))
            }
            "world.body" => on(match str_param(params, "body") {
                Some("earth") => self.explorer.active == Body::Earth,
                Some("moon") => self.explorer.active == Body::Moon,
                Some("custom") => self.explorer.active == Body::Custom,
                _ => false,
            }),
            "ui.explorer" => on(self.settings.workspace.right_pane == Some(RightPane::Explorer)),
            "ui.pane.layers" => {
                on(self.settings.sidebar.open
                    && self.settings.sidebar.panel == SidebarPanel::Layers)
            }
            "ui.pane.places" => {
                on(self.settings.sidebar.open
                    && self.settings.sidebar.panel == SidebarPanel::Places)
            }
            "ui.pane.assistant" => CommandState {
                enabled: cfg!(feature = "assistant"),
                checked: self.settings.workspace.right_pane == Some(RightPane::Assistant),
            },
            "ui.pane.measure" => on(self.settings.workspace.right_pane == Some(RightPane::Measure)),
            "ui.leftPane" => on(self.settings.sidebar.open),
            "ui.navigationControls" => on(!self.settings.hide_navigation_controls),
            "app.workOffline" => on(self.settings.offline),
            "ui.tileDiagnostics" => on(self.frame.diagnostics),
            "ui.timeline" => on(self.timeline.open),
            "ui.legend" => on(self.legend_open()),
            "time.play" => on(self.time.speed() != 0.0),
            "time.pause" => CommandState {
                enabled: self.time.speed() != 0.0,
                checked: false,
            },
            "ui.navigatorStyle" => on(str_param(params, "style")
                .is_some_and(|style| style == self.settings.navigation.navigator.code())),
            "ui.ribbon" => on(self.settings.workspace.ribbon_minimized),
            "ui.ribbon.pin" => on(str_param(params, "group")
                .zip(str_param(params, "tool"))
                .is_some_and(|(g, t)| self.tool_pinned(g, t))),
            "ui.quickTools.pin" => on(str_param(params, "tool")
                .is_some_and(|t| self.settings.workspace.quick_tools.iter().any(|k| k == t))),
            "ui.theme" => on(match str_param(params, "theme") {
                Some("system") => self.settings.appearance.theme == ThemePreference::System,
                Some("dark") => self.settings.appearance.theme == ThemePreference::Dark,
                Some("light") => self.settings.appearance.theme == ThemePreference::Light,
                _ => false,
            }),
            "ui.fullscreen" => on(self.ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))),
            "ui.settings" => on(self.windows.settings),
            "settings.units" => on(match str_param(params, "units") {
                Some("system") => self.settings.distance_units == UnitPreference::System,
                Some("metric") => self.settings.distance_units == UnitPreference::Metric,
                Some("imperial") => self.settings.distance_units == UnitPreference::Imperial,
                _ => false,
            }),
            "settings.experience" => on(str_param(params, "level")
                .is_some_and(|level| level == self.settings.experience.code())),
            "settings.coordinateFormat" => on(str_param(params, "format")
                .is_some_and(|f| f == self.settings.coordinate_display.format.code())),
            "settings.language" => {
                on(str_param(params, "language")
                    .is_some_and(|l| l == self.locale.preference.code()))
            }
            "ruler.unit" => {
                on(str_param(params, "unit").is_some_and(|u| u == self.settings.ruler_unit.code()))
            }
            "style.reset" => CommandState {
                enabled: self.settings.map_style != Default::default(),
                checked: false,
            },
            "camera.zoomIn" | "camera.zoomOut" | "camera.resetNorth" | "camera.globalView"
            | "search.place" | "camera.flyTo" | "camera.set" => CommandState {
                enabled: !self.flat_earth.open,
                checked: false,
            },
            _ => on(false),
        }
    }
}
