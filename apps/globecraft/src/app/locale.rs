//! The language and regional conventions (roadmap 10.2: one of the feature states
//! split out of `App`). The language in use is `App::lang`, which every label reads.
use globecraft_core::{
    language::{Lang, LanguagePreference},
    units::DistanceUnits,
};
use globecraft_data::settings::load_language_preference;

/// The user's language choice and what the system says about its region.
pub(super) struct LocaleState {
    pub(super) preference: LanguagePreference,
    /// The system's locale, such as `pt-BR`.
    pub(super) system: String,
    /// The distance units of the system's region.
    pub(super) system_units: DistanceUnits,
    /// Whether the system's region is the United States (the units' names follow it).
    pub(super) us: bool,
}

impl LocaleState {
    /// The saved language choice and the system's locale.
    pub(super) fn detect() -> Self {
        let system = sys_locale::get_locale().unwrap_or_default();
        Self {
            preference: load_language_preference(),
            system_units: DistanceUnits::for_locale(&system),
            us: globecraft_core::units::is_us_locale(&system),
            system,
        }
    }

    /// The language the interface starts in.
    pub(super) fn language(&self) -> Lang {
        self.preference.resolve(&self.system)
    }
}
