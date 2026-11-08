//! The ruler's state (roadmap 10.2: one of the feature states split out of `App`).
use super::ui;

/// The ruler: whether it is on, its paths and their history, and what is being
/// edited or pointed at.
#[derive(Default)]
pub(super) struct MeasureState {
    /// Clicks on the globe add ruler points.
    pub(super) on: bool,
    /// Open the Measure pane on the next frame.
    pub(super) reveal_requested: bool,
    /// The point being dragged.
    pub(super) drag: Option<globecraft_core::ruler::RulerPointId>,
    pub(super) paths: globecraft_core::ruler::Ruler,
    pub(super) point_editor: Option<ui::ruler::PointEditor>,
    /// The point of the ruler's elevation profile under the pointer, marked on the globe.
    pub(super) profile_hover: Option<(f64, f64)>,
}
