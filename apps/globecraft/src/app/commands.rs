//! Runs commands by id. The ribbon, menus, keyboard shortcuts, the command palette and the AI
//! assistant all come through [`App::run`], so each action has one implementation.
use super::App;
use eframe::egui;
use globecraft_core::layers::MapLayer;
use serde_json::{Value, json};

mod camera;
mod files;
mod info;
mod interface;
mod layers;
mod places;
mod ruler;
mod settings;
mod shortcuts;
mod state;
#[cfg(feature = "assistant")]
pub(in crate::app) use info::context_text;
// The native menu (macOS) and the tests ask for a command's keys.
#[cfg(any(target_os = "macos", test))]
pub(in crate::app) use shortcuts::shortcut;
pub(in crate::app) use shortcuts::{SHORTCUT_COMMANDS, shortcut_text, tool_shortcut_text};

/// Whether a command can run now, and whether its toggle or choice is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct CommandState {
    pub enabled: bool,
    pub checked: bool,
}

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

impl App {
    /// Runs a command; problems are reported in the status bar.
    pub(in crate::app) fn run_reporting(&mut self, id: &str, params: Value) {
        if let Err(error) = self.run(id, params) {
            self.status = error;
        }
    }

    /// Runs command `id` with JSON `params`, returning what it did as JSON. Each
    /// feature runs its own commands (the modules below, and `run_*` beside the
    /// features' states); this only hands the command to its feature.
    pub(in crate::app) fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let p = &params;
        let feature = Feature::of(id).ok_or_else(|| format!("unknown command '{id}'"))?;
        let result = match feature {
            Feature::Camera => self.run_camera(id, p)?,
            Feature::Places => self.run_places(id, p)?,
            Feature::Ruler => self.run_ruler(id, p)?,
            Feature::Files => self.run_file(id, p)?,
            Feature::Layers => self.run_layer(id, p)?,
            Feature::Worlds => self.run_world(id, p)?,
            Feature::UserData => self.run_user_data(id, p)?,
            Feature::Time => self.run_time(id, p)?,
            Feature::Flights => self.run_flight(id, p)?,
            Feature::Satellites => self.run_satellite(id, p)?,
            Feature::Legend => self.run_legend(p),
            Feature::Settings => self.run_settings(id, p)?,
            Feature::Info => self.run_info(id, p)?,
            Feature::Interface => self.run_interface(id, p)?,
        };
        self.ctx.request_repaint();
        Ok(result)
    }
}

/// The feature that runs a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Feature {
    Camera,
    Places,
    Ruler,
    Files,
    Layers,
    Worlds,
    UserData,
    Time,
    Flights,
    Satellites,
    Legend,
    Settings,
    Info,
    Interface,
}

impl Feature {
    /// The feature that runs command `id`: the one its first part names, except for
    /// the interface commands that open a feature's own pane or card.
    fn of(id: &str) -> Option<Self> {
        let group = id.split_once('.').map_or("", |(group, _)| group);
        Some(match (group, id) {
            (_, "ui.addData") | ("data", _) => Self::UserData,
            (_, "ui.legend") => Self::Legend,
            (_, "ui.explorer") | ("world", _) => Self::Worlds,
            (_, "ui.here") | ("info", _) => Self::Info,
            ("camera" | "search", _) => Self::Camera,
            ("places", _) => Self::Places,
            ("ruler" | "edit", _) => Self::Ruler,
            ("file" | "view", _) => Self::Files,
            ("layer", _) => Self::Layers,
            ("settings" | "style", _) => Self::Settings,
            ("time", _) => Self::Time,
            ("flight", _) => Self::Flights,
            ("satellite", _) => Self::Satellites,
            ("ui" | "assistant" | "help" | "app", _) => Self::Interface,
            _ => return None,
        })
    }

    /// The source of the handler, where the tests look for a command's id.
    #[cfg(test)]
    fn handler_source(self) -> &'static str {
        match self {
            Self::Camera => include_str!("commands/camera.rs"),
            Self::Places => include_str!("commands/places.rs"),
            Self::Ruler => include_str!("commands/ruler.rs"),
            Self::Files => include_str!("commands/files.rs"),
            Self::Layers | Self::Worlds => include_str!("commands/layers.rs"),
            Self::UserData => include_str!("user_layers.rs"),
            Self::Time => include_str!("time.rs"),
            Self::Flights => include_str!("selected_flight.rs"),
            Self::Satellites => include_str!("selected_satellite.rs"),
            Self::Legend => include_str!("legend.rs"),
            Self::Settings => include_str!("commands/settings.rs"),
            Self::Info => include_str!("commands/info.rs"),
            Self::Interface => include_str!("commands/interface.rs"),
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
mod tests {
    use super::*;
    use globecraft_core::commands;

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
                let is_command_prefix = commands::COMMANDS
                    .iter()
                    .any(|spec| spec.id.split('.').next() == Some(prefix));
                if !id.contains('.') || !is_command_prefix {
                    continue;
                }
                *checked += 1;
                assert!(
                    commands::find(id).is_some(),
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
    fn every_command_in_the_catalogue_reaches_a_feature_that_runs_it() {
        for spec in commands::COMMANDS {
            let feature =
                Feature::of(spec.id).unwrap_or_else(|| panic!("no feature runs {}", spec.id));
            // The legend has one command, run without looking at its id.
            if feature == Feature::Legend {
                continue;
            }
            assert!(
                feature
                    .handler_source()
                    .contains(&format!("\"{}\"", spec.id)),
                "{} goes to {feature:?}, whose handler does not name it",
                spec.id
            );
        }
        // Names outside the catalogue are no feature's, with or without a known first part.
        for id in ["nonsense", "data", "camera", "unknown.command", ""] {
            assert_eq!(Feature::of(id), None, "{id}");
        }
        assert_eq!(Feature::of("ui.addData"), Some(Feature::UserData));
        assert_eq!(Feature::of("ui.settings"), Some(Feature::Interface));
        assert_eq!(Feature::of("edit.undo"), Some(Feature::Ruler));
    }
}
