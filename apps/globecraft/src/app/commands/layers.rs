//! Layer commands (`layer.*`) and the worlds (`world.*`).
use crate::app::App;
use globecraft_core::commands::{LayerCommand, WorldCommand};
use globecraft_core::{body::Body, earth_model::EarthModel, layers::MapLayer};
use globecraft_data::settings::workspace::PaneTab;
use serde_json::{Value, json};

impl App {
    /// Runs a `layer.*` command.
    pub(super) fn run_layer(&mut self, command: LayerCommand) -> Result<Value, String> {
        Ok(match command {
            LayerCommand::Basemap(p) => {
                let basemap = globecraft_data::tiles::Provider::from_code(&p.basemap)
                    .ok_or("unknown basemap; use satellite, wayback, streets or topo")?;
                self.settings.layers.basemap = basemap;
                json!({"basemap": basemap.code()})
            }
            LayerCommand::WaybackRelease(p) => {
                if self.wayback.releases.is_empty() {
                    return Err(
                        "Wayback releases are still loading; show the wayback basemap first."
                            .into(),
                    );
                }
                let index = if let Some(date) = p.date {
                    self.wayback
                        .releases
                        .iter()
                        .position(|r| r.date.starts_with(&date))
                        .ok_or_else(|| format!("no release dated {date}"))?
                } else {
                    let step = p.step.unwrap_or(1);
                    (self.wayback.index as i64 + step)
                        .clamp(0, self.wayback.releases.len() as i64 - 1)
                        as usize
                };
                self.settings.layers.basemap = globecraft_data::tiles::Provider::Wayback;
                self.set_wayback(index);
                json!({"release": self.wayback.releases[index].date})
            }
            LayerCommand::WaybackCompare(p) => {
                if self.wayback.releases.len() < 2 {
                    return Err(
                        "Wayback releases are still loading; show the wayback basemap first."
                            .into(),
                    );
                }
                let enabled = p.enabled.unwrap_or(
                    self.wayback.compare.is_none()
                        || p.date.is_some()
                        || p.left_date.is_some()
                        || p.swap.is_some()
                        || p.step.is_some()
                        || p.split.is_some(),
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
                if let Some(date) = &p.date {
                    compare.index = self
                        .wayback
                        .releases
                        .iter()
                        .position(|r| r.date.starts_with(date.as_str()))
                        .ok_or_else(|| format!("no release dated {date}"))?;
                } else if let Some(step) = p.step {
                    compare.index = (compare.index as i64 + step)
                        .clamp(0, self.wayback.releases.len() as i64 - 1)
                        as usize;
                }
                if let Some(split) = p.split {
                    compare.split = (split as f32).clamp(
                        crate::app::wayback::MIN_SPLIT,
                        1.0 - crate::app::wayback::MIN_SPLIT,
                    );
                }
                // The left side is the shown release.
                let mut left = self.wayback.index;
                if let Some(date) = &p.left_date {
                    left = self
                        .wayback
                        .releases
                        .iter()
                        .position(|r| r.date.starts_with(date.as_str()))
                        .ok_or_else(|| format!("no release dated {date}"))?;
                }
                if p.swap == Some(true) {
                    (left, compare.index) = (compare.index, left);
                }
                if left != self.wayback.index {
                    self.set_wayback(left);
                }
                self.wayback.compare = Some(compare);
                self.frame.last_state = 0;
                json!({
                    "compare": true,
                    "left": self.wayback.releases[self.wayback.index].date,
                    "right": self.wayback.releases[compare.index].date,
                    "split": compare.split,
                })
            }
            LayerCommand::Show(p) => {
                let layer = MapLayer::from_code(&p.layer)
                    .ok_or("unknown layer; see the layer.show parameters")?;
                if !Self::offers(layer) {
                    return Err("this build of GlobeCraft has no live feeds".into());
                }
                let visible = p.visible.unwrap_or(!self.layer(layer));
                self.settings.set_layer_visible(layer, visible);
                // A refinement is useless without its parent.
                if visible && let Some(parent) = layer.parent() {
                    self.settings.set_layer_visible(parent, true);
                }
                json!({"layer": layer.code(), "visible": visible})
            }
            LayerCommand::Opacity(p) => {
                let layer = MapLayer::from_code(&p.layer)
                    .ok_or("unknown layer; see the layer.opacity parameters")?;
                if !self.settings.layers.opacity.set(layer, p.opacity as f32) {
                    return Err(format!("{} has no opacity", layer.code()));
                }
                json!({"layer": layer.code(), "opacity": self.settings.layers.opacity.get(layer)})
            }
            LayerCommand::Solo(p) => self.solo_layer(p.layer)?,
            LayerCommand::TerrainExaggeration(p) => {
                let value = p.value as f32;
                self.settings.layers.terrain_exaggeration = value.clamp(
                    globecraft_data::settings::layers::MIN_TERRAIN_EXAGGERATION,
                    globecraft_data::settings::layers::MAX_TERRAIN_EXAGGERATION,
                );
                json!({"terrain_exaggeration": self.settings.layers.terrain_exaggeration})
            }
            LayerCommand::FlatLiveObjects(p) => {
                let layers = &mut self.settings.layers;
                layers.flat_live_objects = p.toggled(layers.flat_live_objects);
                json!({"flat": layers.flat_live_objects})
            }
            LayerCommand::StateAltitude(p) => {
                // Finite: the parameter reader refuses anything else.
                self.settings.state_max_altitude_km =
                    p.km.clamp(0.0, globecraft_data::settings::STATE_MAX_ALTITUDE_MAX_KM);
                json!({"km": self.settings.state_max_altitude_km})
            }
        })
    }
}

impl App {
    /// Runs a `world.*` command, or `ui.explorer`.
    pub(super) fn run_world(&mut self, command: WorldCommand) -> Result<Value, String> {
        let earth = self.explorer.on_earth();
        Ok(match command {
            WorldCommand::EarthModel(p) => {
                let model = EarthModel::from_code(&p.model).ok_or("unknown Earth model")?;
                if model != self.settings.earth_model {
                    self.settings.earth_model = model;
                    if earth {
                        self.motion.fly = None;
                        self.measure.paths.end_move();
                        self.measure.drag = None;
                    }
                }
                json!({"model": model.code()})
            }
            WorldCommand::Body(p) => {
                let body = match p.body.as_str() {
                    "earth" => Body::Earth,
                    "moon" => Body::Moon,
                    "custom" => Body::Custom,
                    _ => return Err("body must be earth, moon or custom".into()),
                };
                self.flat_earth.open = false;
                self.explorer.choose(body, &self.files.jobs);
                if body == Body::Custom {
                    self.settings.workspace.open_right(PaneTab::Explorer);
                }
                self.touch();
                json!({"body": p.body})
            }
            WorldCommand::Explorer => {
                self.settings.workspace.toggle_right(PaneTab::Explorer);
                json!({})
            }
        })
    }
}
