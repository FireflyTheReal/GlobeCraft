//! How much of the interface shows (roadmap 2.7). Basic keeps a first start calm:
//! it hides the Earth model and Worlds ribbon groups and the Add Data forms for web
//! services and feature URLs (the source library, KML and dropped files stay).
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceLevel {
    /// New installations start here.
    #[default]
    Basic,
    /// Everything; settings written before the level existed load as this, so an
    /// update never hides what someone already used.
    Advanced,
}

impl ExperienceLevel {
    pub const ALL: [Self; 2] = [Self::Basic, Self::Advanced];
    pub fn code(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Advanced => "advanced",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.code() == code)
    }
    /// The interface text naming the level.
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Basic => "settings-experience-basic",
            Self::Advanced => "settings-experience-advanced",
        }
    }
    pub fn is_advanced(self) -> bool {
        self == Self::Advanced
    }
}
