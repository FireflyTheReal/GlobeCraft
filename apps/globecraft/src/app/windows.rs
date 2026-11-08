//! Which windows are open (roadmap 10.2: one of the feature states split out of `App`).

/// The Settings, About and Shortcuts windows.
pub(super) struct WindowsState {
    pub(super) settings: bool,
    pub(super) about: bool,
    pub(super) shortcuts: bool,
    /// The UI scale slider's value while it is dragged (committed on release).
    pub(super) ui_scale_edit: f32,
}

impl WindowsState {
    pub(super) fn new(ui_scale: f32) -> Self {
        Self {
            settings: false,
            about: false,
            shortcuts: false,
            ui_scale_edit: ui_scale,
        }
    }
}
