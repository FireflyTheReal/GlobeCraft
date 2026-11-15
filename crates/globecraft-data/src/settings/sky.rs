use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkyPreferences {
    /// The Milky Way background (NASA's map behind the globe).
    pub enabled: bool,
    /// No longer offered (2026-10-27): the Milky Way shows whenever it is on. Still read
    /// and written so files keep the value; nothing uses it.
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
        super::lenient::apply(self, value, &[]);
    }
}
