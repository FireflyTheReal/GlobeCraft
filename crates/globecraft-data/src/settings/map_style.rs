//! How map lines and names look: colour, width and dash per line type, and the label size
//! per kind. Every value has a command (`style.*`), so the Settings window, Command Search
//! and the assistant change the same fields.
use serde::{Serialize, Serializer};
use serde_json::Value;

pub const MIN_LINE_WIDTH_POINTS: f32 = 0.5;
pub const MAX_LINE_WIDTH_POINTS: f32 = 6.0;
pub const MIN_LABEL_SIZE_POINTS: f32 = 6.0;
pub const MAX_LABEL_SIZE_POINTS: f32 = 24.0;

/// The kinds of border line a user can style. Municipal state borders, which replace the
/// state lines up close, use the state style, so the handover keeps one look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineType {
    Country,
    State,
    Municipal,
}
impl LineType {
    pub const ALL: [Self; 3] = [Self::Country, Self::State, Self::Municipal];
    pub fn code(self) -> &'static str {
        match self {
            Self::Country => "country",
            Self::State => "state",
            Self::Municipal => "municipal",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// The kinds of map name a user can size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelType {
    Country,
    State,
    City,
}
impl LabelType {
    pub const ALL: [Self; 3] = [Self::Country, Self::State, Self::City];
    pub fn code(self) -> &'static str {
        match self {
            Self::Country => "country",
            Self::State => "state",
            Self::City => "city",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DashStyle {
    Solid,
    Dashed,
    Dotted,
}
impl DashStyle {
    pub const ALL: [Self; 3] = [Self::Solid, Self::Dashed, Self::Dotted];
    pub fn code(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
            Self::Dotted => "dotted",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|dash| dash.code() == code)
    }
}

/// An sRGB colour with straight (unmultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);
impl Rgba {
    /// `#rrggbb` or `#rrggbbaa` (the `#` is optional); six digits mean opaque.
    pub fn parse_hex(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        let alpha = if hex.len() == 8 { byte(6)? } else { u8::MAX };
        Some(Self([byte(0)?, byte(2)?, byte(4)?, alpha]))
    }
    pub fn hex(self) -> String {
        let [r, g, b, a] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}
impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LineStyle {
    pub color: Rgba,
    pub width_points: f32,
    pub dash: DashStyle,
}
impl LineStyle {
    fn apply(&mut self, value: &Value) {
        if let Some(color) = value.get("color").and_then(Value::as_str).and_then(Rgba::parse_hex) {
            self.color = color;
        }
        if let Some(width) = value.get("width_points").and_then(Value::as_f64) {
            self.width_points = clamp_width(width);
        }
        if let Some(dash) = value.get("dash").and_then(Value::as_str).and_then(DashStyle::from_code) {
            self.dash = dash;
        }
    }
}

pub fn clamp_width(width: f64) -> f32 {
    (width as f32).clamp(MIN_LINE_WIDTH_POINTS, MAX_LINE_WIDTH_POINTS)
}
pub fn clamp_label_size(size: f64) -> f32 {
    (size as f32).clamp(MIN_LABEL_SIZE_POINTS, MAX_LABEL_SIZE_POINTS)
}

/// Label font sizes in points, before the map text scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LabelSizes {
    pub country_points: f32,
    pub state_points: f32,
    pub city_points: f32,
}
impl LabelSizes {
    pub fn get(self, kind: LabelType) -> f32 {
        match kind {
            LabelType::Country => self.country_points,
            LabelType::State => self.state_points,
            LabelType::City => self.city_points,
        }
    }
    pub fn set(&mut self, kind: LabelType, size: f32) {
        *match kind {
            LabelType::Country => &mut self.country_points,
            LabelType::State => &mut self.state_points,
            LabelType::City => &mut self.city_points,
        } = size;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MapStyle {
    pub country: LineStyle,
    pub state: LineStyle,
    pub municipal: LineStyle,
    pub labels: LabelSizes,
}
/// States and municipalities share one look by default, so the handover between them
/// reads as one linework.
const SUBNATIONAL_LINE: LineStyle = LineStyle {
    color: Rgba([150, 205, 240, 225]),
    width_points: 1.0,
    dash: DashStyle::Dashed,
};
impl Default for MapStyle {
    fn default() -> Self {
        Self {
            country: LineStyle {
                color: Rgba([255, 255, 255, 245]),
                width_points: 1.6,
                dash: DashStyle::Solid,
            },
            state: SUBNATIONAL_LINE,
            municipal: SUBNATIONAL_LINE,
            labels: LabelSizes {
                country_points: 12.0,
                state_points: 10.0,
                city_points: 9.0,
            },
        }
    }
}
impl MapStyle {
    pub fn line(&self, kind: LineType) -> &LineStyle {
        match kind {
            LineType::Country => &self.country,
            LineType::State => &self.state,
            LineType::Municipal => &self.municipal,
        }
    }
    pub fn line_mut(&mut self, kind: LineType) -> &mut LineStyle {
        match kind {
            LineType::Country => &mut self.country,
            LineType::State => &mut self.state,
            LineType::Municipal => &mut self.municipal,
        }
    }
    pub(super) fn apply(&mut self, value: &Value) {
        for kind in LineType::ALL {
            if let Some(line) = value.get(kind.code()) {
                self.line_mut(kind).apply(line);
            }
        }
        if let Some(labels) = value.get("labels") {
            for kind in LabelType::ALL {
                let field = format!("{}_points", kind.code());
                if let Some(size) = labels.get(field.as_str()).and_then(Value::as_f64) {
                    self.labels.set(kind, clamp_label_size(size));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_with_and_without_alpha() {
        assert_eq!(Rgba::parse_hex("#ff8000"), Some(Rgba([255, 128, 0, 255])));
        assert_eq!(Rgba::parse_hex("0a0b0c80"), Some(Rgba([10, 11, 12, 128])));
        assert_eq!(Rgba::parse_hex("#ff80"), None);
        assert_eq!(Rgba::parse_hex("#gg8000"), None);
        assert_eq!(Rgba([1, 2, 3, 4]).hex(), "#01020304");
    }

    #[test]
    fn style_round_trips_and_ignores_bad_fields() {
        let mut style = MapStyle::default();
        style.state.color = Rgba([200, 10, 10, 255]);
        style.municipal.dash = DashStyle::Dotted;
        style.country.width_points = 3.0;
        style.labels.city_points = 14.0;
        let json = serde_json::to_value(style).unwrap();
        let mut restored = MapStyle::default();
        restored.apply(&json);
        assert_eq!(restored, style);

        let mut tolerant = MapStyle::default();
        tolerant.apply(&serde_json::json!({
            "country": {"color": "red", "width_points": 99, "dash": "wavy"},
            "labels": {"state_points": 1, "city_points": "big"},
        }));
        assert_eq!(tolerant.country.color, MapStyle::default().country.color);
        assert_eq!(tolerant.country.width_points, MAX_LINE_WIDTH_POINTS);
        assert_eq!(tolerant.country.dash, DashStyle::Solid);
        assert_eq!(tolerant.labels.state_points, MIN_LABEL_SIZE_POINTS);
        assert_eq!(tolerant.labels.city_points, 9.0);
    }
}
