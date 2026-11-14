//! Camera and place search commands (`camera.*`, `search.*`).
use super::{bool_param, f64_param, position, str_param, toggled};
use crate::app::App;
use serde_json::Value;
use serde_json::json;

/// Camera altitude of the Global View.
const GLOBAL_VIEW_ALTITUDE_KM: f64 = 14_000.0;
/// Zoom In/Out buttons move the camera by this factor.
const ZOOM_STEP_FACTOR: f64 = 0.5;

impl App {
    /// Runs a `camera.*` or `search.*` command.
    pub(super) fn run_camera(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match id {
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
                self.motion.fly = None;
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
                self.motion.fly = None;
                self.zoom(ZOOM_STEP_FACTOR);
                self.touch();
                self.camera_json()
            }
            "camera.zoomOut" => {
                self.motion.fly = None;
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
                self.motion.fly = None;
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
                self.motion.auto_rotate = toggled(p, self.motion.auto_rotate);
                json!({"auto_rotate": self.motion.auto_rotate})
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }

    pub(super) fn camera_json(&self) -> Value {
        json!({"lat": self.cam.lat, "lon": self.cam.lon, "altitude_km": self.cam.alt,
            "heading_deg": self.cam.heading, "tilt_deg": self.cam.tilt,
            "max_tilt_deg": self.cam.max_tilt()})
    }
}
