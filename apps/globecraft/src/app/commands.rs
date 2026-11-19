//! Runs commands by id. The ribbon, menus, keyboard shortcuts, the command palette and the AI
//! assistant all come through [`App::run`], so each action has one implementation.
use super::App;
use eframe::egui;
use globecraft_core::{commands::Command, layers::MapLayer};
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

/// A latitude/longitude pair, validated like typed coordinates.
fn position(lat: f64, lon: f64) -> Result<(f64, f64), String> {
    globecraft_core::coordinates::validate_coordinates(lat, lon).map_err(|e| e.to_string())
}

impl App {
    /// Runs a command; problems are reported in the status bar.
    pub(in crate::app) fn run_reporting(&mut self, id: &str, params: Value) {
        if let Err(error) = self.run(id, params) {
            self.status = error;
        }
    }

    /// Runs command `id` with JSON `params`, returning what it did as JSON. The
    /// parameters are read once, into the command's own struct; each feature then runs
    /// its own group of commands (the modules below, and `run_*` beside the features'
    /// states), and its `match` must name every command of the group.
    pub(in crate::app) fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let result = match Command::parse(id, &params)? {
            Command::Camera(command) => self.run_camera(command)?,
            Command::Places(command) => self.run_places(command)?,
            Command::Ruler(command) => self.run_ruler(command)?,
            Command::Files(command) => self.run_file(command)?,
            Command::Layers(command) => self.run_layer(command)?,
            Command::Worlds(command) => self.run_world(command)?,
            Command::Data(command) => self.run_user_data(command)?,
            Command::Time(command) => self.run_time(command)?,
            Command::Flights(command) => self.run_flight(command)?,
            Command::Satellites(command) => self.run_satellite(command)?,
            Command::Legend(command) => self.run_legend(command),
            Command::Settings(command) => self.run_settings(command)?,
            Command::Info(command) => self.run_info(command)?,
            Command::Interface(command) => self.run_interface(command)?,
        };
        self.ctx.request_repaint();
        Ok(result)
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
        if !Self::offers(layer) {
            return;
        }
        let mut on = self.layer(layer);
        if ui.checkbox(&mut on, label).changed() {
            self.run_reporting("layer.show", json!({"layer": layer.code(), "visible": on}));
        }
    }
}

#[cfg(test)]
mod tests {
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
                        // `\r`: a Windows checkout ends lines with CR LF.
                        .all(|c| c.is_ascii_lowercase() || "_&, \r\n".contains(c))
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
}
