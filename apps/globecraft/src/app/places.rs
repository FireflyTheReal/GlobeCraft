//! The places' state (roadmap 10.2: one of the feature states split out of `App`).
use super::files;
use globecraft_core::placemark::Places;

/// The user's places: the list, the one selected, the name typed for a new one and
/// the KML file they were read from or are saved to.
pub(super) struct PlacesState {
    pub(super) items: Places,
    pub(super) selection: Option<u64>,
    /// Open the Places pane on the next frame.
    pub(super) reveal_requested: bool,
    pub(super) new_name: String,
    pub(super) kml_path: String,
    pub(super) kml_session: files::KmlSession,
}

impl Default for PlacesState {
    fn default() -> Self {
        Self {
            items: Places::default(),
            selection: None,
            reveal_requested: false,
            new_name: String::new(),
            kml_path: "placemarks.kml".into(),
            kml_session: files::KmlSession::default(),
        }
    }
}
