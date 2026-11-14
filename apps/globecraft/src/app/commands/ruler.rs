//! Ruler commands (`ruler.*`) and its undo history (`edit.*`).
use super::{bool_param, position, str_param, toggled};
use crate::app::App;
use globecraft_core::units::RulerUnit;
use globecraft_data::settings::workspace::RightPane;
use serde_json::{Value, json};

impl App {
    /// Runs a `ruler.*` or `edit.*` command.
    pub(super) fn run_ruler(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match id {
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
                self.measure.point_editor = Some(crate::app::ui::ruler::PointEditor::new((
                    self.cam.lat,
                    self.cam.lon,
                )));
                // The editor is a section of the Measure pane.
                self.settings.workspace.open_right(RightPane::Measure);
                json!({})
            }
            "edit.undo" | "edit.redo" => {
                let command = if id == "edit.undo" {
                    crate::app::ruler_history::HistoryCommand::Undo
                } else {
                    crate::app::ruler_history::HistoryCommand::Redo
                };
                self.apply_ruler_history(command, &ctx);
                self.ruler_json()
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }
}
