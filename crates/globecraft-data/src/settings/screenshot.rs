//! How much detail a screenshot gets.
use serde::Serialize;

/// Screenshot quality (the capture button's drop-down).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotQuality {
    /// Screen size, as on screen once every tile has loaded.
    #[default]
    Normal,
    /// Twice the size: imagery one zoom level deeper.
    Plus,
    /// Four times the size: two levels deeper.
    Pro,
    /// Eight times the size: three levels deeper, tiles the view never showed.
    ProMax,
}

impl ScreenshotQuality {
    pub const ALL: [Self; 4] = [Self::Normal, Self::Plus, Self::Pro, Self::ProMax];
    pub fn code(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Plus => "plus",
            Self::Pro => "pro",
            Self::ProMax => "pro_max",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|quality| quality.code() == code)
    }
    /// Pixels per displayed pixel. Each doubling samples imagery one zoom level deeper.
    pub fn render_scale(self) -> usize {
        match self {
            Self::Normal => 1,
            Self::Plus => 2,
            Self::Pro => 4,
            Self::ProMax => 8,
        }
    }
    /// Whether imagery fades between levels as on screen. The larger qualities show
    /// one level per pixel, so the picture depends only on the downloaded tiles.
    pub fn level_blend(self) -> bool {
        self == Self::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualities_round_trip_and_scale_by_two() {
        for quality in ScreenshotQuality::ALL {
            assert_eq!(ScreenshotQuality::from_code(quality.code()), Some(quality));
            assert_eq!(
                serde_json::to_value(quality).unwrap(),
                serde_json::Value::from(quality.code())
            );
        }
        assert_eq!(ScreenshotQuality::Normal.render_scale(), 1);
        assert_eq!(ScreenshotQuality::ProMax.render_scale(), 8);
        assert!(ScreenshotQuality::Normal.level_blend());
        assert!(!ScreenshotQuality::Plus.level_blend());
    }
}
