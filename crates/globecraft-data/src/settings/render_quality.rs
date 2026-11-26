//! How sharp the globe renders (Settings › Graphics).
use serde::{Deserialize, Serialize};

/// The globe's resolution at rest; frames while moving are a share of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderQuality {
    /// The screen's own pixels (Retina included).
    #[default]
    Screen,
    /// At most 1080 pixels high.
    Hd,
    /// At most 720 pixels high.
    Sd,
}

impl RenderQuality {
    pub const ALL: [Self; 3] = [Self::Screen, Self::Hd, Self::Sd];
    pub fn code(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Hd => "hd",
            Self::Sd => "sd",
        }
    }
    /// The highest a frame of the globe may be (pixels); `None`: the screen's size.
    /// Fewer pixels mean fewer pixels to draw and coarser tiles to load.
    pub fn max_height_px(self) -> Option<f32> {
        match self {
            Self::Screen => None,
            Self::Hd => Some(1080.0),
            Self::Sd => Some(720.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualities_are_saved_by_code_and_shrink_in_order() {
        for quality in RenderQuality::ALL {
            assert_eq!(
                serde_json::to_value(quality).unwrap(),
                serde_json::Value::from(quality.code())
            );
        }
        assert_eq!(RenderQuality::Screen.max_height_px(), None);
        assert!(RenderQuality::Sd.max_height_px() < RenderQuality::Hd.max_height_px());
    }
}
