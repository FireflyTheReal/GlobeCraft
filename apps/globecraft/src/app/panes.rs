//! What the panes keep between frames (roadmap 10.2: one of the feature states split
//! out of `App`).
use super::{legend, ui};

/// The Settings pages, the Layers pane and the colour keys: what each shows and edits.
#[derive(Default)]
pub(super) struct PanesState {
    pub(super) settings: ui::settings::SettingsUi,
    pub(super) layers: ui::layers_pane::LayersUi,
    /// The colour keys window.
    pub(super) legend: legend::Legend,
}
