//! Service keys as typed (roadmap 10.2: one of the feature states split out of `App`).
use globecraft_data::keys;
use std::collections::BTreeMap;

/// The keys of the keyed services, as the text fields show them.
pub(super) struct KeysState {
    /// The TomTom key, typed in Settings -> Keys and in the Layers pane.
    pub(super) tomtom: String,
    /// The other services' keys as typed in Settings -> Keys, by service id.
    pub(super) others: BTreeMap<&'static str, String>,
}

impl KeysState {
    /// The keys stored on this computer.
    pub(super) fn load() -> Self {
        Self {
            tomtom: keys::get(keys::TOMTOM).unwrap_or_default(),
            others: keys::SERVICES
                .iter()
                .filter(|service| service.id != keys::TOMTOM)
                .map(|service| (service.id, keys::get(service.id).unwrap_or_default()))
                .collect(),
        }
    }
}
