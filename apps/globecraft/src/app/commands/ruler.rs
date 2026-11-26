//! Ruler commands (`ruler.*`) and its undo history (`edit.*`).
use super::position;
use crate::app::App;
use globecraft_core::commands::RulerCommand;
use globecraft_core::units::RulerUnit;
use globecraft_data::settings::workspace::PaneTab;
use serde_json::{Value, json};

impl App {
    /// Runs a `ruler.*` or `edit.*` command.
    pub(super) fn run_ruler(&mut self, command: RulerCommand) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match command {
            RulerCommand::Enable(p) => {
                self.measure.on = p.toggled(self.measure.on);
                self.reveal_ruler();
                json!({"ruler": self.measure.on})
            }
            RulerCommand::AddPoint(p) => {
                let point = position(p.lat, p.lon)?;
                self.measure.on = true;
                self.measure
                    .paths
                    .add_point(point, p.new_path.unwrap_or(false));
                self.ruler_json()
            }
            RulerCommand::Clear => {
                self.measure.paths.clear();
                self.measure.drag = None;
                self.ruler_json()
            }
            RulerCommand::CloseShape(p) => {
                let closed = p.toggled(self.measure.paths.closed());
                self.measure.paths.set_closed(closed);
                self.ruler_json()
            }
            RulerCommand::Profile => {
                let profile = self
                    .ruler_profile()
                    .ok_or("the ruler needs two points on the Earth for a profile")?;
                profile.to_json()
            }
            RulerCommand::Unit(p) => {
                let unit = RulerUnit::from_code(&p.unit).ok_or("unknown unit")?;
                self.settings.ruler_unit = unit;
                self.ruler_json()
            }
            RulerCommand::EditPoints => {
                self.measure.paths.end_move();
                self.measure.drag = None;
                self.measure.point_editor = Some(crate::app::ui::ruler::PointEditor::new((
                    self.cam.lat,
                    self.cam.lon,
                )));
                // The editor is a section of the Measure pane.
                self.settings.workspace.open_right(PaneTab::Measure);
                json!({})
            }
            RulerCommand::Undo => {
                self.apply_ruler_history(crate::app::ruler_history::HistoryCommand::Undo, &ctx);
                self.ruler_json()
            }
            RulerCommand::Redo => {
                self.apply_ruler_history(crate::app::ruler_history::HistoryCommand::Redo, &ctx);
                self.ruler_json()
            }
        })
    }
}
