//! How much detail a screenshot gets.
use serde::Serialize;

/// Screenshot quality (the capture button's drop-down).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotQuality {
    /// The globe as it is on screen.
    #[default]
    Normal,
    /// Screen size, every tile at full detail.
    Plus,
    /// Twice the size: tiles one zoom level deeper.
    Pro,
    /// Four times the size: tiles two zoom levels deeper.
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
    /// Pixels per displayed pixel of a background render, or `None` for the frame on
    /// screen as it is. Each doubling samples imagery one zoom level deeper.
    pub fn render_scale(self) -> Option<usize> {
        match self {
            Self::Normal => None,
            Self::Plus => Some(1),
            Self::Pro => Some(2),
            Self::ProMax => Some(4),
        }
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
        assert_eq!(ScreenshotQuality::ProMax.render_scale(), Some(4));
        assert_eq!(ScreenshotQuality::Normal.render_scale(), None);
    }
}
