//! Layers, imagery dates, view links and worlds. Handlers for the `layer.*`, `view.*`,
//! `world.*`, `style.*` commands.
use super::*;

impl App {
    pub(in crate::app) fn run_layers(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        let earth = self.explorer.on_earth();
        Ok(match id {
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
                    compare.split = (split as f32).clamp(
                        super::super::wayback::MIN_SPLIT,
                        1.0 - super::super::wayback::MIN_SPLIT,
                    );
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
                self.imagery.last_state = 0;
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
