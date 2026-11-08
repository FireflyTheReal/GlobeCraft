//! Press and hold on the globe (roadmap 6.12): touch screens and trackpads have no
//! right button, so holding the primary button still opens the "What's here?" card
//! at that point, as the context menu's entry does.
use eframe::egui::Pos2;

/// How long the pointer must stay down. Shorter than egui's long touch (its
/// `max_click_duration`, 0.8 s), which opens the context menu on touch screens: the
/// card comes first, and that menu is then left closed.
const HOLD_SECS: f64 = 0.6;
/// The pointer may wander this far (screen points) and still be holding, as egui
/// allows for a click; farther, it is a drag.
const MAX_MOVE_POINTS: f32 = 6.0;

/// One press, from button down to button up.
#[derive(Clone, Copy, Debug)]
struct Press {
    origin: Pos2,
    start_secs: f64,
    /// It moved too far: a drag, never a long press.
    moved: bool,
    /// The long press happened during this press.
    fired: bool,
}

/// What a frame's pointer state means for the long press.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct PressFrame {
    /// The press has just been held long enough, here: open the card.
    pub fire_at: Option<Pos2>,
    /// The press has fired, so it is not also a pan, a click or a context menu (its
    /// release, if under egui's click duration, would otherwise count as a click).
    pub swallow: bool,
    /// Seconds until the press would fire, so the app wakes up then without input.
    pub wake_in_secs: Option<f64>,
}

#[derive(Debug, Default)]
pub(super) struct LongPress {
    press: Option<Press>,
}

impl LongPress {
    /// Feeds one frame: whether the primary button is down on the globe, where the
    /// pointer is, and the input time (seconds).
    pub(super) fn update(&mut self, down: bool, pos: Option<Pos2>, now_secs: f64) -> PressFrame {
        if !down {
            // The release frame is still the long press's own.
            let swallow = self.press.take().is_some_and(|press| press.fired);
            return PressFrame {
                swallow,
                ..PressFrame::default()
            };
        }
        let Some(pos) = pos else {
            return PressFrame {
                swallow: self.press.is_some_and(|press| press.fired),
                ..PressFrame::default()
            };
        };
        let press = self.press.get_or_insert(Press {
            origin: pos,
            start_secs: now_secs,
            moved: false,
            fired: false,
        });
        if pos.distance(press.origin) > MAX_MOVE_POINTS {
            press.moved = true;
        }
        let mut frame = PressFrame::default();
        if !press.moved && !press.fired {
            let held = now_secs - press.start_secs;
            if held >= HOLD_SECS {
                press.fired = true;
                frame.fire_at = Some(pos);
            } else {
                frame.wake_in_secs = Some(HOLD_SECS - held);
            }
        }
        frame.swallow = press.fired;
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Option<Pos2> {
        Some(Pos2::new(x, y))
    }

    #[test]
    fn holding_still_fires_once_and_swallows_the_release() {
        let mut press = LongPress::default();
        let first = press.update(true, at(10.0, 10.0), 1.0);
        assert_eq!(first.fire_at, None);
        assert!(!first.swallow);
        assert!((first.wake_in_secs.unwrap() - HOLD_SECS).abs() < 1e-9);
        // A small wobble is still a hold.
        let early = press.update(true, at(13.0, 12.0), 1.0 + HOLD_SECS / 2.0);
        assert_eq!(early.fire_at, None);
        let fired = press.update(true, at(12.0, 11.0), 1.0 + HOLD_SECS);
        assert_eq!(fired.fire_at, at(12.0, 11.0));
        assert!(fired.swallow);
        let held_on = press.update(true, at(12.0, 11.0), 2.5);
        assert_eq!(held_on.fire_at, None, "fires once per press");
        assert!(held_on.swallow);
        assert_eq!(held_on.wake_in_secs, None);
        let release = press.update(false, None, 2.6);
        assert!(release.swallow, "the release is not a click");
        let after = press.update(false, None, 2.7);
        assert_eq!(after, PressFrame::default());
    }

    #[test]
    fn moving_away_is_a_drag_even_if_it_comes_back() {
        let mut press = LongPress::default();
        press.update(true, at(0.0, 0.0), 0.0);
        let moved = press.update(true, at(MAX_MOVE_POINTS + 1.0, 0.0), 0.1);
        assert_eq!(moved.wake_in_secs, None);
        let back = press.update(true, at(0.0, 0.0), 1.0);
        assert_eq!(back, PressFrame::default());
        assert!(!press.update(false, None, 1.1).swallow);
    }

    #[test]
    fn a_short_tap_is_left_alone() {
        let mut press = LongPress::default();
        press.update(true, at(5.0, 5.0), 0.0);
        assert_eq!(press.update(false, None, 0.2), PressFrame::default());
        // The next press starts afresh.
        let next = press.update(true, at(50.0, 50.0), 3.0);
        assert!((next.wake_in_secs.unwrap() - HOLD_SECS).abs() < 1e-9);
    }
}
