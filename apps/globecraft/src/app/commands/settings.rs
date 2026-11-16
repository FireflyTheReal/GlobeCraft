//! Settings and map style commands (`settings.*`, `style.*`).
use crate::app::App;
use globecraft_core::{
    commands::SettingsCommand, coordinates::CoordinateFormat, language::LanguagePreference,
};
use globecraft_data::settings::{UnitPreference, experience::ExperienceLevel};
use serde_json::{Value, json};

impl App {
    /// Runs a `settings.*` or `style.*` command.
    pub(super) fn run_settings(&mut self, command: SettingsCommand) -> Result<Value, String> {
        Ok(match command {
            SettingsCommand::Units(p) => {
                self.settings.distance_units = match p.units.as_str() {
                    "system" => UnitPreference::System,
                    "metric" => UnitPreference::Metric,
                    "imperial" => UnitPreference::Imperial,
                    _ => return Err("units must be system, metric or imperial".into()),
                };
                json!({})
            }
            SettingsCommand::Experience(p) => {
                self.settings.experience = ExperienceLevel::from_code(&p.level)
                    .ok_or("level must be basic or advanced")?;
                json!({})
            }
            SettingsCommand::CoordinateFormat(p) => {
                let format =
                    CoordinateFormat::from_code(&p.format).ok_or("unknown coordinate format")?;
                self.settings.coordinate_display.select_format(format);
                json!({})
            }
            SettingsCommand::Language(p) => {
                let preference = LanguagePreference::from_code(&p.language)
                    .ok_or("language must be system, en or pt-BR")?;
                self.set_language_preference(preference);
                json!({"language": self.lang.code()})
            }
            SettingsCommand::Line(p) => {
                use globecraft_data::settings::map_style::{
                    DashStyle, LineType, Rgba, clamp_width,
                };
                let line = LineType::from_code(&p.line).ok_or_else(|| {
                    let codes: Vec<&str> = LineType::ALL.iter().map(|kind| kind.code()).collect();
                    format!("line must be one of {}", codes.join(", "))
                })?;
                let color = match &p.color {
                    Some(text) => {
                        Some(Rgba::parse_hex(text).ok_or("color must be #rrggbb or #rrggbbaa")?)
                    }
                    None => None,
                };
                let dash = match &p.dash {
                    Some(code) => Some(
                        DashStyle::from_code(code).ok_or("dash must be solid, dashed or dotted")?,
                    ),
                    None => None,
                };
                let style = self.settings.map_style.line_mut(line);
                if let Some(color) = color {
                    style.color = color;
                }
                if let Some(width) = p.width {
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
            SettingsCommand::LabelSize(p) => {
                use globecraft_data::settings::map_style::{LabelType, clamp_label_size};
                let label = LabelType::from_code(&p.label)
                    .ok_or("label must be country, state, city or road")?;
                let size = clamp_label_size(p.size);
                self.settings.map_style.labels.set(label, size);
                json!({"label": label.code(), "size": size})
            }
            SettingsCommand::ResetStyle => {
                self.settings.map_style = Default::default();
                json!({})
            }
        })
    }
}
