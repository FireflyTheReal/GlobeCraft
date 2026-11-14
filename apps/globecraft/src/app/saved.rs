//! The settings on disk (roadmap 10.2: one of the feature states split out of `App`).
//! The settings in use are `App::settings`.
use globecraft_data::settings::{Settings, SettingsWriter};

/// The settings as last handed to the writer, and the writer.
pub(super) struct SavedSettings {
    pub(super) settings: Settings,
    pub(super) writer: SettingsWriter,
}
