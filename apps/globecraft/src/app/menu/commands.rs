//! One command catalog for native menus and the in-window fallback.
use eframe::egui;
use globecraft_core::language::Lang;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Application,
    File,
    Edit,
    View,
    Add,
    Tools,
    Window,
    Help,
}
impl Section {
    pub(super) const ALL: [Self; 8] = [
        Self::Application,
        Self::File,
        Self::Edit,
        Self::View,
        Self::Add,
        Self::Tools,
        Self::Window,
        Self::Help,
    ];
    pub(super) fn label(self, lang: Lang) -> &'static str {
        lang.text(match self {
            Self::Application => "menu-application-title",
            Self::File => "menu-file-title",
            Self::Edit => "menu-edit-title",
            Self::View => "menu-view-title",
            Self::Add => "menu-add-title",
            Self::Tools => "menu-tools-title",
            Self::Window => "menu-window-title",
            Self::Help => "menu-help-title",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Action {
    About,
    Settings,
    Open,
    Save,
    SaveAs,
    Screenshot,
    Quit,
    Undo,
    Redo,
    Sidebar,
    ShowExplore,
    ShowLayers,
    ShowPlaces,
    NavigationControls,
    Grid,
    Atmosphere,
    Sunlight,
    North,
    GlobalView,
    Placemark,
    Folder,
    Ruler,
    Explorer,
    Fullscreen,
    Minimize,
    Help,
    Documentation,
    ReportIssue,
}
impl Action {
    pub(super) const ALL: [Self; 28] = [
        Self::About,
        Self::Settings,
        Self::Open,
        Self::Save,
        Self::SaveAs,
        Self::Screenshot,
        Self::Quit,
        Self::Undo,
        Self::Redo,
        Self::Sidebar,
        Self::ShowExplore,
        Self::ShowLayers,
        Self::ShowPlaces,
        Self::NavigationControls,
        Self::Grid,
        Self::Atmosphere,
        Self::Sunlight,
        Self::North,
        Self::GlobalView,
        Self::Placemark,
        Self::Folder,
        Self::Ruler,
        Self::Explorer,
        Self::Fullscreen,
        Self::Minimize,
        Self::Help,
        Self::Documentation,
        Self::ReportIssue,
    ];
    pub(super) fn section(self) -> Section {
        match self {
            Self::About | Self::Settings | Self::Quit => Section::Application,
            Self::Open | Self::Save | Self::SaveAs | Self::Screenshot => Section::File,
            Self::Undo | Self::Redo => Section::Edit,
            Self::Sidebar
            | Self::ShowExplore
            | Self::ShowLayers
            | Self::ShowPlaces
            | Self::NavigationControls
            | Self::Grid
            | Self::Atmosphere
            | Self::Sunlight
            | Self::North
            | Self::GlobalView => Section::View,
            Self::Placemark | Self::Folder => Section::Add,
            Self::Ruler | Self::Explorer => Section::Tools,
            Self::Fullscreen | Self::Minimize => Section::Window,
            Self::Help | Self::Documentation | Self::ReportIssue => Section::Help,
        }
    }
    pub(super) fn label(self, lang: Lang) -> &'static str {
        lang.text(match self {
            Self::Open => "menu-kml-open",
            Self::Save => "menu-kml-save",
            Self::SaveAs => "menu-kml-save-as",
            Self::Screenshot => "menu-screenshot-save",
            Self::Quit => "menu-quit-action",
            Self::Undo => "edit-undo-action",
            Self::Redo => "edit-redo-action",
            Self::About => "menu-about-action",
            Self::Settings => "menu-settings-action",
            Self::Sidebar => "menu-sidebar-toggle",
            Self::ShowExplore => "sidebar-explore-title",
            Self::ShowLayers => "layers-section-title",
            Self::ShowPlaces => "sidebar-places-title",
            Self::NavigationControls => "menu-navigation-toggle",
            Self::Grid => "layers-grid-toggle",
            Self::Atmosphere => "layers-atmosphere-toggle",
            Self::Sunlight => "layers-sunlight-toggle",
            Self::North => "navigation-reset-north-action",
            Self::GlobalView => "navigation-global-view-action",
            Self::Placemark => "menu-add-placemark",
            Self::Folder => "places-add-folder-action",
            Self::Ruler => "ruler-enable-toggle",
            Self::Explorer => "top-explore-action",
            Self::Fullscreen => "menu-fullscreen-toggle",
            Self::Minimize => "menu-minimize-action",
            Self::Help => "menu-controls-help",
            Self::Documentation => "menu-documentation-action",
            Self::ReportIssue => "menu-report-issue-action",
        })
    }
    /// The command this menu item runs.
    pub(in crate::app) fn command(self) -> (&'static str, serde_json::Value) {
        use serde_json::{Value, json};
        match self {
            Self::About => ("ui.about", Value::Null),
            Self::Settings => ("ui.settings", Value::Null),
            Self::Open => ("places.open", Value::Null),
            Self::Save => ("places.save", Value::Null),
            Self::SaveAs => ("places.saveAs", Value::Null),
            Self::Screenshot => ("file.screenshot", Value::Null),
            Self::Quit => ("app.quit", Value::Null),
            Self::Undo => ("edit.undo", Value::Null),
            Self::Redo => ("edit.redo", Value::Null),
            Self::Sidebar => ("ui.leftPane", Value::Null),
            Self::ShowExplore => ("ui.pane.explore", Value::Null),
            Self::ShowLayers => ("ui.pane.layers", Value::Null),
            Self::ShowPlaces => ("ui.pane.places", Value::Null),
            Self::NavigationControls => ("ui.navigationControls", Value::Null),
            Self::Grid => ("layer.show", json!({"layer": "grid"})),
            Self::Atmosphere => ("layer.show", json!({"layer": "atmosphere"})),
            Self::Sunlight => ("layer.show", json!({"layer": "sunlight"})),
            Self::North => ("camera.resetNorth", Value::Null),
            Self::GlobalView => ("camera.globalView", Value::Null),
            Self::Placemark => ("places.addAtCenter", Value::Null),
            Self::Folder => ("places.addFolder", Value::Null),
            Self::Ruler => ("ruler.enable", Value::Null),
            Self::Explorer => ("ui.explorer", Value::Null),
            Self::Fullscreen => ("ui.fullscreen", Value::Null),
            Self::Minimize => ("ui.minimize", Value::Null),
            Self::Help => ("ui.shortcuts", Value::Null),
            Self::Documentation => ("help.documentation", Value::Null),
            Self::ReportIssue => ("help.reportIssue", Value::Null),
        }
    }
    pub(super) fn shortcut(self) -> Option<egui::KeyboardShortcut> {
        crate::app::commands::shortcut(self.command().0)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct MenuState {
    pub image: bool,
    pub undo: bool,
    pub redo: bool,
    pub sidebar: bool,
    pub navigation: bool,
    pub grid: bool,
    pub atmosphere: bool,
    pub sunlight: bool,
    pub ruler: bool,
    pub explorer: bool,
    pub fullscreen: bool,
}
impl MenuState {
    pub(super) fn enabled(self, action: Action) -> bool {
        match action {
            Action::Screenshot => self.image,
            Action::Undo => self.undo,
            Action::Redo => self.redo,
            _ => true,
        }
    }
    pub(super) fn checked(self, action: Action) -> Option<bool> {
        Some(match action {
            Action::Sidebar => self.sidebar,
            Action::NavigationControls => self.navigation,
            Action::Grid => self.grid,
            Action::Atmosphere => self.atmosphere,
            Action::Sunlight => self.sunlight,
            Action::Ruler => self.ruler,
            Action::Explorer => self.explorer,
            Action::Fullscreen => self.fullscreen,
            _ => return None,
        })
    }
}
