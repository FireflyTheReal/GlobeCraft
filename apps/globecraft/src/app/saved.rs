//! The settings on disk (roadmap 10.2: one of the feature states split out of `App`).
//! The settings in use are `App::settings`.
use globecraft_data::settings::{Settings, SettingsWriter};

/// The settings as last handed to the writer, and the writer.
pub(super) struct SavedSettings {
    pub(super) settings: Settings,
    /// When the settings in use were last compared with `settings` (see
    /// `App::persist_settings`).
    pub(super) checked: std::time::Instant,
    pub(super) writer: SettingsWriter,
}
