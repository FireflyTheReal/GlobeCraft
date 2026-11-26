//! Camera and place search commands (`camera.*`, `search.*`).
use super::position;
use crate::app::App;
use globecraft_core::commands::CameraCommand;
use serde_json::{Value, json};

/// Camera altitude of the Global View.
const GLOBAL_VIEW_ALTITUDE_KM: f64 = 14_000.0;
/// Zoom In/Out buttons move the camera by this factor.
const ZOOM_STEP_FACTOR: f64 = 0.5;

impl App {
    /// Runs a `camera.*` or `search.*` command.
    pub(super) fn run_camera(&mut self, command: CameraCommand) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match command {
            CameraCommand::FlyTo(p) => {
                let (lat, lon) = position(p.lat, p.lon)?;
                let alt = p.altitude_km.unwrap_or(self.cam.alt).clamp(
                    globecraft_core::MIN_CAMERA_ALTITUDE_KM,
                    globecraft_core::MAX_CAMERA_ALTITUDE_KM,
                );
                self.fly_to_with_orientation(lat, lon, alt, p.heading_deg, p.tilt_deg);
                json!({"flying_to": {"lat": lat, "lon": lon, "altitude_km": alt}})
            }
            CameraCommand::Set(p) => {
                self.motion.fly = None;
                let lat = p.lat.unwrap_or(self.cam.lat);
                let lon = p.lon.unwrap_or(self.cam.lon);
                globecraft_core::coordinates::validate_coordinates(lat, lon)
                    .map_err(|e| e.to_string())?;
                self.cam.lat = lat.clamp(
                    -globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                    globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                );
                self.cam.lon = lon;
                if let Some(alt) = p.altitude_km {
                    self.cam.alt = alt.clamp(
                        globecraft_core::MIN_CAMERA_ALTITUDE_KM,
                        globecraft_core::MAX_CAMERA_ALTITUDE_KM,
                    );
                }
                if let Some(heading) = p.heading_deg {
                    self.cam.heading = globecraft_core::math::wrap_lon(heading);
                }
                self.cam.clamp_tilt();
                if let Some(tilt) = p.tilt_deg {
                    // A tilt given leans about the ground point, without a look up.
                    self.cam.look_up = 0.0;
                    // A given altitude is kept (the tilt yields); otherwise the tilt
                    // wins and the camera backs off as far as it needs.
                    if p.altitude_km.is_some() {
                        self.cam.set_tilt(tilt);
                    } else {
                        self.cam.tilt_zooming_out(tilt);
                    }
                }
                self.touch();
                self.camera_json()
            }
            CameraCommand::ClearSearch => {
                self.search.results.clear();
                self.search.query.clear();
                self.search.suggestions_open = false;
                json!({})
            }
            CameraCommand::SearchPlace(p) => match p.query {
                Some(query) if !query.is_empty() => {
                    self.search.query = query;
                    self.search.suggestions_open = false;
                    self.search.fly = p.fly.unwrap_or(true);
                    self.start_search(&ctx);
                    json!({"status": self.status, "note": "Results arrive in a moment; describe_view lists them."})
                }
                // Without a query (the ribbon button), the search box takes the focus.
                _ => {
                    self.search.focus = true;
                    json!({})
                }
            },
            CameraCommand::ZoomIn => {
                self.motion.fly = None;
                self.zoom(ZOOM_STEP_FACTOR);
                self.touch();
                self.camera_json()
            }
            CameraCommand::ZoomOut => {
                self.motion.fly = None;
                self.zoom(1.0 / ZOOM_STEP_FACTOR);
                self.touch();
                self.camera_json()
            }
            CameraCommand::ResetNorth => {
                self.cam.heading = 0.0;
                self.touch();
                self.camera_json()
            }
            CameraCommand::Tilt(p) => {
                self.motion.fly = None;
                let tilt = match (p.tilt_deg, p.step) {
                    (Some(tilt), _) => tilt,
                    (None, Some(step)) => self.cam.view_tilt() + step,
                    (None, None) => return Err("give tilt_deg or step".into()),
                };
                self.cam.tilt_view(tilt);
                self.touch();
                self.camera_json()
            }
            CameraCommand::LookAround(p) if p.turn_deg.is_none() && p.raise_deg.is_none() => {
                self.motion.look_mode = !self.motion.look_mode;
                json!({"look_mode": self.motion.look_mode})
            }
            CameraCommand::LookAround(p) => {
                let (turn, raise) = (p.turn_deg.unwrap_or(0.0), p.raise_deg.unwrap_or(0.0));
                self.motion.fly = None;
                if !self.look_around(turn, raise) {
                    return Err("the view cannot turn that way from here".into());
                }
                self.touch();
                self.camera_json()
            }
            CameraCommand::TopDown => {
                (self.cam.tilt, self.cam.look_up) = (0.0, 0.0);
                self.touch();
                self.camera_json()
            }
            CameraCommand::GlobalView => {
                self.fly_to(self.cam.lat, self.cam.lon, GLOBAL_VIEW_ALTITUDE_KM);
                self.camera_json()
            }
            CameraCommand::AutoRotate(p) => {
                self.motion.auto_rotate = p.toggled(self.motion.auto_rotate);
                json!({"auto_rotate": self.motion.auto_rotate})
            }
        })
    }

    pub(super) fn camera_json(&self) -> Value {
        json!({"lat": self.cam.lat, "lon": self.cam.lon, "altitude_km": self.cam.alt,
            "heading_deg": self.cam.heading, "tilt_deg": self.cam.tilt,
            "max_tilt_deg": self.cam.max_tilt(), "look_up_deg": self.cam.look_up})
    }
}
