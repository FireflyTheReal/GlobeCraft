//! Runs commands by id. The ribbon, menus, keyboard shortcuts, the command palette and the AI
//! assistant all come through [`App::run`], so each action has one implementation.
use super::App;
use eframe::egui;
use globecraft_core::{
    body::Body,
    coordinates::CoordinateFormat,
    earth_model::EarthModel,
    language::{Lang, LanguagePreference},
    layers::MapLayer,
    placemark::{PlaceItem, PlaceNode, Placemark},
    units::RulerUnit,
};
use globecraft_data::settings::experience::ExperienceLevel;
use globecraft_data::settings::screenshot::ScreenshotQuality;
use globecraft_data::settings::{
    UnitPreference, appearance::ThemePreference, sidebar::SidebarPanel, workspace::RightPane,
};
use serde_json::{Value, json};

mod camera;
mod interface;
mod layers;
mod places;
mod ruler;

/// Camera altitude of the Global View.
const GLOBAL_VIEW_ALTITUDE_KM: f64 = 14_000.0;
/// Zoom In/Out buttons move the camera by this factor.
const ZOOM_STEP_FACTOR: f64 = 0.5;
/// Altitude a placemark is shown from, as in the Places tree.
const PLACE_FLY_ALTITUDE_KM: f64 = 3.0;
const DOCUMENTATION_URL: &str = "https://github.com/FireflyTheReal/GlobeCraft#readme";
const ISSUES_URL: &str = "https://github.com/FireflyTheReal/GlobeCraft/issues/new/choose";

/// Whether a command can run now, and whether its toggle or choice is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct CommandState {
    pub enabled: bool,
    pub checked: bool,
}

/// The keyboard shortcut of a command, as text for tooltips and menus.
pub(in crate::app) fn shortcut_text(id: &str) -> String {
    tool_shortcut_text(id)
}

/// The keyboard shortcut of a tool (see [`super::tools::tool_key`]), as text.
pub(in crate::app) fn tool_shortcut_text(key: &str) -> String {
    tool_shortcut(key)
        .map(|s| keys_label(&s))
        .unwrap_or_default()
}

/// Keys as the interface shows them (⌘⇧E on macOS, Ctrl+Shift+E elsewhere).
pub(in crate::app) fn keys_label(shortcut: &egui::KeyboardShortcut) -> String {
    shortcut.format(&egui::ModifierNames::NAMES, cfg!(target_os = "macos"))
}

/// The keyboard shortcut of a command: the user's, or the default.
pub(in crate::app) fn shortcut(id: &str) -> Option<egui::KeyboardShortcut> {
    tool_shortcut(id)
}

/// The keyboard shortcut of a tool: the user's, or the default.
pub(in crate::app) fn tool_shortcut(key: &str) -> Option<egui::KeyboardShortcut> {
    super::tools::user_shortcut(key).unwrap_or_else(|| default_shortcut(key))
}

/// The keyboard shortcut a command has at installation.
pub(in crate::app) fn default_shortcut(id: &str) -> Option<egui::KeyboardShortcut> {
    use egui::{Key, Modifiers};
    let (modifiers, key) = match id {
        "places.open" => (Modifiers::COMMAND, Key::O),
        "places.save" => (Modifiers::COMMAND, Key::S),
        "places.saveAs" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::S),
        "file.screenshot" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::E),
        "app.quit" => (Modifiers::COMMAND, Key::Q),
        "edit.undo" => (Modifiers::COMMAND, Key::Z),
        "edit.redo" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
        "ui.settings" => (Modifiers::COMMAND, Key::Comma),
        "places.addAtCenter" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::P),
        "places.addFolder" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::N),
        "ui.leftPane" => (Modifiers::COMMAND | Modifiers::ALT, Key::B),
        "ui.commandSearch" => (Modifiers::COMMAND, Key::K),
        "ui.pane.assistant" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::A),
        "ui.ribbon" => (Modifiers::CTRL, Key::F1),
        "search.place" => (Modifiers::COMMAND, Key::F),
        "ui.focusNext" => (Modifiers::NONE, Key::F6),
        "ui.focusPrevious" => (Modifiers::SHIFT, Key::F6),
        _ => return None,
    };
    Some(egui::KeyboardShortcut::new(modifiers, key))
}

/// Commands with keyboard shortcuts that the window handles (macOS routes some through its
/// native menu instead).
pub(in crate::app) const SHORTCUT_COMMANDS: &[&str] = &[
    "places.open",
    "places.save",
    "places.saveAs",
    "file.screenshot",
    "app.quit",
    "ui.settings",
    "places.addAtCenter",
    "places.addFolder",
    "ui.leftPane",
    "ui.commandSearch",
    "ui.pane.assistant",
    "ui.ribbon",
    "search.place",
    "ui.focusNext",
    "ui.focusPrevious",
];

fn f64_param(params: &Value, key: &str) -> Result<Option<f64>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{key} must be a number")),
    }
}

fn required_f64(params: &Value, key: &str) -> Result<f64, String> {
    f64_param(params, key)?.ok_or_else(|| format!("{key} is required"))
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str).map(str::trim)
}

fn bool_param(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

/// A latitude/longitude pair, validated like typed coordinates.
fn position(params: &Value) -> Result<(f64, f64), String> {
    let lat = required_f64(params, "lat")?;
    let lon = required_f64(params, "lon")?;
    globecraft_core::coordinates::validate_coordinates(lat, lon).map_err(|e| e.to_string())
}

/// `on` when given, else the opposite of `current`.
fn toggled(params: &Value, current: bool) -> bool {
    bool_param(params, "on").unwrap_or(!current)
}

fn place_json(depth: usize, node: &PlaceNode) -> Value {
    match &node.item {
        PlaceItem::Folder {
            name, description, ..
        } => json!({"type": "folder", "name": name, "description": description,
            "visible": node.visible, "depth": depth}),
        PlaceItem::Marker(pm) => json!({"type": "placemark", "name": pm.name,
            "description": pm.desc, "lat": pm.lat, "lon": pm.lon,
            "visible": node.visible, "depth": depth}),
    }
}

impl App {
    /// Runs a command; problems are reported in the status bar.
    pub(in crate::app) fn run_reporting(&mut self, id: &str, params: Value) {
        if let Err(error) = self.run(id, params) {
            self.status = error;
        }
    }

    /// Runs command `id` with JSON `params`, returning what it did as JSON.
    pub(in crate::app) fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let p = &params;
        let ctx = self.ctx.clone();
        let result = match id {
            id if id.starts_with("data.") || id == "ui.addData" => self.run_user_data(id, p)?,
            "info.view" => self.view_json(),
            "info.places" => {
                let mut places = Vec::new();
                self.places
                    .items
                    .walk(|depth, node| places.push(place_json(depth, node)));
                json!({"count": self.places.items.len(), "items": places})
            }
            "info.ruler" => self.ruler_json(),
            "info.here" => {
                let (lat, lon) = position(p)?;
                self.here_json(lat, lon)
            }
            id if id.starts_with("time.") => self.run_time(id, p)?,
            id if id.starts_with("flight.") => self.run_flight(id, p)?,
            id if id.starts_with("satellite.") => self.run_satellite(id, p)?,
            id if id.starts_with("camera.") || id.starts_with("search.") => {
                self.run_camera(id, p)?
            }
            id if id.starts_with("places.") => self.run_places(id, p)?,
            id if id.starts_with("ruler.") || id.starts_with("edit.") => self.run_ruler(id, p)?,
            id if id.starts_with("layer.")
                || id.starts_with("view.")
                || id.starts_with("world.")
                || id.starts_with("style.") =>
            {
                self.run_layers(id, p)?
            }
            id if id.starts_with("ui.")
                || id.starts_with("assistant.")
                || id.starts_with("settings.")
                || id.starts_with("file.")
                || id.starts_with("help.")
                || id.starts_with("app.") =>
            {
                self.run_interface(id, p)?
            }
            _ => return Err(format!("unknown command '{id}'")),
        };
        ctx.request_repaint();
        Ok(result)
    }

    /// Whether a command is available and checked, for buttons and menus.
    pub(in crate::app) fn command_state(
        &self,
        id: &str,
        params: &Value,
    ) -> super::commands::CommandState {
        let on = |checked| CommandState {
            enabled: true,
            checked,
        };
        let earth = self.explorer.on_earth();
        match id {
            "camera.autoRotate" => on(self.auto_rotate),
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
                enabled: self.imagery.image.is_some() && !self.screenshot_running(),
                checked: false,
            },
            "satellite.clear" => CommandState {
                enabled: self.selection.satellite.selected().is_some(),
                checked: false,
            },
            "satellite.select" => CommandState {
                enabled: earth && self.layer(MapLayer::Satellites),
                checked: false,
            },
            "flight.clear" => CommandState {
                enabled: self.selection.flight.selected().is_some(),
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
                Some("earth") => self.explorer.on_earth(),
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
            "ui.tileDiagnostics" => on(self.tile_diagnostics),
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
                    .is_some_and(|l| l == self.language_preference.code()))
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

    fn camera_json(&self) -> Value {
        json!({"lat": self.cam.lat, "lon": self.cam.lon, "altitude_km": self.cam.alt,
            "heading_deg": self.cam.heading, "tilt_deg": self.cam.tilt,
            "max_tilt_deg": self.cam.max_tilt()})
    }

    pub(in crate::app) fn ruler_json(&self) -> Value {
        let model = self.active_earth_model();
        let total_km = self.measure.paths.distance_km(model);
        let paths: Vec<Vec<[f64; 2]>> = self
            .measure
            .paths
            .paths()
            .map(|path| path.iter().map(|(lat, lon)| [*lat, *lon]).collect())
            .collect();
        let bearing = self
            .measure
            .paths
            .last_leg()
            .and_then(|pair| model.initial_bearing(pair[0], pair[1]));
        json!({
            "enabled": self.measure.on,
            "closed_shapes": self.measure.paths.closed(),
            "area_km2": self.measure.paths.area_km2(model),
            "points": self.measure.paths.point_count(),
            "paths_lat_lon": paths,
            "total_km": total_km,
            "total_display": self.settings.ruler_unit.format_with_angle(
                total_km,
                self.measure.paths.angular_distance_degrees(model),
                self.distance_units(),
                Lang::En,
            ),
            "last_bearing_deg": bearing,
        })
    }

    /// What is on screen, for `info.view` and the assistant's context.
    pub(in crate::app) fn view_json(&self) -> Value {
        let body = match self.explorer.active {
            Body::Earth => "earth",
            Body::Moon => "moon",
            Body::Custom => "custom",
        };
        let layers: Vec<&str> = self
            .settings
            .visible_layers()
            .iter()
            .map(MapLayer::code)
            .collect();
        let results: Vec<Value> = self
            .search
            .results
            .iter()
            .take(5)
            .map(|r| json!({"name": r.name, "lat": r.lat, "lon": r.lon}))
            .collect();
        let center = self
            .settings
            .coordinate_display
            .format(self.cam.lat, self.cam.lon)
            .ok();
        json!({
            "camera": self.camera_json(),
            "camera_display": center,
            "world": body,
            "earth_model": self.settings.earth_model.code(),
            "basemap": self.settings.layers.basemap.code(),
            "wayback_release": (self.settings.layers.basemap == globecraft_data::tiles::Provider::Wayback)
                .then(|| self.wayback.releases.get(self.wayback.index).map(|r| r.date.clone())).flatten(),
            "visible_layers": layers,
            "user_layers": self.user_layers_json(),
            "units": match self.distance_units() {
                globecraft_core::units::DistanceUnits::Metric => "metric",
                globecraft_core::units::DistanceUnits::Imperial => "imperial",
            },
            "places": self.places.items.len(),
            "ruler_points": self.measure.paths.point_count(),
            "search_query": (!self.search.query.trim().is_empty()).then(|| self.search.query.trim().to_string()),
            "search_results": results,
            "status": self.status,
        })
    }

    /// Gives `tool` the keys `keys` (`None` restores the default, empty removes it). A tool
    /// that had these keys loses them, so each keys run one tool.
    fn set_shortcut(&mut self, tool: &str, keys: Option<&str>) -> Result<Value, String> {
        let wanted = match keys {
            None => default_shortcut(tool),
            Some("") => None,
            Some(text) => {
                let keys = super::tools::parse_keys(text).ok_or("unknown keys")?;
                if !super::tools::allowed(&keys) {
                    return Err("use Ctrl, Alt or Command with the key, or a function key".into());
                }
                Some(keys)
            }
        };
        let mut previous = None;
        if let Some(keys) = wanted {
            let mut holders: Vec<String> =
                SHORTCUT_COMMANDS.iter().map(|id| id.to_string()).collect();
            holders.extend(self.settings.workspace.shortcuts.keys().cloned());
            holders.sort();
            holders.dedup();
            for other in holders {
                if other != tool && tool_shortcut(&other) == Some(keys) {
                    self.settings
                        .workspace
                        .shortcuts
                        .insert(other.clone(), String::new());
                    previous = Some(other);
                }
            }
        }
        let shortcuts = &mut self.settings.workspace.shortcuts;
        if wanted == default_shortcut(tool) {
            shortcuts.remove(tool);
        } else {
            shortcuts.insert(
                tool.to_string(),
                wanted
                    .map(|k| super::tools::keys_text(&k))
                    .unwrap_or_default(),
            );
        }
        super::tools::sync_shortcuts(shortcuts);
        if let (Some(keys), Some(previous)) = (wanted, &previous) {
            self.status = self.lang.format(
                "shortcut-taken",
                &[
                    ("keys", keys_label(&keys).into()),
                    (
                        "tool",
                        super::ui::ribbon_tools::tool_name(self, tool).into(),
                    ),
                    (
                        "previous",
                        super::ui::ribbon_tools::tool_name(self, previous).into(),
                    ),
                ],
            );
        }
        Ok(json!({
            "keys": wanted.map(|k| super::tools::keys_text(&k)),
            "removed_from": previous,
        }))
    }

    /// Commands bound to keyboard shortcuts, run when their keys are pressed.
    pub(in crate::app) fn command_shortcuts(&mut self, ctx: &egui::Context) {
        super::tools::sync_shortcuts(&self.settings.workspace.shortcuts);
        let mut bound: Vec<(String, egui::KeyboardShortcut)> = SHORTCUT_COMMANDS
            .iter()
            .filter_map(|id| shortcut(id).map(|s| (id.to_string(), s)))
            .collect();
        for (key, keys) in super::tools::user_shortcuts() {
            if !SHORTCUT_COMMANDS.contains(&key.as_str()) {
                bound.push((key, keys));
            }
        }
        // Keys with more modifiers first: ⌘⇧S must not run ⌘S's command.
        bound.sort_by_key(|(key, keys)| {
            let m = keys.modifiers;
            (
                std::cmp::Reverse(
                    u8::from(m.command) + u8::from(m.ctrl) + u8::from(m.alt) + u8::from(m.shift),
                ),
                key.clone(),
            )
        });
        for (key, keys) in bound {
            #[cfg(target_os = "macos")]
            if self.native_menu.is_some() && super::menu::native_shortcut(&key) {
                continue;
            }
            if ctx.input_mut(|input| input.consume_shortcut(&keys))
                && let Some((id, params)) = super::tools::parse_tool(&key)
            {
                self.run_reporting(&id, params);
            }
        }
    }
}

/// The assistant's view of the application: commands run through [`App::run`].
#[cfg(feature = "assistant")]
pub(in crate::app) struct AssistantHost<'a>(pub &'a mut App);

#[cfg(feature = "assistant")]
impl globecraft_assistant::Host for AssistantHost<'_> {
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let spec =
            globecraft_core::commands::find(id).ok_or_else(|| format!("unknown command '{id}'"))?;
        if !spec.assistant_may_run() {
            return Err(format!("'{id}' is reserved to the user"));
        }
        self.0.run(id, params.clone())
    }
    fn context(&self) -> String {
        context_text(self.0)
    }
}

/// A compact description of the view, sent with the user's message when it changed.
#[cfg(feature = "assistant")]
pub(in crate::app) fn context_text(app: &App) -> String {
    let v = app.view_json();
    let mut lines = vec![format!(
        "Camera: lat {:.4}, lon {:.4}, altitude {:.1} km, heading {:.0}°",
        app.cam.lat, app.cam.lon, app.cam.alt, app.cam.heading
    )];
    lines.push(format!(
        "World: {} · Basemap: {} · Units: {}",
        v["world"].as_str().unwrap_or_default(),
        v["basemap"].as_str().unwrap_or_default(),
        v["units"].as_str().unwrap_or_default()
    ));
    let layers: Vec<&str> = v["visible_layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    lines.push(format!("Visible layers: {}", layers.join(", ")));
    lines.push(format!(
        "Places: {} · Ruler points: {}",
        app.places.items.len(),
        app.measure.paths.point_count()
    ));
    if let Some(results) = v["search_results"].as_array().filter(|r| !r.is_empty()) {
        let names: Vec<String> = results
            .iter()
            .map(|r| {
                r["name"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(60)
                    .collect()
            })
            .collect();
        lines.push(format!("Search results: {}", names.join(" | ")));
    }
    lines.join("\n")
}

impl App {
    pub(in crate::app) fn reveal_places(&mut self) {
        self.settings.sidebar.open = true;
        self.places.reveal_requested = true;
        self.measure.reveal_requested = false;
    }
    pub(in crate::app) fn reveal_ruler(&mut self) {
        self.measure.reveal_requested = self.measure.on;
        if self.measure.on {
            self.places.reveal_requested = false;
        }
    }
    /// A check box that shows or hides `layer` through its command.
    pub(in crate::app) fn layer_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        layer: MapLayer,
        label: &str,
    ) {
        let mut on = self.layer(layer);
        if ui.checkbox(&mut on, label).changed() {
            self.run_reporting("layer.show", json!({"layer": layer.code(), "visible": on}));
        }
    }
}

#[cfg(test)]
mod dispatch_tests {
    /// The body of `fn <name>(` in `source`: up to the first line that closes it at the
    /// function's own indentation.
    fn body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("fn {name} not found"));
        let indent = source[..start]
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .take_while(|c| *c == ' ')
            .count();
        let close = format!("\n{}}}\n", " ".repeat(indent));
        let end = source[start..]
            .find(&close)
            .map_or(source.len(), |end| start + end);
        &source[start..end]
    }

    /// Every command in the catalog is a match arm of `App::run` or of the handler it
    /// hands a prefix to, so a command added to the catalog without a handler (which
    /// would only fail at run time, as "unknown command") fails here.
    /// Functions whose first text argument (after at most two plain arguments such as
    /// `ui, self`) is a command id.
    const COMMAND_CALLS: [&str; 12] = [
        "run",
        "run_reporting",
        "small",
        "big",
        "small_with",
        "big_with",
        "menu_item",
        "menu_item_with",
        "user_shortcut",
        "shortcut",
        "tool_key",
        "command_state",
    ];

    /// The command ids passed to [`COMMAND_CALLS`] in `source`.
    fn command_ids_called(source: &str) -> Vec<&str> {
        let mut found = Vec::new();
        for call in COMMAND_CALLS {
            let pattern = format!("{call}(");
            for (start, _) in source.match_indices(&pattern) {
                let before = source[..start].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &source[start + pattern.len()..];
                let Some(quote) = rest.find('"') else {
                    continue;
                };
                let leading = &rest[..quote];
                if leading.matches(',').count() > 2
                    || !leading
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || "_&, \n".contains(c))
                {
                    continue;
                }
                let literal = &rest[quote + 1..];
                if let Some(end) = literal.find('"') {
                    found.push(&literal[..end]);
                }
            }
        }
        found
    }

    /// A command id mistyped where the interface runs or shows a command compiles and
    /// fails only when clicked; every id passed to the command functions must be in the
    /// catalog.
    #[test]
    fn every_command_id_in_the_interface_is_in_the_catalog() {
        fn check(path: &std::path::Path, checked: &mut usize) {
            if path.is_dir() {
                for entry in std::fs::read_dir(path).unwrap() {
                    check(&entry.unwrap().path(), checked);
                }
                return;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                return;
            }
            let source = std::fs::read_to_string(path).unwrap();
            for id in command_ids_called(&source) {
                let prefix = id.split('.').next().unwrap_or_default();
                let is_command_prefix = globecraft_core::commands::COMMANDS
                    .iter()
                    .any(|spec| spec.id.split('.').next() == Some(prefix));
                if !id.contains('.') || !is_command_prefix {
                    continue;
                }
                *checked += 1;
                assert!(
                    globecraft_core::commands::find(id).is_some(),
                    "{} names command {id:?}, which is not in the catalog",
                    path.display()
                );
            }
        }
        let mut checked = 0;
        check(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut checked,
        );
        assert!(
            checked > 200,
            "only {checked} command ids found; the scan is broken"
        );
    }

    #[test]
    fn every_catalog_command_has_a_handler() {
        let commands = include_str!("commands.rs");
        let bodies = [
            body(commands, "run"),
            body(include_str!("user_layers.rs"), "run_user_data"),
            body(include_str!("time.rs"), "run_time"),
            body(include_str!("selected_flight.rs"), "run_flight"),
            body(include_str!("selected_satellite.rs"), "run_satellite"),
            body(include_str!("legend.rs"), "run_legend"),
            body(include_str!("commands/camera.rs"), "run_camera"),
            body(include_str!("commands/places.rs"), "run_places"),
            body(include_str!("commands/ruler.rs"), "run_ruler"),
            body(include_str!("commands/layers.rs"), "run_layers"),
            body(include_str!("commands/interface.rs"), "run_interface"),
        ];
        let missing: Vec<&str> = globecraft_core::commands::COMMANDS
            .iter()
            .map(|spec| spec.id)
            .filter(|id| {
                let quoted = format!("\"{id}\"");
                !bodies.iter().any(|body| body.contains(&quoted))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "commands without a handler: {missing:?}"
        );
    }
}
