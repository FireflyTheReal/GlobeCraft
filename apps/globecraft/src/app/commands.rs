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
use globecraft_data::settings::experience::ExperienceLevel;
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
        let earth = self.explorer.active == Body::Earth;
        let result = match id {
            // ── Camera
            "camera.flyTo" => {
                let (lat, lon) = position(p)?;
                let alt = f64_param(p, "altitude_km")?.unwrap_or(self.cam.alt).clamp(
                    globecraft_core::MIN_CAMERA_ALTITUDE_KM,
                    globecraft_core::MAX_CAMERA_ALTITUDE_KM,
                );
                let heading = f64_param(p, "heading_deg")?;
                let tilt = f64_param(p, "tilt_deg")?;
                self.fly_to_with_orientation(lat, lon, alt, heading, tilt);
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
                    // A given altitude is kept (the tilt yields); otherwise the tilt
                    // wins and the camera backs off as far as it needs.
                    if p.get("altitude_km").is_some() {
                        self.cam.set_tilt(tilt);
                    } else {
                        self.cam.tilt_zooming_out(tilt);
                    }
                }
                self.touch();
                self.camera_json()
            }
            "search.clear" => {
                self.search.results.clear();
                self.search.query.clear();
                self.search.suggestions_open = false;
                json!({})
            }
            "search.place" => match str_param(p, "query") {
                Some(query) if !query.is_empty() => {
                    self.search.query = query.to_string();
                    self.search.suggestions_open = false;
                    self.search.fly = bool_param(p, "fly").unwrap_or(true);
                    self.start_search(&ctx);
                    json!({"status": self.status, "note": "Results arrive in a moment; describe_view lists them."})
                }
                // Without a query (the ribbon button), the search box takes the focus.
                _ => {
                    self.search.focus = true;
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
                self.cam.tilt_zooming_out(tilt);
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
                    self.places.new_name = name.to_string();
                }
                self.add_center_placemark();
                if let Some(desc) = str_param(p, "description")
                    && let Some(node) = self
                        .places
                        .selection
                        .and_then(|id| self.places.items.find_mut(id))
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
                        self.places
                            .items
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
                            &[("number", (self.places.items.len() + 1).to_string().into())],
                        )
                    });
                let node = PlaceNode::marker(Placemark {
                    name: name.clone(),
                    desc: str_param(p, "description").unwrap_or_default().to_string(),
                    lat,
                    lon,
                });
                self.places.selection = Some(node.id());
                self.places.items.insert(parent, node);
                json!({"added": name})
            }
            "places.addFolder" => {
                if let Some(name) = str_param(p, "name") {
                    self.places.new_name = name.to_string();
                }
                self.add_places_folder();
                self.reveal_places();
                json!({})
            }
            "places.flyTo" => {
                let name = str_param(p, "name").ok_or("name is required")?;
                let node = self
                    .places
                    .items
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
                    .places
                    .items
                    .find_by_name(name)
                    .map(PlaceNode::id)
                    .ok_or_else(|| format!("no place named '{name}'"))?;
                self.places.items.remove(id);
                if self.places.selection == Some(id) {
                    self.places.selection = None;
                }
                json!({"removed": name})
            }
            "places.open" => {
                self.pick_kml_open();
                json!({})
            }
            "places.save" => {
                let path = self.resolve_kml_path(&self.places.kml_path.clone());
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
                self.measure.on = toggled(p, self.measure.on);
                self.reveal_ruler();
                json!({"ruler": self.measure.on})
            }
            "ruler.addPoint" => {
                let point = position(p)?;
                self.measure.on = true;
                self.measure
                    .paths
                    .add_point(point, bool_param(p, "new_path").unwrap_or(false));
                self.ruler_json()
            }
            "ruler.clear" => {
                self.measure.paths.clear();
                self.measure.drag = None;
                self.ruler_json()
            }
            "ruler.closeShape" => {
                let closed = toggled(p, self.measure.paths.closed());
                self.measure.paths.set_closed(closed);
                self.ruler_json()
            }
            "ruler.profile" => {
                let profile = self
                    .ruler_profile()
                    .ok_or("the ruler needs two points on the Earth for a profile")?;
                profile.to_json()
            }
            "ruler.unit" => {
                let unit = str_param(p, "unit")
                    .and_then(RulerUnit::from_code)
                    .ok_or("unknown unit")?;
                self.settings.ruler_unit = unit;
                self.ruler_json()
            }
            "ruler.editPoints" => {
                self.measure.paths.end_move();
                self.measure.drag = None;
                self.measure.point_editor = Some(super::ui::ruler::PointEditor::new((
                    self.cam.lat,
                    self.cam.lon,
                )));
                // The editor is a section of the Measure pane.
                self.settings.workspace.open_right(RightPane::Measure);
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
                if self.wayback.releases.is_empty() {
                    return Err(
                        "Wayback releases are still loading; show the wayback basemap first."
                            .into(),
                    );
                }
                let index = if let Some(date) = str_param(p, "date") {
                    self.wayback
                        .releases
                        .iter()
                        .position(|r| r.date.starts_with(date))
                        .ok_or_else(|| format!("no release dated {date}"))?
                } else {
                    let step = p.get("step").and_then(Value::as_i64).unwrap_or(1);
                    (self.wayback.index as i64 + step)
                        .clamp(0, self.wayback.releases.len() as i64 - 1)
                        as usize
                };
                self.settings.layers.basemap = globecraft_data::tiles::Provider::Wayback;
                self.set_wayback(index);
                json!({"release": self.wayback.releases[index].date})
            }
            "layer.waybackCompare" => {
                if self.wayback.releases.len() < 2 {
                    return Err(
                        "Wayback releases are still loading; show the wayback basemap first."
                            .into(),
                    );
                }
                let enabled = bool_param(p, "enabled").unwrap_or(
                    self.wayback.compare.is_none()
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
                if self.wayback.compare.is_none() {
                    self.set_wayback_compare(true);
                }
                let mut compare = self.wayback.compare.ok_or("nothing to compare")?;
                if let Some(date) = str_param(p, "date") {
                    compare.index = self
                        .wayback
                        .releases
                        .iter()
                        .position(|r| r.date.starts_with(date))
                        .ok_or_else(|| format!("no release dated {date}"))?;
                } else if let Some(step) = p.get("step").and_then(Value::as_i64) {
                    compare.index = (compare.index as i64 + step)
                        .clamp(0, self.wayback.releases.len() as i64 - 1)
                        as usize;
                }
                if let Some(split) = p.get("split").and_then(Value::as_f64) {
                    compare.split = (split as f32)
                        .clamp(super::wayback::MIN_SPLIT, 1.0 - super::wayback::MIN_SPLIT);
                }
                // The left side is the shown release.
                let mut left = self.wayback.index;
                if let Some(date) = str_param(p, "left_date") {
                    left = self
                        .wayback
                        .releases
                        .iter()
                        .position(|r| r.date.starts_with(date))
                        .ok_or_else(|| format!("no release dated {date}"))?;
                }
                if bool_param(p, "swap") == Some(true) {
                    (left, compare.index) = (compare.index, left);
                }
                if left != self.wayback.index {
                    self.set_wayback(left);
                }
                self.wayback.compare = Some(compare);
                self.last_state = 0;
                json!({
                    "compare": true,
                    "left": self.wayback.releases[self.wayback.index].date,
                    "right": self.wayback.releases[compare.index].date,
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
            "view.copyLink" => self.copy_view_link(&ctx),
            "view.openLink" => {
                let link = str_param(p, "link").ok_or("link is required")?.to_owned();
                self.open_view_link(&link)?
            }
            "layer.opacity" => {
                let layer = str_param(p, "layer")
                    .and_then(MapLayer::from_code)
                    .ok_or("unknown layer; see the layer.opacity parameters")?;
                let opacity = p
                    .get("opacity")
                    .and_then(Value::as_f64)
                    .ok_or("opacity (0 to 1) is required")?;
                if !self.settings.layers.opacity.set(layer, opacity as f32) {
                    return Err(format!("{} has no opacity", layer.code()));
                }
                json!({"layer": layer.code(), "opacity": self.settings.layers.opacity.get(layer)})
            }
            "layer.solo" => self.solo_layer(str_param(p, "layer").map(str::to_owned))?,
            "layer.terrainExaggeration" => {
                let value = required_f64(p, "value")? as f32;
                self.settings.layers.terrain_exaggeration = value.clamp(
                    globecraft_data::settings::layers::MIN_TERRAIN_EXAGGERATION,
                    globecraft_data::settings::layers::MAX_TERRAIN_EXAGGERATION,
                );
                json!({"terrain_exaggeration": self.settings.layers.terrain_exaggeration})
            }
            "layer.flatLiveObjects" => {
                let layers = &mut self.settings.layers;
                layers.flat_live_objects = bool_param(p, "on").unwrap_or(!layers.flat_live_objects);
                json!({"flat": layers.flat_live_objects})
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
                        self.measure.paths.end_move();
                        self.measure.drag = None;
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
                    self.settings.workspace.open_right(RightPane::Explorer);
                }
                self.touch();
                json!({"body": str_param(p, "body")})
            }
            "ui.explorer" => {
                self.settings.workspace.toggle_right(RightPane::Explorer);
                json!({})
            }
            // ── Panes and interface
            "ui.pane.layers" | "ui.pane.places" => {
                let panel = match id {
                    "ui.pane.layers" => SidebarPanel::Layers,
                    _ => SidebarPanel::Places,
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
            "assistant.stop" => {
                let running = self.assistant.busy();
                self.assistant_stop();
                json!({"stopped": running})
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
            "ui.focusNext" | "ui.focusPrevious" => {
                self.focus_next_region(&ctx, id == "ui.focusPrevious");
                json!({})
            }
            "ui.ribbon.pin" => {
                let group = str_param(p, "group").ok_or("group is required")?;
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                let on = bool_param(p, "on").unwrap_or(!self.tool_pinned(group, tool));
                let entry = super::tools::group_entry(group, tool);
                let toggled = on != super::tools::pinned_by_default(group, tool);
                self.settings.workspace.set_ribbon_toggled(&entry, toggled);
                json!({"pinned": on})
            }
            "ui.quickTools.pin" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                super::tools::parse_tool(tool).ok_or("tool is not a command")?;
                let present = self
                    .settings
                    .workspace
                    .quick_tools
                    .iter()
                    .any(|k| k == tool);
                let on = bool_param(p, "on").unwrap_or(!present);
                self.settings.workspace.set_quick_tool(tool, on);
                let now = self
                    .settings
                    .workspace
                    .quick_tools
                    .iter()
                    .any(|k| k == tool);
                if on && !now {
                    return Err(format!(
                        "the quick-access row holds at most {} tools",
                        globecraft_data::settings::workspace::MAX_QUICK_TOOLS
                    ));
                }
                json!({"pinned": now})
            }
            "ui.shortcut.edit" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                super::tools::parse_tool(tool).ok_or("tool is not a command")?;
                self.ribbon_tools.capture = Some(tool.to_string());
                json!({})
            }
            "ui.shortcut.set" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?.to_string();
                super::tools::parse_tool(&tool).ok_or("tool is not a command")?;
                self.set_shortcut(&tool, str_param(p, "keys"))?
            }
            "ui.ribbon.resetTools" => {
                let defaults = globecraft_data::settings::workspace::WorkspaceLayout::default();
                let w = &mut self.settings.workspace;
                w.ribbon_toggled = defaults.ribbon_toggled;
                w.quick_tools = defaults.quick_tools;
                w.shortcuts = defaults.shortcuts;
                super::tools::sync_shortcuts(&w.shortcuts);
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
                self.windows.settings = !self.windows.settings;
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
            "settings.experience" => {
                self.settings.experience = str_param(p, "level")
                    .and_then(ExperienceLevel::from_code)
                    .ok_or("level must be basic or advanced")?;
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
                    .ok_or_else(|| {
                        let codes: Vec<&str> =
                            LineType::ALL.iter().map(|kind| kind.code()).collect();
                        format!("line must be one of {}", codes.join(", "))
                    })?;
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
                    .ok_or("label must be country, state, city or road")?;
                let size = clamp_label_size(required_f64(p, "size")?);
                self.settings.map_style.labels.set(label, size);
                json!({"label": label.code(), "size": size})
            }
            "style.reset" => {
                self.settings.map_style = Default::default();
                json!({})
            }
            "ui.shortcuts" => {
                self.windows.shortcuts = !self.windows.shortcuts;
                json!({})
            }
            "ui.about" => {
                self.windows.about = !self.windows.about;
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
            "ui.here" => {
                let (lat, lon) = position(p)?;
                self.close_other_cards(super::here::Card::Here);
                self.here_card = Some(super::here::HereCard { lat, lon });
                json!({"lat": lat, "lon": lon})
            }
            id if id.starts_with("data.") || id == "ui.addData" => self.run_user_data(id, p)?,
            id if id.starts_with("time.") => self.run_time(id, p)?,
            id if id.starts_with("flight.") => self.run_flight(id, p)?,
            id if id.starts_with("satellite.") => self.run_satellite(id, p)?,
            "ui.legend" => self.run_legend(p),
            "ui.timeline" => {
                self.timeline.open = bool_param(p, "visible").unwrap_or(!self.timeline.open);
                json!({"visible": self.timeline.open})
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
        let earth = self.explorer.active == Body::Earth;
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
                enabled: self.image.is_some() && !self.screenshot_running(),
                checked: false,
            },
            "satellite.clear" => CommandState {
                enabled: self.satellite_selection.selected().is_some(),
                checked: false,
            },
            "satellite.select" => CommandState {
                enabled: earth && self.layer(MapLayer::Satellites),
                checked: false,
            },
            "flight.clear" => CommandState {
                enabled: self.flight_selection.selected().is_some(),
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
                enabled: self.assistant.busy(),
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
            "ui.pane.assistant" => {
                on(self.settings.workspace.right_pane == Some(RightPane::Assistant))
            }
            "ui.pane.measure" => on(self.settings.workspace.right_pane == Some(RightPane::Measure)),
            "ui.leftPane" => on(self.settings.sidebar.open),
            "ui.navigationControls" => on(!self.settings.hide_navigation_controls),
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
