//! How the camera is moving (roadmap 10.2: one of the feature states split out of
//! `App`). Where it is stays `App::cam`, which every feature reads.

/// The camera's own movement: a flight, the idle rotation and the ground it follows.
#[derive(Default)]
pub(super) struct MotionState {
    pub(super) fly: Option<globecraft_core::flight::Flight>,
    pub(super) auto_rotate: bool,
    /// The terrain height (km, exaggerated) the camera looks at, eased toward the
    /// elevation tiles so a finer tile arriving does not jolt the view.
    pub(super) ground_km: f64,
    /// The turn-or-tilt drag under way.
    pub(super) turn_drag: Option<TurnDrag>,
}

/// A drag with the secondary button (or Shift): it turns the view or tilts it, not
/// both. Doing both at once, every turn also tilted a little and the view was hard
/// to bring where it should go (user, 2026-11-16).
#[derive(Default)]
pub(super) struct TurnDrag {
    /// How far the drag went before it was clear which of the two it is.
    pub(super) travelled: eframe::egui::Vec2,
    /// Whether it tilts (else turns), once clear.
    pub(super) tilts: Option<bool>,
}
