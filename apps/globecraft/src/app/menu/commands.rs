//! The menu bar's catalog: which command each item runs, in which menu. Items run the same
//! commands as the ribbon and Command Search, and take their names and states from them.
use eframe::egui;
use globecraft_core::{
    language::Lang,
    layers::{LayerGroup, MapLayer},
};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Application,
    File,
    Edit,
    View,
    Go,
    Map,
    Live,
    Add,
    Tools,
    Window,
    Help,
}
impl Section {
    pub(super) const ALL: [Self; 11] = [
        Self::Application,
        Self::File,
        Self::Edit,
        Self::View,
        Self::Go,
        Self::Map,
        Self::Live,
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
            Self::Go => "menu-go-title",
            Self::Map => "ribbon-tab-map",
            Self::Live => "ribbon-tab-live",
            Self::Add => "menu-add-title",
            Self::Tools => "menu-tools-title",
            Self::Window => "menu-window-title",
            Self::Help => "menu-help-title",
        })
    }
}

/// One menu item: a catalog command, or the switch of one map layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct Action {
    pub(super) section: Section,
    pub(super) id: &'static str,
    /// For `layer.show`: the layer it switches.
    pub(super) layer: Option<MapLayer>,
    /// Shown with a check mark.
    pub(super) toggle: bool,
    /// The menu's own wording; otherwise the command's name.
    label: Option<&'static str>,
}

const fn item(section: Section, id: &'static str, label: Option<&'static str>) -> Action {
    Action {
        section,
        id,
        layer: None,
        toggle: false,
        label,
    }
}
const fn toggle(section: Section, id: &'static str, label: Option<&'static str>) -> Action {
    Action {
        toggle: true,
        ..item(section, id, label)
    }
}

use Section::*;

/// The items before the map layers, by menu.
const COMMANDS: &[Action] = &[
    item(Application, "ui.about", Some("menu-about-action")),
    item(Application, "ui.settings", Some("menu-settings-action")),
    item(Application, "app.quit", Some("menu-quit-action")),
    item(File, "places.open", Some("menu-kml-open")),
    item(File, "ui.addData", Some("menu-add-data")),
    item(File, "places.save", Some("menu-kml-save")),
    item(File, "places.saveAs", Some("menu-kml-save-as")),
    item(File, "places.export", Some("menu-places-export")),
    item(File, "file.screenshot", Some("menu-screenshot-save")),
    item(File, "file.exportSettings", None),
    item(File, "file.importSettings", None),
    toggle(File, "app.workOffline", None),
    item(Edit, "edit.undo", Some("edit-undo-action")),
    item(Edit, "edit.redo", Some("edit-redo-action")),
    item(Edit, "view.copyLink", None),
    toggle(View, "ui.leftPane", Some("menu-sidebar-toggle")),
    toggle(View, "ui.pane.layers", Some("layers-section-title")),
    toggle(View, "ui.pane.places", Some("sidebar-places-title")),
    toggle(View, "ui.pane.measure", None),
    toggle(View, "ui.legend", None),
    toggle(
        View,
        "ui.navigationControls",
        Some("menu-navigation-toggle"),
    ),
    toggle(View, "ui.ribbon", None),
    item(View, "ui.commandSearch", None),
    toggle(View, "ui.fullscreen", Some("menu-fullscreen-toggle")),
    item(Go, "search.place", None),
    item(
        Go,
        "camera.globalView",
        Some("navigation-global-view-action"),
    ),
    item(
        Go,
        "camera.resetNorth",
        Some("navigation-reset-north-action"),
    ),
    item(Go, "camera.topDown", None),
    item(Go, "camera.zoomIn", None),
    item(Go, "camera.zoomOut", None),
    toggle(Go, "camera.lookAround", None),
    toggle(Go, "camera.autoRotate", None),
    item(Go, "ui.here", None),
    toggle(Live, "ui.timeline", None),
    toggle(Live, "time.play", None),
    item(Add, "places.addAtCenter", Some("menu-add-placemark")),
    item(Add, "places.addFolder", Some("places-add-folder-action")),
    toggle(Tools, "ruler.enable", Some("ruler-enable-toggle")),
    item(Tools, "ruler.profile", None),
    toggle(Tools, "ui.explorer", Some("top-explore-action")),
    toggle(Tools, "ui.pane.assistant", None),
    item(Window, "ui.minimize", Some("menu-minimize-action")),
    item(Help, "ui.shortcuts", Some("menu-controls-help")),
    item(
        Help,
        "help.documentation",
        Some("menu-documentation-action"),
    ),
    item(Help, "help.reportIssue", Some("menu-report-issue-action")),
    item(Help, "help.copyDiagnostics", None),
];

impl Action {
    /// Every item, menu by menu: the commands above, then each layer this build offers in
    /// Map, or in Live for weather and moving objects, in the Layers pane's order.
    pub(super) fn all() -> &'static [Action] {
        static ALL: LazyLock<Vec<Action>> = LazyLock::new(|| {
            let layers = LayerGroup::ALL.into_iter().flat_map(|group| {
                MapLayer::ALL
                    .into_iter()
                    .filter(move |layer| layer.group() == group && crate::app::App::offers(*layer))
                    .map(|layer| Action {
                        section: match layer.group() {
                            LayerGroup::Weather | LayerGroup::LiveObjects => Live,
                            _ => Map,
                        },
                        id: "layer.show",
                        layer: Some(layer),
                        toggle: true,
                        label: None,
                    })
            });
            let mut all: Vec<Action> = COMMANDS.iter().copied().chain(layers).collect();
            // Stable: keeps the order within each menu.
            all.sort_by_key(|action| Section::ALL.iter().position(|s| *s == action.section));
            all
        });
        &ALL
    }
    pub(super) fn label(self, lang: Lang) -> &'static str {
        if let Some(layer) = self.layer {
            return crate::app::ui::short_layer_label(lang, layer);
        }
        lang.text(self.label.unwrap_or_else(|| {
            globecraft_core::commands::find(self.id).map_or(self.id, |spec| spec.label)
        }))
    }
    /// The command this menu item runs.
    pub(in crate::app) fn command(self) -> (&'static str, serde_json::Value) {
        match self.layer {
            Some(layer) => (self.id, serde_json::json!({"layer": layer.code()})),
            None => (self.id, serde_json::Value::Null),
        }
    }
    pub(super) fn shortcut(self) -> Option<egui::KeyboardShortcut> {
        // A layer's shortcut belongs to its ribbon button, which passes the layer too.
        self.layer
            .is_none()
            .then(|| crate::app::commands::shortcut(self.id))
            .flatten()
    }
    /// Whether a separator goes before this item: between groups of layers, and before the
    /// first layer after other items.
    #[cfg(target_os = "macos")]
    pub(super) fn starts_group(self, previous: Option<Action>) -> bool {
        previous.is_some_and(|previous| {
            previous.section == self.section
                && self.layer.map(MapLayer::group) != previous.layer.map(MapLayer::group)
        })
    }
}

/// Each item's state, in `Action::all()` order.
#[cfg(target_os = "macos")]
pub(super) type MenuState = Vec<crate::app::commands::CommandState>;
