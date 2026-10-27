use serde::Serialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SkyPreferences {
    /// The Milky Way background (NASA's map behind the globe).
    pub enabled: bool,
    /// No longer offered (2026-10-27): the Milky Way shows whenever it is on.
    pub cinematic: bool,
    /// The point stars.
    pub stars: bool,
}
impl Default for SkyPreferences {
    fn default() -> Self {
        Self {
            enabled: false,
            cinematic: false,
            stars: true,
        }
    }
}
impl SkyPreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        if let Some(enabled) = value.get("enabled").and_then(|v| v.as_bool()) {
            self.enabled = enabled;
        }
        if let Some(cinematic) = value.get("cinematic").and_then(|v| v.as_bool()) {
            self.cinematic = cinematic;
        }
        if let Some(stars) = value.get("stars").and_then(|v| v.as_bool()) {
            self.stars = stars;
        }
    }
}
