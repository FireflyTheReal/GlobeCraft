//! Commands as values: [`Command::parse`] turns a command id and its JSON parameters
//! into a [`Command`], which a frontend matches on. Commands are grouped by the part of
//! GlobeCraft they act on; a frontend runs each group in one place, and a `match` on a
//! group must name every command in it, so a command added here without a handler does
//! not compile.
//!
//! The table at the end is the only place that pairs an id with its variant.
use super::params::*;
use serde_json::Value;

/// A command with its parameters, ready to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Camera(CameraCommand),
    Places(PlacesCommand),
    Ruler(RulerCommand),
    Files(FileCommand),
    Layers(LayerCommand),
    Worlds(WorldCommand),
    Data(DataCommand),
    Time(TimeCommand),
    Flights(FlightCommand),
    Satellites(SatelliteCommand),
    Legend(LegendCommand),
    Settings(SettingsCommand),
    Info(InfoCommand),
    Interface(InterfaceCommand),
}

/// The camera and place search: `camera.*`, `search.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraCommand {
    FlyTo(FlyTo),
    Set(CameraSet),
    SearchPlace(SearchPlace),
    ClearSearch,
    ZoomIn,
    ZoomOut,
    ResetNorth,
    Tilt(Tilt),
    LookAround(LookAround),
    TopDown,
    GlobalView,
    AutoRotate(Switch),
}

/// The user's places: `places.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacesCommand {
    AddAtCenter(PlaceText),
    Add(AddPlace),
    AddFolder(AddFolder),
    FlyTo(PlaceName),
    Remove(PlaceName),
    Open,
    Save,
    SaveAs,
    Export(ExportPlaces),
    Clear,
}

/// The ruler and its undo history: `ruler.*`, `edit.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum RulerCommand {
    Enable(Switch),
    AddPoint(AddPoint),
    Clear,
    CloseShape(Switch),
    Profile,
    Unit(SetRulerUnit),
    EditPoints,
    Undo,
    Redo,
}

/// Screenshots, view links and settings files: `file.*`, `view.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum FileCommand {
    CopyLink,
    OpenLink(OpenLink),
    Screenshot(Screenshot),
    ScreenshotQuality(SetScreenshotQuality),
    CancelScreenshot,
    ExportSettings,
    ImportSettings,
}

/// The built-in map layers and the basemap: `layer.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerCommand {
    Basemap(SetBasemap),
    WaybackRelease(WaybackRelease),
    WaybackCompare(WaybackCompare),
    Show(ShowLayer),
    Opacity(LayerOpacity),
    Solo(SoloLayer),
    TerrainExaggeration(Amount),
    FlatLiveObjects(Switch),
    StateAltitude(StateAltitude),
}

/// The world shown: `world.*` and the world explorer.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldCommand {
    EarthModel(SetEarthModel),
    Body(SetBody),
    Explorer,
}

/// Layers the user adds: `data.*` and the Add Data window.
#[derive(Clone, Debug, PartialEq)]
pub enum DataCommand {
    OpenAddData,
    Add(AddData),
    Search(DataSearch),
    Show(ShowData),
    Opacity(DataOpacity),
    Move(MoveData),
    ZoomTo(DataId),
    Remove(DataId),
}

/// The scene's time: `time.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum TimeCommand {
    Set(SetTime),
    Step(Hours),
    Play(Play),
    Pause,
    Speed(SetSpeed),
    Window(Hours),
}

/// The selected aircraft: `flight.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum FlightCommand {
    Select(SelectFlight),
    Clear,
}

/// The selected satellite: `satellite.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum SatelliteCommand {
    Select(SelectSatellite),
    Clear,
}

/// The map legend.
#[derive(Clone, Debug, PartialEq)]
pub enum LegendCommand {
    Show(Switch),
}

/// Preferences and the map style: `settings.*`, `style.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsCommand {
    Units(SetUnits),
    Experience(SetExperience),
    CoordinateFormat(SetCoordinateFormat),
    Language(SetLanguage),
    Line(LineStyle),
    LabelSize(LabelSize),
    ResetStyle,
}

/// What the assistant can ask about: `info.*` and the place card.
#[derive(Clone, Debug, PartialEq)]
pub enum InfoCommand {
    View,
    Places,
    Ruler,
    Here(Position),
    ShowHere(Position),
}

/// The window, panes and the application: `ui.*`, `assistant.*`, `help.*`, `app.*`.
#[derive(Clone, Debug, PartialEq)]
pub enum InterfaceCommand {
    LayersPane,
    PlacesPane,
    AssistantPane(AskAssistant),
    StopAssistant,
    MeasurePane,
    LeftPane,
    NavigationControls(Switch),
    Timeline(Switch),
    TileDiagnostics(Switch),
    NavigatorStyle(SetNavigatorStyle),
    Ribbon,
    FocusNext,
    FocusPrevious,
    PinRibbonTool(PinRibbonTool),
    PinQuickTool(PinQuickTool),
    EditShortcut(ToolName),
    SetShortcut(SetShortcut),
    ResetRibbonTools,
    Theme(SetTheme),
    Fullscreen,
    Minimize,
    CommandSearch,
    Settings,
    Shortcuts,
    Documentation,
    ReportIssue,
    CopyDiagnostics,
    About,
    WorkOffline(Switch),
    Quit,
}

/// Pairs each command id with its variant: `"id" => Group(GroupEnum::Variant)`, with the
/// parameter type in parentheses for a command that takes parameters.
macro_rules! command_table {
    ($( $id:literal => $group:ident ( $kind:ident :: $variant:ident $( ( $params:ty ) )? ), )*) => {
        impl Command {
            /// Every command id the table knows, in its order.
            pub const IDS: &'static [&'static str] = &[$($id),*];

            /// The command `id` with its `params` read, or why they cannot be.
            pub fn parse(id: &str, params: &Value) -> Result<Self, String> {
                Ok(match id {
                    $( $id => Self::$group($kind::$variant $( (<$params as Params>::read(params)?) )?), )*
                    _ => return Err(format!("unknown command '{id}'")),
                })
            }

            /// The command's id.
            pub fn id(&self) -> &'static str {
                match self {
                    $( Self::$group($kind::$variant $( (command_table!(@any $params)) )?) => $id, )*
                }
            }

            /// The parameters command `id` reads, with whether each is required.
            pub fn fields(id: &str) -> Option<&'static [(&'static str, bool)]> {
                match id {
                    $( $id => Some(command_table!(@fields $($params)?)), )*
                    _ => None,
                }
            }
        }
    };
    (@any $params:ty) => { _ };
    (@fields) => { &[] };
    (@fields $params:ty) => { <$params as Params>::FIELDS };
}

command_table! {
    "camera.flyTo" => Camera(CameraCommand::FlyTo(FlyTo)),
    "camera.set" => Camera(CameraCommand::Set(CameraSet)),
    "search.place" => Camera(CameraCommand::SearchPlace(SearchPlace)),
    "search.clear" => Camera(CameraCommand::ClearSearch),
    "camera.zoomIn" => Camera(CameraCommand::ZoomIn),
    "camera.zoomOut" => Camera(CameraCommand::ZoomOut),
    "camera.resetNorth" => Camera(CameraCommand::ResetNorth),
    "camera.tilt" => Camera(CameraCommand::Tilt(Tilt)),
    "camera.lookAround" => Camera(CameraCommand::LookAround(LookAround)),
    "camera.topDown" => Camera(CameraCommand::TopDown),
    "camera.globalView" => Camera(CameraCommand::GlobalView),
    "camera.autoRotate" => Camera(CameraCommand::AutoRotate(Switch)),
    "places.addAtCenter" => Places(PlacesCommand::AddAtCenter(PlaceText)),
    "places.add" => Places(PlacesCommand::Add(AddPlace)),
    "places.addFolder" => Places(PlacesCommand::AddFolder(AddFolder)),
    "places.flyTo" => Places(PlacesCommand::FlyTo(PlaceName)),
    "places.remove" => Places(PlacesCommand::Remove(PlaceName)),
    "places.open" => Places(PlacesCommand::Open),
    "places.save" => Places(PlacesCommand::Save),
    "places.saveAs" => Places(PlacesCommand::SaveAs),
    "places.export" => Places(PlacesCommand::Export(ExportPlaces)),
    "places.clear" => Places(PlacesCommand::Clear),
    "ruler.enable" => Ruler(RulerCommand::Enable(Switch)),
    "ruler.addPoint" => Ruler(RulerCommand::AddPoint(AddPoint)),
    "ruler.clear" => Ruler(RulerCommand::Clear),
    "ruler.closeShape" => Ruler(RulerCommand::CloseShape(Switch)),
    "ruler.profile" => Ruler(RulerCommand::Profile),
    "ruler.unit" => Ruler(RulerCommand::Unit(SetRulerUnit)),
    "ruler.editPoints" => Ruler(RulerCommand::EditPoints),
    "edit.undo" => Ruler(RulerCommand::Undo),
    "edit.redo" => Ruler(RulerCommand::Redo),
    "view.copyLink" => Files(FileCommand::CopyLink),
    "view.openLink" => Files(FileCommand::OpenLink(OpenLink)),
    "file.screenshot" => Files(FileCommand::Screenshot(Screenshot)),
    "file.screenshotQuality" => Files(FileCommand::ScreenshotQuality(SetScreenshotQuality)),
    "file.cancelScreenshot" => Files(FileCommand::CancelScreenshot),
    "file.exportSettings" => Files(FileCommand::ExportSettings),
    "file.importSettings" => Files(FileCommand::ImportSettings),
    "ui.addData" => Data(DataCommand::OpenAddData),
    "data.add" => Data(DataCommand::Add(AddData)),
    "data.search" => Data(DataCommand::Search(DataSearch)),
    "data.show" => Data(DataCommand::Show(ShowData)),
    "data.opacity" => Data(DataCommand::Opacity(DataOpacity)),
    "data.move" => Data(DataCommand::Move(MoveData)),
    "data.zoomTo" => Data(DataCommand::ZoomTo(DataId)),
    "data.remove" => Data(DataCommand::Remove(DataId)),
    "layer.basemap" => Layers(LayerCommand::Basemap(SetBasemap)),
    "layer.waybackRelease" => Layers(LayerCommand::WaybackRelease(WaybackRelease)),
    "layer.waybackCompare" => Layers(LayerCommand::WaybackCompare(WaybackCompare)),
    "layer.show" => Layers(LayerCommand::Show(ShowLayer)),
    "layer.opacity" => Layers(LayerCommand::Opacity(LayerOpacity)),
    "layer.solo" => Layers(LayerCommand::Solo(SoloLayer)),
    "layer.terrainExaggeration" => Layers(LayerCommand::TerrainExaggeration(Amount)),
    "layer.flatLiveObjects" => Layers(LayerCommand::FlatLiveObjects(Switch)),
    "layer.stateAltitude" => Layers(LayerCommand::StateAltitude(StateAltitude)),
    "world.earthModel" => Worlds(WorldCommand::EarthModel(SetEarthModel)),
    "world.body" => Worlds(WorldCommand::Body(SetBody)),
    "ui.explorer" => Worlds(WorldCommand::Explorer),
    "ui.pane.layers" => Interface(InterfaceCommand::LayersPane),
    "ui.pane.places" => Interface(InterfaceCommand::PlacesPane),
    "ui.pane.assistant" => Interface(InterfaceCommand::AssistantPane(AskAssistant)),
    "assistant.stop" => Interface(InterfaceCommand::StopAssistant),
    "ui.pane.measure" => Interface(InterfaceCommand::MeasurePane),
    "ui.leftPane" => Interface(InterfaceCommand::LeftPane),
    "ui.navigationControls" => Interface(InterfaceCommand::NavigationControls(Switch)),
    "ui.legend" => Legend(LegendCommand::Show(Switch)),
    "ui.timeline" => Interface(InterfaceCommand::Timeline(Switch)),
    "ui.here" => Info(InfoCommand::ShowHere(Position)),
    "flight.select" => Flights(FlightCommand::Select(SelectFlight)),
    "satellite.select" => Satellites(SatelliteCommand::Select(SelectSatellite)),
    "satellite.clear" => Satellites(SatelliteCommand::Clear),
    "flight.clear" => Flights(FlightCommand::Clear),
    "time.set" => Time(TimeCommand::Set(SetTime)),
    "time.step" => Time(TimeCommand::Step(Hours)),
    "time.play" => Time(TimeCommand::Play(Play)),
    "time.pause" => Time(TimeCommand::Pause),
    "time.speed" => Time(TimeCommand::Speed(SetSpeed)),
    "time.window" => Time(TimeCommand::Window(Hours)),
    "ui.tileDiagnostics" => Interface(InterfaceCommand::TileDiagnostics(Switch)),
    "ui.navigatorStyle" => Interface(InterfaceCommand::NavigatorStyle(SetNavigatorStyle)),
    "ui.ribbon" => Interface(InterfaceCommand::Ribbon),
    "ui.focusNext" => Interface(InterfaceCommand::FocusNext),
    "ui.focusPrevious" => Interface(InterfaceCommand::FocusPrevious),
    "ui.ribbon.pin" => Interface(InterfaceCommand::PinRibbonTool(PinRibbonTool)),
    "ui.quickTools.pin" => Interface(InterfaceCommand::PinQuickTool(PinQuickTool)),
    "ui.shortcut.edit" => Interface(InterfaceCommand::EditShortcut(ToolName)),
    "ui.shortcut.set" => Interface(InterfaceCommand::SetShortcut(SetShortcut)),
    "ui.ribbon.resetTools" => Interface(InterfaceCommand::ResetRibbonTools),
    "ui.theme" => Interface(InterfaceCommand::Theme(SetTheme)),
    "ui.fullscreen" => Interface(InterfaceCommand::Fullscreen),
    "ui.minimize" => Interface(InterfaceCommand::Minimize),
    "ui.commandSearch" => Interface(InterfaceCommand::CommandSearch),
    "ui.settings" => Interface(InterfaceCommand::Settings),
    "settings.units" => Settings(SettingsCommand::Units(SetUnits)),
    "settings.experience" => Settings(SettingsCommand::Experience(SetExperience)),
    "settings.coordinateFormat" => Settings(SettingsCommand::CoordinateFormat(SetCoordinateFormat)),
    "settings.language" => Settings(SettingsCommand::Language(SetLanguage)),
    "style.line" => Settings(SettingsCommand::Line(LineStyle)),
    "style.labelSize" => Settings(SettingsCommand::LabelSize(LabelSize)),
    "style.reset" => Settings(SettingsCommand::ResetStyle),
    "ui.shortcuts" => Interface(InterfaceCommand::Shortcuts),
    "help.documentation" => Interface(InterfaceCommand::Documentation),
    "help.reportIssue" => Interface(InterfaceCommand::ReportIssue),
    "help.copyDiagnostics" => Interface(InterfaceCommand::CopyDiagnostics),
    "ui.about" => Interface(InterfaceCommand::About),
    "app.workOffline" => Interface(InterfaceCommand::WorkOffline(Switch)),
    "app.quit" => Interface(InterfaceCommand::Quit),
    "info.view" => Info(InfoCommand::View),
    "info.places" => Info(InfoCommand::Places),
    "info.ruler" => Info(InfoCommand::Ruler),
    "info.here" => Info(InfoCommand::Here(Position)),
}

#[cfg(test)]
mod tests {
    use super::super::COMMANDS;
    use super::*;
    use serde_json::json;

    /// The catalog and the table name the same commands, each once.
    #[test]
    fn the_table_has_every_catalog_command_once() {
        let catalog: Vec<&str> = COMMANDS.iter().map(|spec| spec.id).collect();
        let mut table = Command::IDS.to_vec();
        table.sort_unstable();
        let unique = table.len();
        table.dedup();
        assert_eq!(unique, table.len(), "a command is in the table twice");
        let mut sorted = catalog.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, table);
    }

    /// Each command's parameter struct has exactly the parameters its schema (what the
    /// assistant reads) describes, and requires only parameters the schema requires.
    /// It may accept more than the schema promises: the ribbon's Search button runs
    /// `search.place` without a query, which the assistant must give.
    #[test]
    fn parameter_structs_match_their_schemas() {
        for spec in COMMANDS {
            let schema: Value = serde_json::from_str(spec.params).unwrap();
            let required: Vec<&str> = schema["required"]
                .as_array()
                .map(|names| names.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let mut described: Vec<&str> = schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let fields = Command::fields(spec.id).unwrap();
            let mut read: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
            described.sort_unstable();
            read.sort_unstable();
            assert_eq!(read, described, "{}: struct and schema differ", spec.id);
            for (name, needed) in fields {
                assert!(
                    !needed || required.contains(name),
                    "{}: {name} is required, but the schema does not say so",
                    spec.id
                );
            }
        }
    }

    /// Every command reads parameters its schema allows: each property set to a value
    /// of the schema's type (an enum's first value), and only the required ones.
    #[test]
    fn every_command_parses_what_its_schema_describes() {
        fn sample(property: &Value) -> Value {
            if let Some(first) = property["enum"]
                .as_array()
                .and_then(|values| values.first())
            {
                return first.clone();
            }
            match &property["type"] {
                Value::String(kind) => match kind.as_str() {
                    "number" => json!(1.5),
                    "integer" => json!(2),
                    "boolean" => json!(true),
                    _ => json!("text"),
                },
                // `time` takes text or a number.
                _ => json!("now"),
            }
        }
        for spec in COMMANDS {
            let schema: Value = serde_json::from_str(spec.params).unwrap();
            let properties = schema["properties"].as_object().unwrap();
            let all: serde_json::Map<String, Value> = properties
                .iter()
                .map(|(name, property)| (name.clone(), sample(property)))
                .collect();
            let command = Command::parse(spec.id, &Value::Object(all))
                .unwrap_or_else(|error| panic!("{}: {error}", spec.id));
            assert_eq!(command.id(), spec.id);
            let required: serde_json::Map<String, Value> = properties
                .iter()
                .filter(|(name, _)| {
                    schema["required"]
                        .as_array()
                        .is_some_and(|names| names.iter().any(|n| n == *name))
                })
                .map(|(name, property)| (name.clone(), sample(property)))
                .collect();
            let missing_optional = Command::parse(spec.id, &Value::Object(required));
            assert!(missing_optional.is_ok(), "{}", spec.id);
        }
        assert_eq!(
            Command::parse("camera.fly", &json!({})).unwrap_err(),
            "unknown command 'camera.fly'"
        );
        assert_eq!(
            Command::parse("camera.flyTo", &json!({"lat": 1})).unwrap_err(),
            "lon is required"
        );
    }
}
