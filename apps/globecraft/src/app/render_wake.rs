//! Render-loop wakeup bookkeeping for the globe view.
//!
//! One pass of the UI decides between submitting a frame, waiting for the
//! redraw horizon, or pausing (publication pauses while the sidebar animates).
//! Tile-cache generations must only be marked as handled once they were acted
//! upon: consuming the marker on a throttle-deferred pass made the scheduled
//! wake-up see no change, which froze imagery streaming in after a camera stop
//! exactly while it was transitioning between quality levels.
use std::time::Duration;

/// While imagery streams in, every arrival would otherwise force a full CPU
/// raster about 16 times a second even after the camera stops, keeping every
/// performance core busy until the view is complete. The horizon still shows
/// progressive tiles while roughly halving that sustained load.
pub(in crate::app) const TILE_REDRAW_INTERVAL: Duration = Duration::from_millis(120);

/// What one UI pass must do about the globe view.
pub(in crate::app) enum WakeOutcome {
    /// A camera/style or tile change needs a new render now.
    Submit,
    /// A tile-only change landed inside the redraw horizon; wake up again
    /// after `remaining` instead of rasterizing once per arrival. The tile
    /// generation stays pending so the wake-up can still act on it.
    Defer(Duration),
    /// Publication or submission is paused (sidebar motion); the tile
    /// generation stays pending until the app is at rest again.
    Pause,
    /// Nothing pending this pass; the tile generation was marked as handled.
    Settled,
}

/// Per-pass facts for [`RenderWakeup::decide`].
#[derive(Clone, Copy)]
pub(in crate::app) struct RenderPass {
    /// Camera, model, provider and size key of the current pass.
    pub state: u64,
    /// Tile cache generation; small and monotonic, never wrapped in practice.
    pub generation: u64,
    /// Some awaited tile reached the cache since the displayed frame.
    pub tiles_changed: bool,
    /// A globe texture exists (some frame was published before).
    pub has_texture: bool,
    /// No texture yet and the frame worker is done: the first frame is due.
    pub needs_first_frame: bool,
    /// Publication and submission only resume when the sidebar is at rest.
    pub at_rest: bool,
}

/// Tracks which tile generations have been handled by the render loop.
///
/// `last_gen` of `None` means "dependencies may have changed in a way whose
/// identity is unknown", set on frame publication: tiles can land between a
/// raster and its publication, so the next pass must recheck them.
#[derive(Debug, Default)]
pub(in crate::app) struct RenderWakeup {
    last_gen: Option<u64>,
}

impl RenderWakeup {
    /// True while `generation` has not been acted upon yet.
    pub(in crate::app) fn changed(&self, generation: u64) -> bool {
        self.last_gen != Some(generation)
    }

    /// Marks `generation` as seen.
    pub(in crate::app) fn observe(&mut self, generation: u64) {
        self.last_gen = Some(generation);
    }

    /// A tile may have finished between a raster and its publication, or the
    /// tile provider changed identity; force the next pass to recheck.
    pub(in crate::app) fn dependencies_may_have_changed(&mut self) {
        self.last_gen = None;
    }

    /// Decides the outcome of one UI pass and records which generation was
    /// acted upon. `last_render_age` is the time since the last submission.
    pub(in crate::app) fn decide(
        &mut self,
        pass: RenderPass,
        last_state: u64,
        last_render_age: Option<Duration>,
    ) -> WakeOutcome {
        let need = pass.state != last_state
            || (self.changed(pass.generation) && pass.tiles_changed)
            || pass.needs_first_frame;
        if !need {
            self.observe(pass.generation);
            return WakeOutcome::Settled;
        }
        let throttled = pass.state == last_state
            && pass.has_texture
            && last_render_age.is_some_and(|age| age < TILE_REDRAW_INTERVAL);
        // A deferred change must keep its marker: the scheduled wake-up is the
        // only thing that will act on it. Consuming it here stopped imagery
        // from updating mid-fade after a camera stopped moving.
        if throttled {
            return WakeOutcome::Defer(TILE_REDRAW_INTERVAL - last_render_age.unwrap_or_default());
        }
        if !pass.at_rest {
            return WakeOutcome::Pause;
        }
        self.observe(pass.generation);
        WakeOutcome::Submit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: u64 = 7;

    fn pass() -> RenderPass {
        RenderPass {
            state: STATE,
            generation: 1,
            tiles_changed: true,
            has_texture: true,
            needs_first_frame: false,
            at_rest: true,
        }
    }

    fn age(millis: u64) -> Option<Duration> {
        Some(Duration::from_millis(millis))
    }

    #[test]
    fn deferred_tile_arrival_survives_until_the_horizon_expires() {
        let mut wake = RenderWakeup::default();
        wake.observe(0);
        assert!(
            wake.changed(1),
            "a fresh generation must be seen as changed"
        );
        assert!(matches!(
            wake.decide(pass(), STATE, age(50)),
            WakeOutcome::Defer(_)
        ));
        // The deferred pass must not consume the marker, or the scheduled
        // wake-up would see no change and the frame would freeze mid-fade.
        assert!(wake.changed(1));
        assert!(matches!(
            wake.decide(pass(), STATE, age(121)),
            WakeOutcome::Submit
        ));
        assert!(!wake.changed(1));
    }

    #[test]
    fn camera_change_submits_even_inside_the_horizon() {
        let mut wake = RenderWakeup::default();
        wake.observe(1);
        let mut p = pass();
        p.state = 9;
        assert!(matches!(wake.decide(p, STATE, age(5)), WakeOutcome::Submit));
        // Submitting must consume the generation so the next pass settles.
        assert!(!wake.changed(1));
    }

    #[test]
    fn generation_without_ready_tiles_settles_instead_of_looping() {
        let mut wake = RenderWakeup::default();
        let mut p = pass();
        p.tiles_changed = false;
        assert!(matches!(wake.decide(p, STATE, None), WakeOutcome::Settled));
        assert!(!wake.changed(1));
    }

    #[test]
    fn late_arrivals_after_publication_rerender_then_converge() {
        let mut wake = RenderWakeup::default();
        wake.dependencies_may_have_changed();
        assert!(matches!(
            wake.decide(pass(), STATE, age(200)),
            WakeOutcome::Submit
        ));
        // The returned frame rendered with generation 1; a tile landed while it
        // was being rasterized, publication notes it, and one more render picks
        // it up before falling quiet.
        wake.dependencies_may_have_changed();
        assert!(matches!(
            wake.decide(pass(), STATE, age(10)),
            WakeOutcome::Defer(_)
        ));
        assert!(matches!(
            wake.decide(pass(), STATE, age(150)),
            WakeOutcome::Submit
        ));
        assert!(matches!(
            wake.decide(pass(), STATE, age(300)),
            WakeOutcome::Settled
        ));
    }

    #[test]
    fn first_frame_submits_even_without_a_camera_change() {
        let mut wake = RenderWakeup::default();
        wake.dependencies_may_have_changed();
        let mut p = pass();
        p.tiles_changed = false;
        p.has_texture = false;
        p.needs_first_frame = true;
        assert!(matches!(wake.decide(p, STATE, None), WakeOutcome::Submit));
    }

    #[test]
    fn sidebar_pause_keeps_pending_change_for_when_the_app_is_at_rest() {
        let mut wake = RenderWakeup::default();
        let mut p = pass();
        p.state = 9; // camera moved while the sidebar was animating
        p.at_rest = false;
        assert!(matches!(
            wake.decide(p, STATE, age(500)),
            WakeOutcome::Pause
        ));
        // Camera changes are re-derived from `last_state`, and the tile
        // generation stays pending; once at rest, both submit together.
        p.at_rest = true;
        assert!(matches!(
            wake.decide(p, STATE, age(600)),
            WakeOutcome::Submit
        ));
        assert!(matches!(
            wake.decide(pass(), STATE, age(700)),
            WakeOutcome::Settled
        ));
    }
}
