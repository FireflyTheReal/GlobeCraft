//! Service keys as typed (roadmap 10.2: one of the feature states split out of `App`).
use globecraft_data::keys;
use std::collections::BTreeMap;

/// The keys of the keyed services, as the text fields show them.
pub(super) struct KeysState {
    /// Each service's key as typed in Settings -> Keys (TomTom's also in the Layers
    /// pane), by service id.
    pub(super) typed: BTreeMap<&'static str, String>,
}

impl KeysState {
    /// The keys stored on this computer.
    pub(super) fn load() -> Self {
        Self {
            typed: keys::SERVICES
                .iter()
                .map(|service| (service.id, keys::get(service.id).unwrap_or_default()))
                .collect(),
        }
    }
    /// Service `id`'s key as typed (empty when none is).
    pub(super) fn get(&self, id: &str) -> &str {
        self.typed.get(id).map_or("", String::as_str)
    }
    /// Service `id`'s key, for a text field to edit.
    pub(super) fn edit(&mut self, id: &'static str) -> &mut String {
        self.typed.entry(id).or_default()
    }
}
