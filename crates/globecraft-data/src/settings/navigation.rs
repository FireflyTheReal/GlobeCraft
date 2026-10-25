//! Navigation sensitivity and a validated startup camera; no UI dependencies.
use globecraft_core::{
    MAX_CAMERA_ALTITUDE_KM, MIN_CAMERA_ALTITUDE_KM, camera::Cam, coordinates::validate_coordinates,
};
use serde::Serialize;
pub const MIN_SENSITIVITY: f64 = 0.1;
pub const MAX_SENSITIVITY: f64 = 4.0;

/// Which on-globe navigator is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NavigatorStyle {
    /// A mini globe in the rotation ring, and the zoom slider.
    #[default]
    Globe,
    /// The original Google Earth-style ring, move pad and zoom slider.
    Classic,
}
impl NavigatorStyle {
    pub const ALL: [Self; 2] = [Self::Globe, Self::Classic];
    pub fn code(self) -> &'static str {
        match self {
            Self::Globe => "globe",
            Self::Classic => "classic",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.code() == code)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StartupView {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_km: f64,
    pub heading_deg: f64,
}
impl Default for StartupView {
    fn default() -> Self {
        Self {
            latitude: -15.0,
            longitude: -50.0,
            altitude_km: 14_000.0,
            heading_deg: 0.0,
        }
    }
}
impl StartupView {
    pub fn camera(self) -> Cam {
        Cam {
            lat: self.latitude,
            lon: self.longitude,
            alt: self.altitude_km,
            heading: self.heading_deg,
        }
    }
    pub fn from_camera(camera: Cam) -> Self {
        Self {
            latitude: camera.lat,
            longitude: camera.lon,
            altitude_km: camera.alt,
            heading_deg: camera.heading,
        }
    }
    fn apply(&mut self, value: &serde_json::Value) {
        let lat = value
            .get("latitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.latitude);
        let lon = value
            .get("longitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.longitude);
        if let Ok((latitude, longitude)) = validate_coordinates(lat, lon) {
            self.latitude = latitude.clamp(
                -globecraft_core::MAX_CAMERA_LATITUDE_DEG,
                globecraft_core::MAX_CAMERA_LATITUDE_DEG,
            );
            self.longitude = longitude;
        }
        if let Some(alt) = value
            .get("altitude_km")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
        {
            self.altitude_km = alt.clamp(MIN_CAMERA_ALTITUDE_KM, MAX_CAMERA_ALTITUDE_KM);
        }
        if let Some(heading) = value
            .get("heading_deg")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
        {
            self.heading_deg = globecraft_core::math::wrap_lon(heading);
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NavigationPreferences {
    pub pan_sensitivity: f64,
    pub zoom_sensitivity: f64,
    pub rotation_sensitivity: f64,
    pub invert_scroll: bool,
    pub navigator: NavigatorStyle,
    pub startup: StartupView,
}
impl Default for NavigationPreferences {
    fn default() -> Self {
        Self {
            pan_sensitivity: 1.0,
            zoom_sensitivity: 1.0,
            rotation_sensitivity: 1.0,
            invert_scroll: false,
            navigator: NavigatorStyle::default(),
            startup: StartupView::default(),
        }
    }
}
impl NavigationPreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        for (key, target) in [
            ("pan_sensitivity", &mut self.pan_sensitivity),
            ("zoom_sensitivity", &mut self.zoom_sensitivity),
            ("rotation_sensitivity", &mut self.rotation_sensitivity),
        ] {
            if let Some(number) = value
                .get(key)
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite())
            {
                *target = number.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);
            }
        }
        if let Some(invert) = value.get("invert_scroll").and_then(|v| v.as_bool()) {
            self.invert_scroll = invert;
        }
        if let Some(style) = value
            .get("navigator")
            .and_then(|v| v.as_str())
            .and_then(NavigatorStyle::from_code)
        {
            self.navigator = style;
        }
        if let Some(startup) = value.get("startup") {
            self.startup.apply(startup);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_navigator_style_round_trips_and_ignores_unknown_names() {
        let mut prefs = NavigationPreferences {
            navigator: NavigatorStyle::Classic,
            ..NavigationPreferences::default()
        };
        let json = serde_json::to_value(&prefs).unwrap();
        assert_eq!(json["navigator"], "classic");
        let mut restored = NavigationPreferences::default();
        restored.apply(&json);
        assert_eq!(restored, prefs);
        prefs.apply(&serde_json::json!({"navigator": "dial"}));
        assert_eq!(prefs.navigator, NavigatorStyle::Classic);
    }
}
