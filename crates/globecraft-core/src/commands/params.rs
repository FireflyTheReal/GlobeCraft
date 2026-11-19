//! Typed command parameters. A command's parameters arrive as JSON (from the assistant,
//! a widget, a menu or a view link); each command reads them into its own struct once,
//! before it runs, so a handler names its parameters as fields the compiler checks.
//!
//! Reading is lenient where a person or a model is likely to be loose: a number may
//! come as text (`"12.5"`), text is trimmed, and a switch or a whole number of the
//! wrong type counts as not given. A missing required parameter, or a number that is
//! not one, is an error naming the parameter.
//!
//! The catalog's JSON Schema text (what the assistant reads) stays hand-written; a test
//! checks that each struct has exactly the schema's parameters, required where the
//! schema requires them.
use serde_json::Value;

/// A command's parameters, read from JSON.
pub trait Params: Sized {
    /// Each parameter's name and whether it must be given, in declaration order.
    const FIELDS: &'static [(&'static str, bool)];
    fn read(params: &Value) -> Result<Self, String>;
}

/// One parameter: a value of type `T` that must be given, or an `Option<T>` that may not.
pub trait Field: Sized {
    const REQUIRED: bool;
    fn read(value: Option<&Value>, key: &str) -> Result<Self, String>;
}

/// A value a parameter can hold: `None` when it is absent or null (or, for the types
/// that say so, of another type).
pub trait Param: Sized {
    fn read(value: &Value, key: &str) -> Result<Option<Self>, String>;
}

impl<T: Param> Field for Option<T> {
    const REQUIRED: bool = false;
    fn read(value: Option<&Value>, key: &str) -> Result<Self, String> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => T::read(value, key),
        }
    }
}

macro_rules! required_fields {
    ($($ty:ty),*) => {$(
        impl Field for $ty {
            const REQUIRED: bool = true;
            fn read(value: Option<&Value>, key: &str) -> Result<Self, String> {
                <Option<$ty> as Field>::read(value, key)?
                    .ok_or_else(|| format!("{key} is required"))
            }
        }
    )*};
}
required_fields!(f64, bool, String, u64, i64, TimeValue, Identifier);

/// A finite number, also written as text.
impl Param for f64 {
    fn read(value: &Value, key: &str) -> Result<Option<Self>, String> {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
            .filter(|number| number.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{key} must be a number"))
    }
}

/// `true` or `false`; anything else counts as not given.
impl Param for bool {
    fn read(value: &Value, _key: &str) -> Result<Option<Self>, String> {
        Ok(value.as_bool())
    }
}

/// Text, trimmed; anything else counts as not given.
impl Param for String {
    fn read(value: &Value, _key: &str) -> Result<Option<Self>, String> {
        Ok(value.as_str().map(|text| text.trim().to_owned()))
    }
}

/// A whole number from 0; anything else counts as not given.
impl Param for u64 {
    fn read(value: &Value, _key: &str) -> Result<Option<Self>, String> {
        Ok(value.as_u64())
    }
}

/// A whole number; anything else counts as not given.
impl Param for i64 {
    fn read(value: &Value, _key: &str) -> Result<Option<Self>, String> {
        Ok(value.as_i64())
    }
}

/// An identifier such as a catalogue number: text, or a whole number from 0 (kept as
/// its digits); anything else counts as not given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identifier(pub String);

impl Param for Identifier {
    fn read(value: &Value, _key: &str) -> Result<Option<Self>, String> {
        Ok(value
            .as_str()
            .map(|text| text.trim().to_owned())
            .or_else(|| value.as_u64().map(|number| number.to_string()))
            .map(Self))
    }
}

/// A moment as given: text (`now` or a date) or a number (seconds since 1970).
#[derive(Clone, Debug, PartialEq)]
pub enum TimeValue {
    Text(String),
    Number(f64),
}

impl Param for TimeValue {
    fn read(value: &Value, key: &str) -> Result<Option<Self>, String> {
        match value {
            Value::String(text) => Ok(Some(Self::Text(text.trim().to_owned()))),
            Value::Number(number) => number
                .as_f64()
                .map(|n| Some(Self::Number(n)))
                .ok_or_else(|| format!("{key} must be a number")),
            _ => Err(format!(
                "give a {key}: now, an ISO 8601 date or seconds since 1970"
            )),
        }
    }
}

/// Declares a parameter struct: each field is read from the JSON key of its name.
macro_rules! params {
    ($(
        $(#[$meta:meta])*
        pub struct $name:ident { $( $(#[$field_meta:meta])* pub $field:ident : $ty:ty ),* $(,)? }
    )*) => {$(
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name { $( $(#[$field_meta])* pub $field: $ty ),* }

        impl Params for $name {
            const FIELDS: &'static [(&'static str, bool)] =
                &[$( (stringify!($field), <$ty as Field>::REQUIRED) ),*];
            fn read(params: &Value) -> Result<Self, String> {
                Ok(Self {
                    $( $field: Field::read(params.get(stringify!($field)), stringify!($field))? ),*
                })
            }
        }
    )*};
}

params! {
    /// A switch: `on` when given, else the opposite of what it is.
    pub struct Switch {
        pub on: Option<bool>,
    }

    /// A point on the globe, in degrees.
    pub struct Position {
        pub lat: f64,
        pub lon: f64,
    }

    /// `camera.flyTo`
    pub struct FlyTo {
        pub lat: f64,
        pub lon: f64,
        pub altitude_km: Option<f64>,
        pub heading_deg: Option<f64>,
        pub tilt_deg: Option<f64>,
    }

    /// `camera.set`
    pub struct CameraSet {
        pub lat: Option<f64>,
        pub lon: Option<f64>,
        pub altitude_km: Option<f64>,
        pub heading_deg: Option<f64>,
        pub tilt_deg: Option<f64>,
    }

    /// `camera.tilt`
    pub struct Tilt {
        pub tilt_deg: Option<f64>,
        pub step: Option<f64>,
    }

    /// `camera.lookAround`
    pub struct LookAround {
        pub turn_deg: Option<f64>,
        pub raise_deg: Option<f64>,
    }

    /// `search.place`
    pub struct SearchPlace {
        /// Without one (the ribbon's button), the search box takes the focus.
        pub query: Option<String>,
        pub fly: Option<bool>,
    }

    /// `places.addAtCenter`
    pub struct PlaceText {
        pub name: Option<String>,
        pub description: Option<String>,
    }

    /// `places.add`
    pub struct AddPlace {
        pub lat: f64,
        pub lon: f64,
        pub name: Option<String>,
        pub description: Option<String>,
        pub folder: Option<String>,
    }

    /// `places.addFolder`
    pub struct AddFolder {
        pub name: Option<String>,
    }

    /// `places.flyTo` and `places.remove`
    pub struct PlaceName {
        pub name: String,
    }

    /// `places.export`
    pub struct ExportPlaces {
        pub path: Option<String>,
        pub format: Option<String>,
    }

    /// `ruler.addPoint`
    pub struct AddPoint {
        pub lat: f64,
        pub lon: f64,
        pub new_path: Option<bool>,
    }

    /// `ruler.unit`
    pub struct SetRulerUnit {
        pub unit: String,
    }

    /// `view.openLink`
    pub struct OpenLink {
        pub link: String,
    }

    /// `file.screenshot`
    pub struct Screenshot {
        pub quality: Option<String>,
    }

    /// `file.screenshotQuality`
    pub struct SetScreenshotQuality {
        pub quality: String,
    }

    /// `data.add`
    pub struct AddData {
        pub preset: Option<String>,
        pub url: Option<String>,
        pub kind: Option<String>,
        pub layers: Option<String>,
        pub format: Option<String>,
        pub refresh_minutes: Option<u64>,
        pub size_by: Option<String>,
        pub max_zoom: Option<u64>,
        pub name: Option<String>,
        pub attribution: Option<String>,
    }

    /// `data.search`
    pub struct DataSearch {
        pub query: Option<String>,
    }

    /// `data.show`
    pub struct ShowData {
        pub id: u64,
        pub visible: Option<bool>,
    }

    /// `data.opacity`
    pub struct DataOpacity {
        pub id: u64,
        pub opacity: f64,
    }

    /// `data.move`
    pub struct MoveData {
        pub id: u64,
        pub direction: Option<String>,
    }

    /// `data.zoomTo` and `data.remove`
    pub struct DataId {
        pub id: u64,
    }

    /// `layer.basemap`
    pub struct SetBasemap {
        pub basemap: String,
    }

    /// `layer.waybackRelease`
    pub struct WaybackRelease {
        pub date: Option<String>,
        pub step: Option<i64>,
    }

    /// `layer.waybackCompare`
    pub struct WaybackCompare {
        pub enabled: Option<bool>,
        pub date: Option<String>,
        pub left_date: Option<String>,
        pub swap: Option<bool>,
        pub step: Option<i64>,
        pub split: Option<f64>,
    }

    /// `layer.show`
    pub struct ShowLayer {
        pub layer: String,
        pub visible: Option<bool>,
    }

    /// `layer.opacity`
    pub struct LayerOpacity {
        pub layer: String,
        pub opacity: f64,
    }

    /// `layer.solo`
    pub struct SoloLayer {
        pub layer: Option<String>,
    }

    /// `layer.terrainExaggeration`
    pub struct Amount {
        pub value: f64,
    }

    /// `layer.stateAltitude`
    pub struct StateAltitude {
        pub km: f64,
    }

    /// `world.earthModel`
    pub struct SetEarthModel {
        pub model: String,
    }

    /// `world.body`
    pub struct SetBody {
        pub body: String,
    }

    /// `ui.pane.assistant`
    pub struct AskAssistant {
        pub ask: Option<String>,
    }

    /// `flight.select`
    pub struct SelectFlight {
        pub hex: String,
        pub callsign: Option<String>,
    }

    /// `satellite.select`
    pub struct SelectSatellite {
        pub norad: Identifier,
    }

    /// `time.set`
    pub struct SetTime {
        pub time: TimeValue,
    }

    /// `time.step` and `time.window`
    pub struct Hours {
        pub hours: f64,
    }

    /// `time.range`
    pub struct Days {
        pub days: f64,
    }

    /// `time.play`
    pub struct Play {
        pub hours_per_second: Option<f64>,
    }

    /// `time.speed`
    pub struct SetSpeed {
        pub hours_per_second: f64,
    }

    /// `ui.pane.side`
    pub struct SetPaneSide {
        pub side: Option<String>,
    }

    /// `ui.pane.moveTab`
    pub struct MovePaneTab {
        pub tab: String,
        pub to: u64,
    }

    /// `ui.navigatorStyle`
    pub struct SetNavigatorStyle {
        pub style: String,
    }

    /// `ui.ribbon.pin`
    pub struct PinRibbonTool {
        pub group: String,
        pub tool: String,
        pub on: Option<bool>,
    }

    /// `ui.quickTools.pin`
    pub struct PinQuickTool {
        pub tool: String,
        pub on: Option<bool>,
    }

    /// `ui.shortcut.edit`
    pub struct ToolName {
        pub tool: String,
    }

    /// `ui.shortcut.set`
    pub struct SetShortcut {
        pub tool: String,
        pub keys: Option<String>,
    }

    /// `ui.theme`
    pub struct SetTheme {
        pub theme: String,
    }

    /// `settings.units`
    pub struct SetUnits {
        pub units: String,
    }

    /// `settings.experience`
    pub struct SetExperience {
        pub level: String,
    }

    /// `settings.coordinateFormat`
    pub struct SetCoordinateFormat {
        pub format: String,
    }

    /// `settings.language`
    pub struct SetLanguage {
        pub language: String,
    }

    /// `style.line`
    pub struct LineStyle {
        pub line: String,
        pub color: Option<String>,
        pub width: Option<f64>,
        pub dash: Option<String>,
    }

    /// `style.labelSize`
    pub struct LabelSize {
        pub label: String,
        pub size: f64,
    }
}

impl Switch {
    /// `on` when given, else the opposite of `current`.
    pub fn toggled(&self, current: bool) -> bool {
        self.on.unwrap_or(!current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parameters_read_leniently_and_name_what_is_wrong() {
        let fly = FlyTo::read(&json!({"lat": "47.5 ", "lon": 9, "tilt_deg": null})).unwrap();
        assert_eq!((fly.lat, fly.lon, fly.tilt_deg), (47.5, 9.0, None));
        assert_eq!(
            FlyTo::read(&json!({"lat": 1})).unwrap_err(),
            "lon is required"
        );
        assert_eq!(
            FlyTo::read(&json!({"lat": "north", "lon": 1})).unwrap_err(),
            "lat must be a number"
        );
        assert_eq!(
            FlyTo::read(&json!({"lat": 1, "lon": 2, "altitude_km": true})).unwrap_err(),
            "altitude_km must be a number"
        );
        // Text is trimmed; a switch or text of another type counts as not given.
        let search = SearchPlace::read(&json!({"query": " Lindau ", "fly": "yes"})).unwrap();
        assert_eq!(
            (search.query.as_deref(), search.fly),
            (Some("Lindau"), None)
        );
        assert_eq!(SearchPlace::read(&json!({"query": 5})).unwrap().query, None);
        assert_eq!(
            PlaceName::read(&json!({"name": 5})).unwrap_err(),
            "name is required"
        );
        // Whole numbers: a negative id or a fraction is not one.
        assert_eq!(DataId::read(&json!({"id": 3})).unwrap().id, 3);
        assert!(DataId::read(&json!({"id": -3})).is_err());
        assert_eq!(
            WaybackRelease::read(&json!({"step": -2})).unwrap().step,
            Some(-2)
        );
        assert_eq!(
            WaybackRelease::read(&json!({"step": 1.5})).unwrap().step,
            None
        );
        // Unknown keys are passed over; the parameters may also be absent altogether.
        assert_eq!(
            Switch::read(&json!({"on": true, "x": 1})).unwrap().on,
            Some(true)
        );
        assert_eq!(Switch::read(&Value::Null).unwrap().on, None);
        assert!(!Switch { on: None }.toggled(true));
        // A catalogue number may come as text or as a number.
        for norad in [json!("25544"), json!(25544)] {
            let read = SelectSatellite::read(&json!({ "norad": norad })).unwrap();
            assert_eq!(read.norad, Identifier("25544".into()));
        }
        assert!(Switch { on: Some(true) }.toggled(true));
    }

    #[test]
    fn a_time_is_text_or_a_number() {
        assert_eq!(
            SetTime::read(&json!({"time": " now "})).unwrap().time,
            TimeValue::Text("now".into())
        );
        assert_eq!(
            SetTime::read(&json!({"time": 1.8e9})).unwrap().time,
            TimeValue::Number(1.8e9)
        );
        assert!(SetTime::read(&json!({"time": [1]})).is_err());
        assert_eq!(SetTime::read(&json!({})).unwrap_err(), "time is required");
    }
}
