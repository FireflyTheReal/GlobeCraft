//! The command catalog. Every user action in GlobeCraft is a command with a stable id and JSON
//! parameters: ribbon buttons, menus, keyboard shortcuts, the command palette and the AI
//! assistant all run the same commands. This module only describes them; frontends execute.

/// What a command touches beyond the globe view, which decides whether the assistant must ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Changes only the view, layers or in-memory documents.
    View,
    /// Reads or writes files on this computer.
    Files,
    /// Discards work or closes the application.
    Destructive,
    /// Opens interface surfaces; meaningless without a person at the window.
    Interface,
}

/// Ribbon tab a command belongs to (its home in the interface).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Layers,
    Live,
    View,
    Places,
    Settings,
    Help,
}

impl Tab {
    pub const ALL: [Self; 7] = [
        Self::Home,
        Self::Layers,
        Self::Live,
        Self::View,
        Self::Places,
        Self::Settings,
        Self::Help,
    ];
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Home => "ribbon-tab-home",
            Self::Layers => "ribbon-tab-layers",
            Self::Live => "ribbon-tab-live",
            Self::View => "ribbon-tab-view",
            Self::Places => "ribbon-tab-places",
            Self::Settings => "ribbon-tab-settings",
            Self::Help => "ribbon-tab-help",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Layers => "layers",
            Self::Live => "live",
            Self::View => "view",
            Self::Places => "places",
            Self::Settings => "settings",
            Self::Help => "help",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.code() == code)
    }
}

/// One command: its id, Fluent label and help keys, icon, parameters (a JSON Schema object,
/// as text) and reach.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub icon: &'static str,
    pub tab: Tab,
    pub params: &'static str,
    pub reach: Reach,
}

impl CommandSpec {
    /// Whether the assistant may run this command (after the user's approval for [`Reach::Files`]
    /// and [`Reach::Destructive`]).
    pub fn assistant_may_run(&self) -> bool {
        self.reach != Reach::Interface
    }
    /// Why the assistant must ask before running this command, in English for the model.
    pub fn approval_reason(&self) -> Option<&'static str> {
        match self.reach {
            Reach::Files => Some("it reads or writes files on this computer"),
            Reach::Destructive => Some("it discards work or closes GlobeCraft"),
            Reach::View | Reach::Interface => None,
        }
    }
}

const NO_PARAMS: &str = r#"{"type":"object","properties":{}}"#;
const TOGGLE: &str =
    r#"{"type":"object","properties":{"on":{"type":"boolean","description":"Omit to toggle"}}}"#;

macro_rules! command {
    ($id:literal, $key:literal, $icon:literal, $tab:ident, $reach:ident, $params:expr) => {
        CommandSpec {
            id: $id,
            label: concat!("cmd-", $key),
            help: concat!("cmd-", $key, "-help"),
            icon: $icon,
            tab: Tab::$tab,
            params: $params,
            reach: Reach::$reach,
        }
    };
}

/// Every command, in ribbon order within each tab.
pub const COMMANDS: &[CommandSpec] = &[
    // Camera and navigation.
    command!(
        "camera.flyTo",
        "camera-fly-to",
        "fly",
        Home,
        View,
        r#"{"type":"object","properties":{"lat":{"type":"number","description":"Latitude in degrees, -90..90"},"lon":{"type":"number","description":"Longitude in degrees, -180..180"},"altitude_km":{"type":"number","description":"Camera altitude above the surface in km (0.15..40000); default keeps the current altitude"},"heading_deg":{"type":"number","description":"View heading clockwise from north"},"tilt_deg":{"type":"number","description":"View tilt from straight down, 0..80; default keeps the current tilt"}},"required":["lat","lon"]}"#
    ),
    command!(
        "camera.set",
        "camera-set",
        "goto_xy",
        Home,
        View,
        r#"{"type":"object","properties":{"lat":{"type":"number"},"lon":{"type":"number"},"altitude_km":{"type":"number"},"heading_deg":{"type":"number"},"tilt_deg":{"type":"number","description":"0 (straight down) .. 80"}}}"#
    ),
    command!(
        "search.place",
        "search-place",
        "search",
        Home,
        View,
        r#"{"type":"object","properties":{"query":{"type":"string","description":"A place name, an address, or coordinates such as 48.85, 2.35"},"fly":{"type":"boolean","description":"Fly to the best result when it arrives (default true)"}},"required":["query"]}"#
    ),
    command!(
        "camera.zoomIn",
        "camera-zoom-in",
        "zoom_in",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "camera.zoomOut",
        "camera-zoom-out",
        "zoom_out",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "camera.resetNorth",
        "camera-reset-north",
        "north",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "camera.tilt",
        "camera-tilt",
        "terrain",
        Home,
        View,
        r#"{"type":"object","properties":{"tilt_deg":{"type":"number","description":"Tilt from straight down, 0..80 (less close to the ground)"},"step":{"type":"number","description":"Degrees to add to the current tilt"}}}"#
    ),
    command!(
        "camera.topDown",
        "camera-top-down",
        "globe_view",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "camera.globalView",
        "camera-global-view",
        "zoom_world",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "camera.autoRotate",
        "camera-auto-rotate",
        "rotate",
        Home,
        View,
        TOGGLE
    ),
    // Places.
    command!(
        "places.addAtCenter",
        "places-add-center",
        "pin_add",
        Home,
        View,
        r#"{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"}}}"#
    ),
    command!(
        "places.add",
        "places-add",
        "pin_add",
        Places,
        View,
        r#"{"type":"object","properties":{"lat":{"type":"number"},"lon":{"type":"number"},"name":{"type":"string"},"description":{"type":"string"},"folder":{"type":"string","description":"Name of an existing folder to put it in"}},"required":["lat","lon"]}"#
    ),
    command!(
        "places.addFolder",
        "places-add-folder",
        "add_folder",
        Places,
        View,
        r#"{"type":"object","properties":{"name":{"type":"string"}}}"#
    ),
    command!(
        "places.flyTo",
        "places-fly-to",
        "fly",
        Places,
        View,
        r#"{"type":"object","properties":{"name":{"type":"string","description":"Placemark name (case-insensitive)"}},"required":["name"]}"#
    ),
    command!(
        "places.remove",
        "places-remove",
        "delete",
        Places,
        View,
        r#"{"type":"object","properties":{"name":{"type":"string","description":"Placemark or folder name"}},"required":["name"]}"#
    ),
    command!(
        "places.open",
        "places-open",
        "open",
        Places,
        Files,
        NO_PARAMS
    ),
    command!(
        "places.save",
        "places-save",
        "save",
        Places,
        Files,
        NO_PARAMS
    ),
    command!(
        "places.saveAs",
        "places-save-as",
        "save_as",
        Places,
        Files,
        NO_PARAMS
    ),
    command!(
        "places.clear",
        "places-clear",
        "clear_all",
        Places,
        Destructive,
        NO_PARAMS
    ),
    // Measuring.
    command!(
        "ruler.enable",
        "ruler-enable",
        "measure",
        Home,
        View,
        TOGGLE
    ),
    command!(
        "ruler.addPoint",
        "ruler-add-point",
        "measure",
        Home,
        View,
        r#"{"type":"object","properties":{"lat":{"type":"number"},"lon":{"type":"number"},"new_path":{"type":"boolean","description":"Start a separate path"}},"required":["lat","lon"]}"#
    ),
    command!(
        "ruler.clear",
        "ruler-clear",
        "clear_sel",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "ruler.closeShape",
        "ruler-close-shape",
        "shape",
        Home,
        View,
        TOGGLE
    ),
    command!(
        "ruler.profile",
        "ruler-profile",
        "measure",
        Home,
        View,
        NO_PARAMS
    ),
    command!(
        "ruler.unit",
        "ruler-unit",
        "measure",
        Home,
        View,
        r#"{"type":"object","properties":{"unit":{"type":"string","enum":["automatic","centimeters","meters","kilometers","inches","feet","yards","miles","nautical_miles","smoots","degrees","arcseconds","football_fields"]}},"required":["unit"]}"#
    ),
    command!(
        "ruler.editPoints",
        "ruler-edit-points",
        "vertices",
        Home,
        Interface,
        NO_PARAMS
    ),
    command!("edit.undo", "edit-undo", "undo", Home, View, NO_PARAMS),
    command!("edit.redo", "edit-redo", "redo", Home, View, NO_PARAMS),
    command!(
        "file.screenshot",
        "file-screenshot",
        "camera",
        Home,
        Files,
        r#"{"type":"object","properties":{"quality":{"type":"string","enum":["normal","plus","pro","pro_max"],"description":"normal: screen size once every tile has loaded; plus: twice the size, one zoom level deeper; pro: four times the size, two levels deeper; pro_max: eight times the size, three levels deeper. Default: the chosen quality"}}}"#
    ),
    command!(
        "file.screenshotQuality",
        "file-screenshot-quality",
        "camera",
        Home,
        Files,
        r#"{"type":"object","properties":{"quality":{"type":"string","enum":["normal","plus","pro","pro_max"]}},"required":["quality"]}"#
    ),
    command!(
        "file.cancelScreenshot",
        "file-cancel-screenshot",
        "close",
        Home,
        Files,
        NO_PARAMS
    ),
    // Data the user adds.
    command!(
        "ui.addData",
        "ui-add-data",
        "add_data",
        Layers,
        Interface,
        NO_PARAMS
    ),
    command!(
        "data.add",
        "data-add",
        "web_data",
        Layers,
        View,
        r#"{"type":"object","properties":{"preset":{"type":"string","enum":["gebco","emodnet_bathymetry","gibs_true_colour","forest_loss","earthquakes"],"description":"A library source: GEBCO seafloor, EMODnet European bathymetry, NASA GIBS yesterday's true colour, Global Forest Watch tree cover loss, USGS earthquakes of the past week"},"url":{"type":"string","description":"Instead of a preset: a web address. For tiles a template with {z}, {x} and {y}; for a WMS its service address; for features a GeoJSON, GPX or CSV file"},"kind":{"type":"string","enum":["tiles","wms","features"]},"layers":{"type":"string","description":"WMS layer names, comma-separated"},"format":{"type":"string","enum":["geojson","gpx","csv"]},"refresh_minutes":{"type":"integer","description":"Reload a feature URL this often; 0 loads it once"},"size_by":{"type":"string","description":"For features: the property points are sized and ranked by (for example mag or population); left out, the usual ones are found"},"max_zoom":{"type":"integer"},"name":{"type":"string"},"attribution":{"type":"string"}}}"#
    ),
    command!(
        "data.search",
        "data-search",
        "search",
        Layers,
        View,
        r#"{"type":"object","properties":{"query":{"type":"string","description":"Words to look for in the library's names, descriptions and categories (ocean, earth, hazards); empty lists the whole library"}}}"#
    ),
    command!(
        "data.show",
        "data-show",
        "eye",
        Layers,
        View,
        r#"{"type":"object","properties":{"id":{"type":"integer","description":"A user layer's id (info.view lists them)"},"visible":{"type":"boolean","description":"Omit to toggle"}},"required":["id"]}"#
    ),
    command!(
        "data.opacity",
        "data-opacity",
        "transparency",
        Layers,
        View,
        r#"{"type":"object","properties":{"id":{"type":"integer"},"opacity":{"type":"number","description":"0 (invisible) to 1 (opaque)"}},"required":["id","opacity"]}"#
    ),
    command!(
        "data.move",
        "data-move",
        "up",
        Layers,
        View,
        r#"{"type":"object","properties":{"id":{"type":"integer"},"direction":{"type":"string","enum":["up","down"],"description":"Up draws it over the next layer"}},"required":["id"]}"#
    ),
    command!(
        "data.zoomTo",
        "data-zoom-to",
        "zoom_full",
        Layers,
        View,
        r#"{"type":"object","properties":{"id":{"type":"integer","description":"A feature layer's id"}},"required":["id"]}"#
    ),
    command!(
        "data.remove",
        "data-remove",
        "delete",
        Layers,
        View,
        r#"{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}"#
    ),
    // Layers.
    command!(
        "layer.basemap",
        "layer-basemap",
        "basemap",
        Layers,
        View,
        r#"{"type":"object","properties":{"basemap":{"type":"string","enum":["satellite","wayback","streets","topo"]}},"required":["basemap"]}"#
    ),
    command!(
        "layer.waybackRelease",
        "layer-wayback-release",
        "history",
        Layers,
        View,
        r#"{"type":"object","properties":{"date":{"type":"string","description":"Release date as listed, e.g. 2021-06-30"},"step":{"type":"integer","description":"+1 for an older, -1 for a newer release"}}}"#
    ),
    command!(
        "layer.waybackCompare",
        "layer-wayback-compare",
        "history",
        Layers,
        View,
        r#"{"type":"object","properties":{"enabled":{"type":"boolean","description":"Omit to toggle"},"date":{"type":"string","description":"Release shown right of the split, as listed"},"left_date":{"type":"string","description":"Release shown left of the split, as listed"},"swap":{"type":"boolean","description":"Exchange the two sides' releases"},"step":{"type":"integer","description":"+1 for an older, -1 for a newer comparison release"},"split":{"type":"number","description":"Where the split runs, 0 (left edge) .. 1 (right edge)"}}}"#
    ),
    command!(
        "layer.show",
        "layer-show",
        "eye",
        Layers,
        View,
        r#"{"type":"object","properties":{"layer":{"type":"string","enum":["country_borders","state_borders","place_labels","municipal_borders","terrain","grid","grid_labels","time_zones","atmosphere","sunlight","stars","milky_way","cinematic_sky","radar","clouds","aurora","flights","vessels","satellites","roads","traffic","night_lights","railways","ferries","airports","points_of_interest","peaks","waters","parks"]},"visible":{"type":"boolean","description":"Omit to toggle"}},"required":["layer"]}"#
    ),
    command!(
        "layer.terrainExaggeration",
        "layer-terrain-exaggeration",
        "terrain",
        Layers,
        View,
        r#"{"type":"object","properties":{"value":{"type":"number","description":"Vertical exaggeration, 0.5..3"}},"required":["value"]}"#
    ),
    command!(
        "layer.flatLiveObjects",
        "layer-flat-live-objects",
        "satellite",
        Layers,
        View,
        r#"{"type":"object","properties":{"on":{"type":"boolean","description":"true: aircraft and satellites drawn on the ground; false: at their height. Omit to toggle"}}}"#
    ),
    command!(
        "layer.stateAltitude",
        "layer-state-altitude",
        "scale_range",
        Layers,
        View,
        r#"{"type":"object","properties":{"km":{"type":"number","description":"States and provinces are drawn below this camera altitude, 0..8000 km (default 1609)"}},"required":["km"]}"#
    ),
    command!(
        "world.earthModel",
        "world-earth-model",
        "globe_view",
        Layers,
        View,
        r#"{"type":"object","properties":{"model":{"type":"string","enum":["wgs84_g2296","perfect_sphere"]}},"required":["model"]}"#
    ),
    // Worlds and the explorer.
    command!(
        "world.body",
        "world-body",
        "planet",
        View,
        View,
        r#"{"type":"object","properties":{"body":{"type":"string","enum":["earth","moon","custom"]}},"required":["body"]}"#
    ),
    command!(
        "ui.explorer",
        "ui-explorer",
        "explore",
        View,
        Interface,
        NO_PARAMS
    ),
    // Panes and interface.
    command!(
        "ui.pane.layers",
        "ui-pane-layers",
        "contents",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.pane.places",
        "ui-pane-places",
        "places",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.pane.data",
        "ui-pane-data",
        "catalog",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.pane.explore",
        "ui-pane-explore",
        "compass",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.pane.assistant",
        "ui-pane-assistant",
        "assistant",
        View,
        Interface,
        r#"{"type":"object","properties":{"ask":{"type":"string"}}}"#
    ),
    command!(
        "ui.pane.measure",
        "ui-pane-measure",
        "measure",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.leftPane",
        "ui-left-pane",
        "pane",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.navigationControls",
        "ui-navigation-controls",
        "nav_controls",
        View,
        View,
        TOGGLE
    ),
    command!(
        "ui.legend",
        "ui-legend",
        "symbology",
        View,
        Interface,
        TOGGLE
    ),
    command!(
        "ui.timeline",
        "ui-timeline",
        "time_slider",
        View,
        Interface,
        TOGGLE
    ),
    command!(
        "ui.here",
        "ui-here",
        "identify",
        Home,
        Interface,
        r#"{"type":"object","properties":{"lat":{"type":"number"},"lon":{"type":"number"}},"required":["lat","lon"]}"#
    ),
    command!(
        "flight.select",
        "flight-select",
        "plane",
        Live,
        View,
        r#"{"type":"object","properties":{"hex":{"type":"string","description":"The plane's transponder (ICAO 24-bit) address, 6 hex digits"},"callsign":{"type":"string","description":"Its callsign, for its route; left out, the one it reports"}},"required":["hex"]}"#
    ),
    command!(
        "satellite.select",
        "satellite-select",
        "satellite",
        Live,
        View,
        r#"{"type":"object","properties":{"norad":{"type":"string","description":"The satellite's NORAD catalogue number, e.g. 25544 for the ISS"}},"required":["norad"]}"#
    ),
    command!(
        "satellite.clear",
        "satellite-clear",
        "close",
        Live,
        View,
        NO_PARAMS
    ),
    command!(
        "flight.clear",
        "flight-clear",
        "close",
        Live,
        View,
        NO_PARAMS
    ),
    command!(
        "time.set",
        "time-set",
        "history",
        View,
        View,
        r#"{"type":"object","properties":{"time":{"type":["string","number"],"description":"now (back to the real time), an ISO 8601 date or date and time (UTC unless a zone is given), or seconds since 1970"}},"required":["time"]}"#
    ),
    command!(
        "time.step",
        "time-step",
        "history",
        View,
        View,
        r#"{"type":"object","properties":{"hours":{"type":"number","description":"Hours to move the app's time, negative to go back"}},"required":["hours"]}"#
    ),
    command!(
        "time.play",
        "time-play",
        "run",
        View,
        View,
        r#"{"type":"object","properties":{"hours_per_second":{"type":"number","description":"How fast the time runs; left out, the timeline's speed"}}}"#
    ),
    command!(
        "time.pause",
        "time-pause",
        "time_pause",
        View,
        View,
        NO_PARAMS
    ),
    command!(
        "time.speed",
        "time-speed",
        "history",
        View,
        View,
        r#"{"type":"object","properties":{"hours_per_second":{"type":"number","description":"How fast the time runs when it plays; a playing time takes it at once"}},"required":["hours_per_second"]}"#
    ),
    command!(
        "time.window",
        "time-window",
        "history",
        View,
        View,
        r#"{"type":"object","properties":{"hours":{"type":"number","description":"Timed points (earthquakes…) show when they happened within this many hours before the app's time; 0 shows all"}},"required":["hours"]}"#
    ),
    command!(
        "ui.tileDiagnostics",
        "ui-tile-diagnostics",
        "stats",
        View,
        View,
        TOGGLE
    ),
    command!(
        "ui.navigatorStyle",
        "ui-navigator-style",
        "nav_controls",
        View,
        View,
        r#"{"type":"object","properties":{"style":{"type":"string","enum":["globe","classic"],"description":"globe: mini globe in the rotation ring; classic: ring, move pad and zoom slider"}},"required":["style"]}"#
    ),
    command!("ui.ribbon", "ui-ribbon", "up", View, Interface, NO_PARAMS),
    command!(
        "ui.theme",
        "ui-theme",
        "dark",
        View,
        View,
        r#"{"type":"object","properties":{"theme":{"type":"string","enum":["system","dark","light"]}},"required":["theme"]}"#
    ),
    command!(
        "ui.fullscreen",
        "ui-fullscreen",
        "fullscreen",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.minimize",
        "ui-minimize",
        "minimize",
        View,
        Interface,
        NO_PARAMS
    ),
    command!(
        "ui.commandSearch",
        "ui-command-search",
        "command",
        View,
        Interface,
        NO_PARAMS
    ),
    // Settings.
    command!(
        "ui.settings",
        "ui-settings",
        "settings",
        Settings,
        Interface,
        NO_PARAMS
    ),
    command!(
        "settings.units",
        "settings-units",
        "ruler_units",
        Settings,
        View,
        r#"{"type":"object","properties":{"units":{"type":"string","enum":["system","metric","imperial"]}},"required":["units"]}"#
    ),
    command!(
        "settings.experience",
        "settings-experience",
        "settings",
        Settings,
        View,
        r#"{"type":"object","properties":{"level":{"type":"string","enum":["basic","advanced"]}},"required":["level"]}"#
    ),
    command!(
        "settings.coordinateFormat",
        "settings-coordinate-format",
        "goto_xy",
        Settings,
        View,
        r#"{"type":"object","properties":{"format":{"type":"string","enum":["dd","dms","ddm","utm_ups","mgrs"]}},"required":["format"]}"#
    ),
    command!(
        "settings.language",
        "settings-language",
        "globe_lang",
        Settings,
        View,
        r#"{"type":"object","properties":{"language":{"type":"string","enum":["system","en","pt-BR"]}},"required":["language"]}"#
    ),
    command!(
        "style.line",
        "style-line",
        "symbology",
        Settings,
        View,
        r##"{"type":"object","properties":{"line":{"type":"string","enum":["country","state","municipal","disputed","major_road","minor_road","roadworks","railway","service_rail","subway","tram","ferry","runway","taxiway","river","canal","park","time_zone"],"description":"Borders (country, state/province, municipal, disputed), roads (motorway to secondary, smaller roads, roadworks), transport (railways, yards and sidings, subways, trams, ferries, runways, taxiways), rivers, canals, park outlines or time zone lines"},"color":{"type":"string","description":"Hex colour #rrggbb or #rrggbbaa (aa is opacity)"},"width":{"type":"number","description":"Line width in points, 0.5..6"},"dash":{"type":"string","enum":["solid","dashed","dotted"]}},"required":["line"]}"##
    ),
    command!(
        "style.labelSize",
        "style-label-size",
        "label_properties",
        Settings,
        View,
        r#"{"type":"object","properties":{"label":{"type":"string","enum":["country","state","city","road","time_zone"]},"size":{"type":"number","description":"Font size in points before the map text scale, 6..24"}},"required":["label","size"]}"#
    ),
    command!(
        "style.reset",
        "style-reset",
        "symbology",
        Settings,
        View,
        NO_PARAMS
    ),
    // Help.
    command!(
        "ui.shortcuts",
        "ui-shortcuts",
        "keyboard",
        Help,
        Interface,
        NO_PARAMS
    ),
    command!(
        "help.documentation",
        "help-documentation",
        "help",
        Help,
        Interface,
        NO_PARAMS
    ),
    command!(
        "help.reportIssue",
        "help-report-issue",
        "bug",
        Help,
        Interface,
        NO_PARAMS
    ),
    command!("ui.about", "ui-about", "logo", Help, Interface, NO_PARAMS),
    command!(
        "app.quit",
        "app-quit",
        "close",
        Help,
        Destructive,
        NO_PARAMS
    ),
    // Read-only views for the assistant.
    command!("info.view", "info-view", "identify", Home, View, NO_PARAMS),
    command!(
        "info.places",
        "info-places",
        "places",
        Places,
        View,
        NO_PARAMS
    ),
    command!("info.ruler", "info-ruler", "measure", Home, View, NO_PARAMS),
    command!(
        "info.here",
        "info-here",
        "identify",
        Home,
        View,
        r#"{"type":"object","properties":{"lat":{"type":"number"},"lon":{"type":"number"}},"required":["lat","lon"],"description":"What is known about a point: coordinates in every format, ground height, country, state and municipality (null with a reason when their border tiles are not loaded), nearest named place, local times and the sun's height"}"#
    ),
];

/// The command with this id.
pub fn find(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_params_are_json_schema_objects() {
        let mut ids: Vec<_> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let total = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate command id");
        for spec in COMMANDS {
            let schema: serde_json::Value =
                serde_json::from_str(spec.params).unwrap_or_else(|e| panic!("{}: {e}", spec.id));
            assert_eq!(schema["type"], "object", "{}", spec.id);
            assert!(schema["properties"].is_object(), "{}", spec.id);
        }
    }

    #[test]
    fn labels_and_help_exist_in_every_language() {
        for spec in COMMANDS {
            for lang in crate::language::Lang::ALL {
                assert_ne!(lang.text(spec.label), spec.label, "{}", spec.label);
                assert_ne!(lang.text(spec.help), spec.help, "{}", spec.help);
            }
        }
        for tab in Tab::ALL {
            assert_eq!(Tab::from_code(tab.code()), Some(tab));
            assert_ne!(
                crate::language::Lang::En.text(tab.label_key()),
                tab.label_key()
            );
        }
    }

    #[test]
    fn layer_enum_matches_layer_codes() {
        let spec = find("layer.show").unwrap();
        let schema: serde_json::Value = serde_json::from_str(spec.params).unwrap();
        let listed: Vec<&str> = schema["properties"]["layer"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let codes: Vec<&str> = crate::layers::MapLayer::ALL
            .iter()
            .map(|l| l.code())
            .collect();
        assert_eq!(listed, codes);
    }

    #[test]
    fn interface_commands_are_closed_to_the_assistant() {
        for spec in COMMANDS {
            if spec.reach == Reach::Interface {
                assert!(!spec.assistant_may_run(), "{}", spec.id);
            }
        }
        assert!(find("file.screenshot").unwrap().approval_reason().is_some());
        assert!(find("camera.flyTo").unwrap().approval_reason().is_none());
    }
}
