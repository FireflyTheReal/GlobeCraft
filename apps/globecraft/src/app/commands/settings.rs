//! Settings and map style commands (`settings.*`, `style.*`).
use super::{required_f64, str_param};
use crate::app::App;
use globecraft_core::{coordinates::CoordinateFormat, language::LanguagePreference};
use globecraft_data::settings::{UnitPreference, experience::ExperienceLevel};
use serde_json::{Value, json};

impl App {
    /// Runs a `settings.*` or `style.*` command.
    pub(super) fn run_settings(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        Ok(match id {
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
            _ => return Err(format!("unknown command '{id}'")),
        })
    }
}
