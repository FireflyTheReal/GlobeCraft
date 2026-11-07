//! How map lines and names look: colour, width and dash per line type, and the label size
//! per kind. Every value has a command (`style.*`), so the Settings window, Command Search
//! and the assistant change the same fields.
use serde::{Serialize, Serializer};
use serde_json::Value;

pub const MIN_LINE_WIDTH_POINTS: f32 = 0.5;
pub const MAX_LINE_WIDTH_POINTS: f32 = 6.0;
pub const MIN_LABEL_SIZE_POINTS: f32 = 6.0;
pub const MAX_LABEL_SIZE_POINTS: f32 = 24.0;

/// The kinds of map line a user can style: borders, roads, transport, rivers and
/// parks. Municipal state borders, which replace the state lines up close, use the
/// state style, so the handover keeps one look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineType {
    Country,
    State,
    Municipal,
    /// Disputed country borders and claim lines (OpenStreetMap, marked by OpenMapTiles).
    Disputed,
    /// Motorways, trunk, primary and secondary roads.
    MajorRoad,
    /// Tertiary, residential and service roads.
    MinorRoad,
    /// Roads under construction.
    Roadworks,
    Railway,
    /// Yards and sidings.
    ServiceRail,
    Subway,
    Tram,
    Ferry,
    Runway,
    Taxiway,
    River,
    Canal,
    /// Outlines of parks and protected areas.
    Park,
    /// Where the standard time changes (the Time zones layer).
    TimeZone,
}

/// How the line types are grouped in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineGroup {
    Borders,
    Roads,
    Transport,
    WaterAndParks,
    TimeZones,
}
impl LineGroup {
    pub const ALL: [Self; 5] = [
        Self::Borders,
        Self::Roads,
        Self::Transport,
        Self::WaterAndParks,
        Self::TimeZones,
    ];
}

impl LineType {
    pub const COUNT: usize = 18;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Country,
        Self::State,
        Self::Municipal,
        Self::Disputed,
        Self::MajorRoad,
        Self::MinorRoad,
        Self::Roadworks,
        Self::Railway,
        Self::ServiceRail,
        Self::Subway,
        Self::Tram,
        Self::Ferry,
        Self::Runway,
        Self::Taxiway,
        Self::River,
        Self::Canal,
        Self::Park,
        Self::TimeZone,
    ];
    pub fn code(self) -> &'static str {
        match self {
            Self::Country => "country",
            Self::State => "state",
            Self::Municipal => "municipal",
            Self::Disputed => "disputed",
            Self::MajorRoad => "major_road",
            Self::MinorRoad => "minor_road",
            Self::Roadworks => "roadworks",
            Self::Railway => "railway",
            Self::ServiceRail => "service_rail",
            Self::Subway => "subway",
            Self::Tram => "tram",
            Self::Ferry => "ferry",
            Self::Runway => "runway",
            Self::Taxiway => "taxiway",
            Self::River => "river",
            Self::Canal => "canal",
            Self::Park => "park",
            Self::TimeZone => "time_zone",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
    pub fn group(self) -> LineGroup {
        match self {
            Self::Country | Self::State | Self::Municipal | Self::Disputed => LineGroup::Borders,
            Self::MajorRoad | Self::MinorRoad | Self::Roadworks => LineGroup::Roads,
            Self::Railway
            | Self::ServiceRail
            | Self::Subway
            | Self::Tram
            | Self::Ferry
            | Self::Runway
            | Self::Taxiway => LineGroup::Transport,
            Self::River | Self::Canal | Self::Park => LineGroup::WaterAndParks,
            Self::TimeZone => LineGroup::TimeZones,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    /// The look a line type starts with (and returns to on reset).
    pub fn default_style(self) -> LineStyle {
        let line = |[r, g, b, a]: [u8; 4], width_points, dash| LineStyle {
            color: Rgba([r, g, b, a]),
            width_points,
            dash,
        };
        match self {
            Self::Country => line([255, 255, 255, 245], 1.6, DashStyle::Solid),
            Self::State | Self::Municipal => SUBNATIONAL_LINE,
            // Like the country lines, dashed, as on Google's map.
            Self::Disputed => line([255, 255, 255, 245], 1.6, DashStyle::Dashed),
            Self::MajorRoad => line([205, 209, 222, 212], 2.5, DashStyle::Solid),
            Self::MinorRoad => line([165, 174, 191, 165], 1.5, DashStyle::Solid),
            Self::Roadworks => line([240, 83, 70, 255], 1.5, DashStyle::Solid),
            Self::Railway => line([58, 58, 66, 255], 2.4, DashStyle::Solid),
            Self::ServiceRail => line([96, 96, 106, 255], 1.0, DashStyle::Solid),
            Self::Subway => line([150, 92, 210, 255], 1.8, DashStyle::Solid),
            Self::Tram => line([214, 112, 56, 255], 1.3, DashStyle::Solid),
            Self::Ferry => line([86, 178, 255, 255], 1.5, DashStyle::Dashed),
            Self::Runway => line([205, 205, 212, 255], 3.5, DashStyle::Solid),
            Self::Taxiway => line([226, 196, 84, 255], 1.0, DashStyle::Solid),
            Self::River => line([78, 150, 235, 255], 1.6, DashStyle::Solid),
            Self::Canal => line([78, 150, 235, 255], 1.1, DashStyle::Solid),
            Self::Park => line([92, 180, 96, 204], 1.2, DashStyle::Dashed),
            // Amber, as on time zone maps; its names take the same colour.
            Self::TimeZone => line([255, 190, 76, 200], 1.2, DashStyle::Solid),
        }
    }
}

/// The kinds of map name a user can size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelType {
    Country,
    State,
    City,
    /// Street and road names, written along the roads.
    Road,
    /// The time zones' offsets (`+1`, `−3:30`).
    TimeZone,
}
impl LabelType {
    pub const ALL: [Self; 5] = [
        Self::Country,
        Self::State,
        Self::City,
        Self::Road,
        Self::TimeZone,
    ];
    pub fn code(self) -> &'static str {
        match self {
            Self::Country => "country",
            Self::State => "state",
            Self::City => "city",
            Self::Road => "road",
            Self::TimeZone => "time_zone",
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
        if let Some(color) = value
            .get("color")
            .and_then(Value::as_str)
            .and_then(Rgba::parse_hex)
        {
            self.color = color;
        }
        if let Some(width) = value.get("width_points").and_then(Value::as_f64) {
            self.width_points = clamp_width(width);
        }
        if let Some(dash) = value
            .get("dash")
            .and_then(Value::as_str)
            .and_then(DashStyle::from_code)
        {
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
    pub road_points: f32,
    pub time_zone_points: f32,
}
impl LabelSizes {
    pub fn get(self, kind: LabelType) -> f32 {
        match kind {
            LabelType::Country => self.country_points,
            LabelType::State => self.state_points,
            LabelType::City => self.city_points,
            LabelType::Road => self.road_points,
            LabelType::TimeZone => self.time_zone_points,
        }
    }
    pub fn set(&mut self, kind: LabelType, size: f32) {
        *match kind {
            LabelType::Country => &mut self.country_points,
            LabelType::State => &mut self.state_points,
            LabelType::City => &mut self.city_points,
            LabelType::Road => &mut self.road_points,
            LabelType::TimeZone => &mut self.time_zone_points,
        } = size;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapStyle {
    /// Indexed by `LineType as usize`.
    lines: [LineStyle; LineType::COUNT],
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
            lines: LineType::ALL.map(LineType::default_style),
            labels: LabelSizes {
                country_points: 12.0,
                state_points: 10.0,
                city_points: 9.0,
                road_points: 9.5,
                time_zone_points: 12.0,
            },
        }
    }
}
/// Saved as `{"country": {...}, "major_road": {...}, ..., "labels": {...}}`, so files
/// from before the road and transport styles still load.
impl Serialize for MapStyle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(LineType::COUNT + 1))?;
        for kind in LineType::ALL {
            map.serialize_entry(kind.code(), self.line(kind))?;
        }
        map.serialize_entry("labels", &self.labels)?;
        map.end()
    }
}
impl MapStyle {
    pub fn line(&self, kind: LineType) -> &LineStyle {
        &self.lines[kind.index()]
    }
    pub fn line_mut(&mut self, kind: LineType) -> &mut LineStyle {
        &mut self.lines[kind.index()]
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
        style.line_mut(LineType::State).color = Rgba([200, 10, 10, 255]);
        style.line_mut(LineType::Municipal).dash = DashStyle::Dotted;
        style.line_mut(LineType::Country).width_points = 3.0;
        style.line_mut(LineType::Ferry).dash = DashStyle::Solid;
        style.line_mut(LineType::Park).color = Rgba([1, 2, 3, 4]);
        style.line_mut(LineType::TimeZone).width_points = 2.5;
        style.labels.city_points = 14.0;
        style.labels.time_zone_points = 16.0;
        let json = serde_json::to_value(style).unwrap();
        let mut restored = MapStyle::default();
        restored.apply(&json);
        assert_eq!(restored, style);

        let mut tolerant = MapStyle::default();
        tolerant.apply(&serde_json::json!({
            "country": {"color": "red", "width_points": 99, "dash": "wavy"},
            "labels": {"state_points": 1, "city_points": "big"},
        }));
        let country = tolerant.line(LineType::Country);
        assert_eq!(
            country.color,
            MapStyle::default().line(LineType::Country).color
        );
        assert_eq!(country.width_points, MAX_LINE_WIDTH_POINTS);
        assert_eq!(country.dash, DashStyle::Solid);
        assert_eq!(tolerant.labels.state_points, MIN_LABEL_SIZE_POINTS);
        assert_eq!(tolerant.labels.city_points, 9.0);
    }

    /// Settings saved before the road and transport styles keep their border styles
    /// and get the defaults for the rest.
    #[test]
    fn old_files_keep_their_border_styles() {
        let mut style = MapStyle::default();
        style.apply(&serde_json::json!({
            "country": {"color": "#ff0000ff", "width_points": 2.0, "dash": "dotted"},
            "labels": {"country_points": 12.0},
        }));
        assert_eq!(style.line(LineType::Country).color, Rgba([255, 0, 0, 255]));
        assert_eq!(
            *style.line(LineType::MajorRoad),
            LineType::MajorRoad.default_style()
        );
    }

    #[test]
    fn every_line_type_has_a_unique_code_and_index() {
        for (index, kind) in LineType::ALL.into_iter().enumerate() {
            assert_eq!(kind as usize, index);
            assert_eq!(LineType::from_code(kind.code()), Some(kind));
        }
    }

    /// The `style.line` command lists every line type, in order.
    #[test]
    fn the_style_command_lists_every_line_type() {
        let spec = globecraft_core::commands::find("style.line").unwrap();
        let schema: Value = serde_json::from_str(spec.params).unwrap();
        let listed: Vec<&str> = schema["properties"]["line"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let codes: Vec<&str> = LineType::ALL.iter().map(|kind| kind.code()).collect();
        assert_eq!(listed, codes);
    }
}
