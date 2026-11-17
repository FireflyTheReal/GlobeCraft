//! How the camera is moving (roadmap 10.2: one of the feature states split out of
//! `App`). Where it is stays `App::cam`, which every feature reads.

/// The camera's own movement: a flight, the idle rotation and the ground it follows.
#[derive(Default)]
pub(super) struct MotionState {
    pub(super) fly: Option<globecraft_core::flight::Flight>,
    pub(super) auto_rotate: bool,
    /// The terrain height (km, exaggerated) the camera looks at: the first ground
    /// along the line of sight.
    pub(super) ground_km: f64,
    /// Heading and tilt last frame, and when they last changed: while they change,
    /// the view turns about the looked-at point and it stays put.
    pub(super) orbit_seen: (f64, f64),
    pub(super) orbit_changed: Option<std::time::Instant>,
    /// The turn-or-tilt drag under way.
    pub(super) turn_drag: Option<TurnDrag>,
    /// What the last drag did, for the glide after it.
    pub(super) last_drag: Option<DragKind>,
    /// The view coasting on after a drag was let go.
    pub(super) glide: Option<Glide>,
}

/// What a drag does to the view.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum DragKind {
    Pan,
    Turn,
    Tilt,
}

/// The view coasting on after a drag, slowing down (user, 2026-11-17: "natural
/// physics of globe … to keep rotating a bit").
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Glide {
    /// Screen points per second, as the drag moved the globe.
    Pan(eframe::egui::Vec2),
    /// Degrees of heading per second.
    Turn(f64),
}

/// A glide's speed falls by `e` over this time; it travels its starting speed times
/// this in all.
pub(super) const GLIDE_SLOWDOWN_SECS: f32 = 0.35;
/// A drag let go slower than this (points per second) stops where it is: a slow,
/// careful drag means to put the view exactly there.
pub(super) const GLIDE_MIN_START_POINTS_PER_SEC: f32 = 150.0;
/// A glide ends once slower than this.
const GLIDE_STOP_POINTS_PER_SEC: f32 = 8.0;

impl Glide {
    /// The glide from a drag of `kind` let go at `velocity` (points per second), if it
    /// coasts.
    pub(super) fn after(
        kind: DragKind,
        velocity: eframe::egui::Vec2,
        turn_deg_per_point: f64,
    ) -> Option<Self> {
        if velocity.length() < GLIDE_MIN_START_POINTS_PER_SEC {
            return None;
        }
        match kind {
            DragKind::Pan => Some(Self::Pan(velocity)),
            DragKind::Turn => Some(Self::Turn(-f64::from(velocity.x) * turn_deg_per_point)),
            DragKind::Tilt => None,
        }
    }
    /// How far it moves over `dt` seconds, and the glide after that (`None`: stopped).
    /// The speed falls exponentially; the step is its exact integral, so the distance
    /// does not depend on the frame rate.
    pub(super) fn step(self, dt: f32, turn_deg_per_point: f64) -> (Self, Option<Self>) {
        let fall = (-dt / GLIDE_SLOWDOWN_SECS).exp();
        let travelled = GLIDE_SLOWDOWN_SECS * (1.0 - fall);
        let (moved, next, points_per_sec) = match self {
            Self::Pan(v) => (
                Self::Pan(v * travelled),
                Self::Pan(v * fall),
                v.length() * fall,
            ),
            Self::Turn(w) => (
                Self::Turn(w * f64::from(travelled)),
                Self::Turn(w * f64::from(fall)),
                (w * f64::from(fall) / turn_deg_per_point).abs() as f32,
            ),
        };
        (
            moved,
            (points_per_sec >= GLIDE_STOP_POINTS_PER_SEC).then_some(next),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn a_glide_coasts_the_same_way_at_any_frame_rate_and_stops() {
        let start = Glide::Pan(vec2(1000.0, 0.0));
        let coast = |dt: f32| {
            let (mut glide, mut total) = (Some(start), 0.0);
            while let Some(now) = glide {
                let (moved, next) = now.step(dt, 0.3);
                if let Glide::Pan(d) = moved {
                    total += d.x;
                }
                glide = next;
            }
            total
        };
        // Starting at 1,000 points a second it coasts about 350 points.
        let at_60 = coast(1.0 / 60.0);
        assert!((at_60 - 347.0).abs() < 5.0, "{at_60}");
        assert!((coast(1.0 / 120.0) - at_60).abs() < 2.0);
        assert!((coast(1.0 / 30.0) - at_60).abs() < 2.0);
        // Slow drags do not coast; tilting does not.
        assert_eq!(Glide::after(DragKind::Pan, vec2(100.0, 0.0), 0.3), None);
        assert_eq!(Glide::after(DragKind::Tilt, vec2(0.0, 900.0), 0.3), None);
        // A turn coasts the way the drag turned (right: heading falls).
        assert!(
            matches!(Glide::after(DragKind::Turn, vec2(500.0, 0.0), 0.3), Some(Glide::Turn(w)) if w < 0.0)
        );
    }
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
