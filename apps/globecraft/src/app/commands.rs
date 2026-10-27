//! Runs commands by id. The ribbon, menus, keyboard shortcuts, the command palette and the AI
//! assistant all come through [`App::run`], so each action has one implementation.
use super::App;
use eframe::egui;
use globecraft_core::{
    body::Body,
    commands,
    coordinates::CoordinateFormat,
    earth_model::EarthModel,
    language::{Lang, LanguagePreference},
    layers::MapLayer,
    placemark::{PlaceItem, PlaceNode, Placemark},
    units::RulerUnit,
};
use globecraft_data::settings::screenshot::ScreenshotQuality;
use globecraft_data::settings::{
    UnitPreference, appearance::ThemePreference, sidebar::SidebarPanel, workspace::RightPane,
};
use serde_json::{Value, json};

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
    shortcut(id)
        .map(|s| s.format(&egui::ModifierNames::NAMES, cfg!(target_os = "macos")))
        .unwrap_or_default()
}

/// The keyboard shortcut of a command.
pub(in crate::app) fn shortcut(id: &str) -> Option<egui::KeyboardShortcut> {
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
        let earth = self.explorer.active == Body::Earth;
        let result = match id {
            // ── Camera
            "camera.flyTo" => {
                let (lat, lon) = position(p)?;
                let alt = f64_param(p, "altitude_km")?.unwrap_or(self.cam.alt).clamp(
                    globecraft_core::MIN_CAMERA_ALTITUDE_KM,
                    globecraft_core::MAX_CAMERA_ALTITUDE_KM,
                );
                self.fly_to(lat, lon, alt);
                if let Some(heading) = f64_param(p, "heading_deg")? {
                    self.cam.heading = globecraft_core::math::wrap_lon(heading);
                }
                if let Some(tilt) = f64_param(p, "tilt_deg")? {
                    self.cam.set_tilt(tilt);
                }
                json!({"flying_to": {"lat": lat, "lon": lon, "altitude_km": alt}})
            }
            "camera.set" => {
                self.fly = None;
                let lat = f64_param(p, "lat")?.unwrap_or(self.cam.lat);
                let lon = f64_param(p, "lon")?.unwrap_or(self.cam.lon);
                globecraft_core::coordinates::validate_coordinates(lat, lon)
                    .map_err(|e| e.to_string())?;
                self.cam.lat = lat.clamp(
                    -globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                    globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                );
                self.cam.lon = lon;
                if let Some(alt) = f64_param(p, "altitude_km")? {
                    self.cam.alt = alt.clamp(
                        globecraft_core::MIN_CAMERA_ALTITUDE_KM,
                        globecraft_core::MAX_CAMERA_ALTITUDE_KM,
                    );
                }
                if let Some(heading) = f64_param(p, "heading_deg")? {
                    self.cam.heading = globecraft_core::math::wrap_lon(heading);
                }
                self.cam.clamp_tilt();
                if let Some(tilt) = f64_param(p, "tilt_deg")? {
                    self.cam.set_tilt(tilt);
                }
                self.touch();
                self.camera_json()
            }
            "search.place" => match str_param(p, "query") {
                Some(query) if !query.is_empty() => {
                    self.query = query.to_string();
                    self.sugg_open = false;
                    self.search_fly = bool_param(p, "fly").unwrap_or(true);
                    self.start_search(&ctx);
                    json!({"status": self.status, "note": "Results arrive in a moment; describe_view lists them."})
                }
                // Without a query (the ribbon button), the search box takes the focus.
                _ => {
                    self.focus_search = true;
                    json!({})
                }
            },
            "camera.zoomIn" => {
                self.fly = None;
                self.zoom(ZOOM_STEP_FACTOR);
                self.touch();
                self.camera_json()
            }
            "camera.zoomOut" => {
                self.fly = None;
                self.zoom(1.0 / ZOOM_STEP_FACTOR);
                self.touch();
                self.camera_json()
            }
            "camera.resetNorth" => {
                self.cam.heading = 0.0;
                self.touch();
                self.camera_json()
            }
            "camera.tilt" => {
                self.fly = None;
                let tilt = match (f64_param(p, "tilt_deg")?, f64_param(p, "step")?) {
                    (Some(tilt), _) => tilt,
                    (None, Some(step)) => self.cam.tilt + step,
                    (None, None) => return Err("give tilt_deg or step".into()),
                };
                self.cam.set_tilt(tilt);
                self.touch();
                self.camera_json()
            }
            "camera.topDown" => {
                self.cam.tilt = 0.0;
                self.touch();
                self.camera_json()
            }
            "camera.globalView" => {
                self.fly_to(self.cam.lat, self.cam.lon, GLOBAL_VIEW_ALTITUDE_KM);
                self.camera_json()
            }
            "camera.autoRotate" => {
                self.auto_rotate = toggled(p, self.auto_rotate);
                json!({"auto_rotate": self.auto_rotate})
            }
            // ── Places
            "places.addAtCenter" => {
                if let Some(name) = str_param(p, "name") {
                    self.new_name = name.to_string();
                }
                self.add_center_placemark();
                if let Some(desc) = str_param(p, "description")
                    && let Some(node) = self
                        .place_selection
                        .and_then(|id| self.placemarks.find_mut(id))
                {
                    *node.description_mut() = desc.to_string();
                }
                self.reveal_places();
                json!({"added": {"lat": self.cam.lat, "lon": self.cam.lon}})
            }
            "places.add" => {
                let (lat, lon) = position(p)?;
                let parent = match str_param(p, "folder").filter(|f| !f.is_empty()) {
                    Some(folder) => Some(
                        self.placemarks
                            .find_by_name(folder)
                            .filter(|n| matches!(n.item, PlaceItem::Folder { .. }))
                            .map(PlaceNode::id)
                            .ok_or_else(|| format!("no folder named '{folder}'"))?,
                    ),
                    None => None,
                };
                let name = str_param(p, "name")
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        self.lang.format(
                            "placemarks-generated-name",
                            &[("number", (self.placemarks.len() + 1).to_string().into())],
                        )
                    });
                let node = PlaceNode::marker(Placemark {
                    name: name.clone(),
                    desc: str_param(p, "description").unwrap_or_default().to_string(),
                    lat,
                    lon,
                });
                self.place_selection = Some(node.id());
                self.placemarks.insert(parent, node);
                json!({"added": name})
            }
            "places.addFolder" => {
                if let Some(name) = str_param(p, "name") {
                    self.new_name = name.to_string();
                }
                self.add_places_folder();
                self.reveal_places();
                json!({})
            }
            "places.flyTo" => {
                let name = str_param(p, "name").ok_or("name is required")?;
                let node = self
                    .placemarks
                    .find_by_name(name)
                    .ok_or_else(|| format!("no place named '{name}'"))?;
                let PlaceItem::Marker(pm) = &node.item else {
                    return Err(format!("'{name}' is a folder"));
                };
                let (lat, lon) = (pm.lat, pm.lon);
                self.fly_to(lat, lon, PLACE_FLY_ALTITUDE_KM);
                json!({"flying_to": {"lat": lat, "lon": lon}})
            }
            "places.remove" => {
                let name = str_param(p, "name").ok_or("name is required")?;
                let id = self
                    .placemarks
                    .find_by_name(name)
                    .map(PlaceNode::id)
                    .ok_or_else(|| format!("no place named '{name}'"))?;
                self.placemarks.remove(id);
                if self.place_selection == Some(id) {
                    self.place_selection = None;
                }
                json!({"removed": name})
            }
            "places.open" => {
                self.pick_kml_open();
                json!({})
            }
            "places.save" => {
                let path = self.resolve_kml_path(&self.kml_path.clone());
                self.save_kml_to(path.clone());
                json!({"saving": path})
            }
            "places.saveAs" => {
                self.pick_kml_save_as();
                json!({})
            }
            "places.clear" => {
                self.clear_places();
                json!({})
            }
            // ── Ruler
            "ruler.enable" => {
                self.ruler = toggled(p, self.ruler);
                self.reveal_ruler();
                json!({"ruler": self.ruler})
            }
            "ruler.addPoint" => {
                let point = position(p)?;
                self.ruler = true;
                self.ruler_paths
                    .add_point(point, bool_param(p, "new_path").unwrap_or(false));
                self.ruler_json()
            }
            "ruler.clear" => {
                self.ruler_paths.clear();
                self.ruler_drag = None;
                self.ruler_json()
            }
            "ruler.unit" => {
                let unit = str_param(p, "unit")
                    .and_then(RulerUnit::from_code)
                    .ok_or("unknown unit")?;
                self.settings.ruler_unit = unit;
                self.ruler_json()
            }
            "ruler.editPoints" => {
                self.ruler_paths.end_move();
                self.ruler_drag = None;
                self.ruler_point_editor = Some(super::ui::ruler::PointEditor::new((
                    self.cam.lat,
                    self.cam.lon,
                )));
                json!({})
            }
            "edit.undo" | "edit.redo" => {
                let command = if id == "edit.undo" {
                    super::ruler_history::HistoryCommand::Undo
                } else {
                    super::ruler_history::HistoryCommand::Redo
                };
                self.apply_ruler_history(command, &ctx);
                self.ruler_json()
            }
            "file.screenshot" => {
                if self.image.is_none() {
                    return Err("The globe has not finished drawing yet.".into());
                }
                let quality = match str_param(p, "quality") {
                    Some(code) => ScreenshotQuality::from_code(code)
                        .ok_or("quality must be normal, plus, pro or pro_max")?,
                    None => self.settings.screenshot_quality,
                };
                self.take_screenshot(quality)?;
                json!({"folder": self.settings.screenshot_dir, "quality": quality.code()})
            }
            "file.cancelScreenshot" => {
                self.cancel_screenshot()?;
                json!({"cancelling": true})
            }
            "file.screenshotQuality" => {
                let quality = str_param(p, "quality")
                    .and_then(ScreenshotQuality::from_code)
                    .ok_or("quality must be normal, plus, pro or pro_max")?;
                self.settings.screenshot_quality = quality;
                json!({"quality": quality.code()})
            }
            // ── Layers
            "layer.basemap" => {
                let basemap = str_param(p, "basemap")
                    .and_then(globecraft_data::tiles::Provider::from_code)
                    .ok_or("unknown basemap; use satellite, wayback, streets or topo")?;
                self.settings.layers.basemap = basemap;
                json!({"basemap": basemap.code()})
            }
            "layer.waybackRelease" => {
                if self.wayback_list.is_empty() {
                    return Err(
                        "Wayback releases are still loading; show the wayback basemap first."
                            .into(),
                    );
                }
                let index = if let Some(date) = str_param(p, "date") {
                    self.wayback_list
                        .iter()
                        .position(|r| r.date.starts_with(date))
                        .ok_or_else(|| format!("no release dated {date}"))?
                } else {
                    let step = p.get("step").and_then(Value::as_i64).unwrap_or(1);
                    (self.wayback_idx as i64 + step).clamp(0, self.wayback_list.len() as i64 - 1)
                        as usize
                };
                self.settings.layers.basemap = globecraft_data::tiles::Provider::Wayback;
                self.set_wayback(index);
                json!({"release": self.wayback_list[index].date})
            }
            "layer.waybackCompare" => {
                if self.wayback_list.len() < 2 {
                    return Err(
                        "Wayback releases are still loading; show the wayback basemap first."
                            .into(),
                    );
                }
                let enabled = bool_param(p, "enabled").unwrap_or(
                    self.wayback_compare.is_none()
                        || p.get("date").is_some()
                        || p.get("left_date").is_some()
                        || p.get("swap").is_some()
                        || p.get("step").is_some()
                        || p.get("split").is_some(),
                );
                self.settings.layers.basemap = globecraft_data::tiles::Provider::Wayback;
                if !enabled {
                    self.set_wayback_compare(false);
                    return Ok(json!({"compare": false}));
                }
                if self.wayback_compare.is_none() {
                    self.set_wayback_compare(true);
                }
                let mut compare = self.wayback_compare.ok_or("nothing to compare")?;
                if let Some(date) = str_param(p, "date") {
                    compare.index = self
                        .wayback_list
                        .iter()
                        .position(|r| r.date.starts_with(date))
                        .ok_or_else(|| format!("no release dated {date}"))?;
                } else if let Some(step) = p.get("step").and_then(Value::as_i64) {
                    compare.index = (compare.index as i64 + step)
                        .clamp(0, self.wayback_list.len() as i64 - 1)
                        as usize;
                }
                if let Some(split) = p.get("split").and_then(Value::as_f64) {
                    compare.split = (split as f32)
                        .clamp(super::wayback::MIN_SPLIT, 1.0 - super::wayback::MIN_SPLIT);
                }
                // The left side is the shown release.
                let mut left = self.wayback_idx;
                if let Some(date) = str_param(p, "left_date") {
                    left = self
                        .wayback_list
                        .iter()
                        .position(|r| r.date.starts_with(date))
                        .ok_or_else(|| format!("no release dated {date}"))?;
                }
                if bool_param(p, "swap") == Some(true) {
                    (left, compare.index) = (compare.index, left);
                }
                if left != self.wayback_idx {
                    self.set_wayback(left);
                }
                self.wayback_compare = Some(compare);
                self.last_state = 0;
                json!({
                    "compare": true,
                    "left": self.wayback_list[self.wayback_idx].date,
                    "right": self.wayback_list[compare.index].date,
                    "split": compare.split,
                })
            }
            "layer.show" => {
                let layer = str_param(p, "layer")
                    .and_then(MapLayer::from_code)
                    .ok_or("unknown layer; see the layer.show parameters")?;
                let visible = bool_param(p, "visible").unwrap_or(!self.layer(layer));
                self.settings.set_layer_visible(layer, visible);
                // A refinement is useless without its parent.
                if visible && let Some(parent) = layer.parent() {
                    self.settings.set_layer_visible(parent, true);
                }
                json!({"layer": layer.code(), "visible": visible})
            }
            "layer.terrainExaggeration" => {
                let value = required_f64(p, "value")? as f32;
                self.settings.layers.terrain_exaggeration = value.clamp(
                    globecraft_data::settings::layers::MIN_TERRAIN_EXAGGERATION,
                    globecraft_data::settings::layers::MAX_TERRAIN_EXAGGERATION,
                );
                json!({"terrain_exaggeration": self.settings.layers.terrain_exaggeration})
            }
            "layer.stateAltitude" => {
                let km = required_f64(p, "km")?;
                if !km.is_finite() {
                    return Err("km must be a number".into());
                }
                self.settings.state_max_altitude_km =
                    km.clamp(0.0, globecraft_data::settings::STATE_MAX_ALTITUDE_MAX_KM);
                json!({"km": self.settings.state_max_altitude_km})
            }
            "world.earthModel" => {
                let model = str_param(p, "model")
                    .and_then(EarthModel::from_code)
                    .ok_or("unknown Earth model")?;
                if model != self.settings.earth_model {
                    self.settings.earth_model = model;
                    if earth {
                        self.fly = None;
                        self.ruler_paths.end_move();
                        self.ruler_drag = None;
                    }
                }
                json!({"model": model.code()})
            }
            // ── Worlds
            "world.body" => {
                let body = match str_param(p, "body") {
                    Some("earth") => Body::Earth,
                    Some("moon") => Body::Moon,
                    Some("custom") => Body::Custom,
                    _ => return Err("body must be earth, moon or custom".into()),
                };
                self.flat_earth.open = false;
                self.explorer.choose(body, &self.file_jobs);
                if body == Body::Custom {
                    self.explorer.open = true;
                }
                self.touch();
                json!({"body": str_param(p, "body")})
            }
            "ui.explorer" => {
                self.explorer.open = !self.explorer.open;
                json!({})
            }
            // ── Panes and interface
            "ui.pane.layers" | "ui.pane.places" | "ui.pane.explore" => {
                let panel = match id {
                    "ui.pane.layers" => SidebarPanel::Layers,
                    "ui.pane.places" => SidebarPanel::Places,
                    _ => SidebarPanel::Explore,
                };
                let layout = &mut self.settings.sidebar;
                if layout.open && layout.panel == panel {
                    layout.open = false;
                } else {
                    layout.panel = panel;
                    layout.open = true;
                }
                json!({})
            }
            "ui.pane.assistant" => {
                match str_param(p, "ask").filter(|q| !q.is_empty()) {
                    Some(question) => {
                        self.settings.workspace.open_right(RightPane::Assistant);
                        self.assistant.queue(question.to_string());
                    }
                    None => self.settings.workspace.toggle_right(RightPane::Assistant),
                }
                json!({})
            }
            "ui.pane.measure" => {
                self.settings.workspace.toggle_right(RightPane::Measure);
                json!({})
            }
            "ui.leftPane" => {
                self.settings.sidebar.open = !self.settings.sidebar.open;
                json!({})
            }
            "ui.navigationControls" => {
                let shown = toggled(p, !self.settings.hide_navigation_controls);
                self.settings.hide_navigation_controls = !shown;
                json!({"shown": shown})
            }
            "ui.tileDiagnostics" => {
                let shown = toggled(p, self.tile_diagnostics);
                self.tile_diagnostics = shown;
                json!({"shown": shown})
            }
            "ui.navigatorStyle" => {
                let style = str_param(p, "style")
                    .and_then(globecraft_data::settings::navigation::NavigatorStyle::from_code)
                    .ok_or("style must be globe or classic")?;
                self.settings.navigation.navigator = style;
                self.settings.hide_navigation_controls = false;
                json!({"style": style.code()})
            }
            "ui.ribbon" => {
                self.settings.workspace.ribbon_minimized =
                    !self.settings.workspace.ribbon_minimized;
                self.ribbon_peek = false;
                json!({})
            }
            "ui.theme" => {
                self.settings.appearance.theme = match str_param(p, "theme") {
                    Some("system") => ThemePreference::System,
                    Some("dark") => ThemePreference::Dark,
                    Some("light") => ThemePreference::Light,
                    _ => return Err("theme must be system, dark or light".into()),
                };
                json!({})
            }
            "ui.fullscreen" => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                json!({})
            }
            "ui.minimize" => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                json!({})
            }
            "ui.commandSearch" => {
                self.command_search = Some(super::ui::palette::CommandSearch::default());
                json!({})
            }
            "ui.settings" => {
                self.show_settings = !self.show_settings;
                json!({})
            }
            "settings.units" => {
                self.settings.distance_units = match str_param(p, "units") {
                    Some("system") => UnitPreference::System,
                    Some("metric") => UnitPreference::Metric,
                    Some("imperial") => UnitPreference::Imperial,
                    _ => return Err("units must be system, metric or imperial".into()),
                };
                json!({})
            }
            "settings.coordinateFormat" => {
                let format = str_param(p, "format")
                    .and_then(CoordinateFormat::from_code)
                    .ok_or("unknown coordinate format")?;
                self.settings.coordinate_display.select_format(format);
                json!({})
            }
            "settings.language" => {
                let preference = str_param(p, "language")
                    .and_then(LanguagePreference::from_code)
                    .ok_or("language must be system, en or pt-BR")?;
                self.set_language_preference(preference);
                json!({"language": self.lang.code()})
            }
            "style.line" => {
                use globecraft_data::settings::map_style::{
                    DashStyle, LineType, Rgba, clamp_width,
                };
                let line = str_param(p, "line")
                    .and_then(LineType::from_code)
                    .ok_or("line must be country, state, municipal or disputed")?;
                let color = match str_param(p, "color") {
                    Some(text) => {
                        Some(Rgba::parse_hex(text).ok_or("color must be #rrggbb or #rrggbbaa")?)
                    }
                    None => None,
                };
                let dash = match str_param(p, "dash") {
                    Some(code) => Some(
                        DashStyle::from_code(code).ok_or("dash must be solid, dashed or dotted")?,
                    ),
                    None => None,
                };
                let style = self.settings.map_style.line_mut(line);
                if let Some(color) = color {
                    style.color = color;
                }
                if let Some(width) = p.get("width").and_then(Value::as_f64) {
                    style.width_points = clamp_width(width);
                }
                if let Some(dash) = dash {
                    style.dash = dash;
                }
                json!({
                    "line": line.code(),
                    "color": style.color.hex(),
                    "width": style.width_points,
                    "dash": style.dash.code(),
                })
            }
            "style.labelSize" => {
                use globecraft_data::settings::map_style::{LabelType, clamp_label_size};
                let label = str_param(p, "label")
                    .and_then(LabelType::from_code)
                    .ok_or("label must be country, state or city")?;
                let size = clamp_label_size(required_f64(p, "size")?);
                self.settings.map_style.labels.set(label, size);
                json!({"label": label.code(), "size": size})
            }
            "style.reset" => {
                self.settings.map_style = Default::default();
                json!({})
            }
            "ui.shortcuts" => {
                self.show_shortcuts = !self.show_shortcuts;
                json!({})
            }
            "ui.about" => {
                self.show_about = !self.show_about;
                json!({})
            }
            "help.documentation" => {
                ctx.open_url(egui::OpenUrl::new_tab(DOCUMENTATION_URL));
                json!({})
            }
            "help.reportIssue" => {
                ctx.open_url(egui::OpenUrl::new_tab(ISSUES_URL));
                json!({})
            }
            "app.quit" => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                json!({})
            }
            // ── Read-only views
            "info.view" => self.view_json(),
            "info.places" => {
                let mut places = Vec::new();
                self.placemarks
                    .walk(|depth, node| places.push(place_json(depth, node)));
                json!({"count": self.placemarks.len(), "items": places})
            }
            "info.ruler" => self.ruler_json(),
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
        let earth = self.explorer.active == Body::Earth;
        match id {
            "camera.autoRotate" => on(self.auto_rotate),
            "ruler.enable" => on(self.ruler),
            "edit.undo" => CommandState {
                enabled: self.ruler_paths.can_undo(),
                checked: false,
            },
            "edit.redo" => CommandState {
                enabled: self.ruler_paths.can_redo(),
                checked: false,
            },
            "file.screenshot" => CommandState {
                enabled: self.image.is_some() && !self.screenshot_running(),
                checked: false,
            },
            "file.cancelScreenshot" => CommandState {
                enabled: self.screenshot_running(),
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
            "layer.waybackRelease" => CommandState {
                enabled: !self.wayback_list.is_empty(),
                checked: false,
            },
            "layer.waybackCompare" => CommandState {
                enabled: self.wayback_list.len() > 1,
                checked: self.active_compare().is_some(),
            },
            "layer.terrainExaggeration" => CommandState {
                enabled: self.layer(MapLayer::Terrain),
                checked: false,
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
            "ui.explorer" => on(self.explorer.open),
            "ui.pane.layers" => {
                on(self.settings.sidebar.open
                    && self.settings.sidebar.panel == SidebarPanel::Layers)
            }
            "ui.pane.places" => {
                on(self.settings.sidebar.open
                    && self.settings.sidebar.panel == SidebarPanel::Places)
            }
            "ui.pane.explore" => {
                on(self.settings.sidebar.open
                    && self.settings.sidebar.panel == SidebarPanel::Explore)
            }
            "ui.pane.assistant" => {
                on(self.settings.workspace.right_pane == Some(RightPane::Assistant))
            }
            "ui.pane.measure" => on(self.settings.workspace.right_pane == Some(RightPane::Measure)),
            "ui.leftPane" => on(self.settings.sidebar.open),
            "ui.navigationControls" => on(!self.settings.hide_navigation_controls),
            "ui.tileDiagnostics" => on(self.tile_diagnostics),
            "ui.navigatorStyle" => on(str_param(params, "style")
                .is_some_and(|style| style == self.settings.navigation.navigator.code())),
            "ui.ribbon" => on(self.settings.workspace.ribbon_minimized),
            "ui.theme" => on(match str_param(params, "theme") {
                Some("system") => self.settings.appearance.theme == ThemePreference::System,
                Some("dark") => self.settings.appearance.theme == ThemePreference::Dark,
                Some("light") => self.settings.appearance.theme == ThemePreference::Light,
                _ => false,
            }),
            "ui.fullscreen" => on(self.ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))),
            "ui.settings" => on(self.show_settings),
            "settings.units" => on(match str_param(params, "units") {
                Some("system") => self.settings.distance_units == UnitPreference::System,
                Some("metric") => self.settings.distance_units == UnitPreference::Metric,
                Some("imperial") => self.settings.distance_units == UnitPreference::Imperial,
                _ => false,
            }),
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
        let total_km = self.ruler_paths.distance_km(model);
        let paths: Vec<Vec<[f64; 2]>> = self
            .ruler_paths
            .paths()
            .map(|path| path.iter().map(|(lat, lon)| [*lat, *lon]).collect())
            .collect();
        let bearing = self
            .ruler_paths
            .last_leg()
            .and_then(|pair| model.initial_bearing(pair[0], pair[1]));
        json!({
            "enabled": self.ruler,
            "points": self.ruler_paths.point_count(),
            "paths_lat_lon": paths,
            "total_km": total_km,
            "total_display": self.settings.ruler_unit.format_with_angle(
                total_km,
                self.ruler_paths.angular_distance_degrees(model),
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
                .then(|| self.wayback_list.get(self.wayback_idx).map(|r| r.date.clone())).flatten(),
            "visible_layers": layers,
            "units": match self.distance_units() {
                globecraft_core::units::DistanceUnits::Metric => "metric",
                globecraft_core::units::DistanceUnits::Imperial => "imperial",
            },
            "places": self.placemarks.len(),
            "ruler_points": self.ruler_paths.point_count(),
            "search_query": (!self.query.trim().is_empty()).then(|| self.query.trim().to_string()),
            "search_results": results,
            "status": self.status,
        })
    }

    /// Commands bound to keyboard shortcuts, run when their keys are pressed.
    pub(in crate::app) fn command_shortcuts(&mut self, ctx: &egui::Context) {
        for id in SHORTCUT_COMMANDS {
            #[cfg(target_os = "macos")]
            if self.native_menu.is_some() && super::menu::native_shortcut(id) {
                continue;
            }
            if let Some(shortcut) = shortcut(id)
                && ctx.input_mut(|input| input.consume_shortcut(&shortcut))
            {
                self.run_reporting(id, Value::Null);
            }
        }
    }
}

/// The assistant's view of the application: commands run through [`App::run`].
pub(in crate::app) struct AssistantHost<'a>(pub &'a mut App);

impl globecraft_assistant::Host for AssistantHost<'_> {
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let spec = commands::find(id).ok_or_else(|| format!("unknown command '{id}'"))?;
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
        app.placemarks.len(),
        app.ruler_paths.point_count()
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
        self.place_reveal_requested = true;
        self.ruler_reveal_requested = false;
    }
    pub(in crate::app) fn reveal_ruler(&mut self) {
        self.ruler_reveal_requested = self.ruler;
        if self.ruler {
            self.place_reveal_requested = false;
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
