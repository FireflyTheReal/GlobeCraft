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
}
