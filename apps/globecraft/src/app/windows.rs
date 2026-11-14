//! Which windows are open (roadmap 10.2: one of the feature states split out of `App`).
use super::ui;

/// The Settings, About and Shortcuts windows, and the dialogs that exist only while
/// they are open.
pub(super) struct WindowsState {
    pub(super) settings: bool,
    pub(super) about: bool,
    pub(super) shortcuts: bool,
    /// The UI scale slider's value while it is dragged (committed on release).
    pub(super) ui_scale_edit: f32,
    /// The command palette, while open.
    pub(super) command_search: Option<ui::palette::CommandSearch>,
    /// The altitude popover, while open.
    pub(super) altitude_editor: Option<ui::altitude::AltitudeEditor>,
    /// The Add Data window, while open.
    pub(super) add_data: Option<ui::add_data::AddDataDialog>,
}

impl WindowsState {
    pub(super) fn new(ui_scale: f32) -> Self {
        Self {
            settings: false,
            about: false,
            shortcuts: false,
            ui_scale_edit: ui_scale,
            command_search: None,
            altitude_editor: None,
            add_data: None,
        }
    }
}
