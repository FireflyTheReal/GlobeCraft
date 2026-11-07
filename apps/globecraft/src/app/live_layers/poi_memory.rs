//! The place names on screen, so a name once drawn keeps its place and side while
//! the view zooms and finer tiles replace coarser ones, instead of vanishing or
//! jumping (and fading in again) whenever the set of places is placed anew.
use globecraft_render::live_layers::{PoiKey, PoiShown};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A name that left the screen (or the data) is remembered this long, so a tile
/// swap or a frame without it does not fade it in again.
const FORGET_AFTER: Duration = Duration::from_secs(2);

/// How long a shown name must stay before it keeps its room from place names that
/// arrive later. A jump to street level can show place names a few seconds after
/// the places of interest; those are all newer than this, so the place names may
/// claim room from them. A name being zoomed with has been shown longer and keeps
/// its place.
const SETTLED_AFTER: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Remembered {
    /// Which of its places the name took (right or left of the badge); `None` when
    /// only the marker showed.
    pub side: Option<usize>,
    /// When it first showed: its fade-in runs from here, whatever tile carries it.
    pub since: Instant,
    seen: Instant,
}

#[derive(Default)]
pub(super) struct PoiMemory {
    entries: HashMap<PoiKey, Remembered>,
    /// The remembered keys, shared with the projection so these come first. A new
    /// `Arc` only when the set changes, as the projection compares by pointer.
    shown: Arc<PoiShown>,
    /// When the last placement finished.
    last_time: Option<Instant>,
    /// What the last pass drew: each name's key, side and point on screen.
    last: Vec<(PoiKey, Option<usize>, eframe::egui::Pos2)>,
    /// Names whose marker had no room in the last pass (tests only).
    #[cfg(test)]
    pub(super) blocked: Vec<PoiKey>,
}

impl PoiMemory {
    pub(super) fn get(&self, key: &PoiKey) -> Option<Remembered> {
        if disabled_for_comparison() {
            return None;
        }
        self.entries.get(key).copied()
    }

    /// The order names already shown are placed in: those drawn with their name
    /// last time first, then those that showed only their marker, then those
    /// remembered from earlier. A name that lost its text, or left for a moment,
    /// never takes the room of one still written.
    pub(super) fn tier(&self, entry: Remembered) -> u8 {
        match (Some(entry.seen) == self.last_time, entry.side.is_some()) {
            (true, true) => 0,
            (true, false) => 1,
            (false, _) => 2,
        }
    }

    pub(super) fn shown(&self) -> Arc<PoiShown> {
        if disabled_for_comparison() {
            return Arc::default();
        }
        self.shown.clone()
    }

    /// Whether the last pass drew any name or marker at all.
    pub(super) fn has_shown_name(&self) -> bool {
        !self.last.is_empty()
    }

    /// Whether a name drawn by the last pass has held its place for `SETTLED_AFTER`.
    /// When none has, a jump has just brought this view in: place names arriving
    /// after these names may still claim their room.
    pub(super) fn has_settled_name(&self, now: Instant) -> bool {
        if disabled_for_comparison() {
            return true;
        }
        self.last.iter().any(|(key, _, _)| {
            self.entries
                .get(key)
                .is_some_and(|entry| now.saturating_duration_since(entry.since) >= SETTLED_AFTER)
        })
    }

    /// Records what one placement pass showed at `now`: each key with the side its
    /// name took. Keys not shown are kept until `FORGET_AFTER`.
    pub(super) fn finish(
        &mut self,
        now: Instant,
        shown: Vec<(PoiKey, Option<usize>, eframe::egui::Pos2)>,
    ) {
        self.last = shown.clone();
        self.last_time = Some(now);
        for (key, side, _) in shown {
            let since = self.entries.get(&key).map_or(now, |entry| entry.since);
            self.entries.insert(
                key,
                Remembered {
                    side,
                    since,
                    seen: now,
                },
            );
        }
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.seen) < FORGET_AFTER);
        if self.shown.len() != self.entries.len()
            || self.entries.keys().any(|key| !self.shown.contains(key))
        {
            self.shown = Arc::new(self.entries.keys().cloned().collect());
        }
    }

    /// What the last placement drew (for the zoom-stability check).
    #[cfg(test)]
    pub(super) fn last(&self) -> &[(PoiKey, Option<usize>, eframe::egui::Pos2)] {
        &self.last
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.shown = Arc::default();
    }
}

/// Tests may turn the memory off to measure the placement without it.
fn disabled_for_comparison() -> bool {
    cfg!(test) && std::env::var_os("GLOBECRAFT_TEST_POI_NO_MEMORY").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_data::poi::{PoiPoint, PointKind};

    fn key(name: &str) -> PoiKey {
        PoiKey::of(&PoiPoint {
            kind: PointKind::Poi,
            lat: 52.5,
            lon: 13.4,
            name: name.into(),
            class: String::new(),
            subclass: String::new(),
            rank: 0,
            elevation_m: None,
        })
    }

    #[test]
    fn names_keep_their_side_and_first_time_and_are_forgotten_later() {
        let mut memory = PoiMemory::default();
        let start = Instant::now();
        memory.finish(
            start,
            vec![(key("Stadtmitte"), Some(1), eframe::egui::Pos2::ZERO)],
        );
        let shared = memory.shown();
        assert!(shared.contains(&key("Stadtmitte")));
        // Shown again later, on the other side: the first time stays.
        let later = start + Duration::from_millis(500);
        memory.finish(
            later,
            vec![(key("Stadtmitte"), Some(0), eframe::egui::Pos2::ZERO)],
        );
        let entry = memory.get(&key("Stadtmitte")).unwrap();
        assert_eq!((entry.side, entry.since), (Some(0), start));
        // Same set: the same shared set, so the projection is not redone.
        assert!(Arc::ptr_eq(&shared, &memory.shown()));
        // Missing for a moment: still remembered; after FORGET_AFTER: gone.
        memory.finish(later + Duration::from_millis(300), Vec::new());
        assert!(memory.get(&key("Stadtmitte")).is_some());
        memory.finish(later + FORGET_AFTER, Vec::new());
        assert!(memory.get(&key("Stadtmitte")).is_none());
        assert!(memory.shown().is_empty());
    }

    #[test]
    fn only_a_name_shown_for_a_while_is_settled() {
        let mut memory = PoiMemory::default();
        let start = Instant::now();
        assert!(!memory.has_shown_name());
        memory.finish(
            start,
            vec![(key("Stadtmitte"), Some(1), eframe::egui::Pos2::ZERO)],
        );
        assert!(memory.has_shown_name());
        assert!(!memory.has_settled_name(start + Duration::from_secs(1)));
        assert!(memory.has_settled_name(start + SETTLED_AFTER));
        // A name that was not drawn by the latest pass does not settle the view.
        memory.finish(start + SETTLED_AFTER, Vec::new());
        assert!(!memory.has_shown_name());
        assert!(!memory.has_settled_name(start + SETTLED_AFTER));
    }
}
