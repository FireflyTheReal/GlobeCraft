//! Which place names were on screen, so a name once drawn keeps its room while a
//! lower layer's names arrive first. After a jump straight to street level the
//! places of interest can be projected and placed before the new place-name tiles;
//! their memory alone would then keep a city or district name hidden.
use globecraft_data::boundaries::{LabelKind, MapLabel};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A name that left the screen (or the data) is remembered this long, so a tile
/// swap or a frame without it does not lose its room to a newly arrived name.
const FORGET_AFTER: Duration = Duration::from_secs(2);

/// One place name, whichever tile or dataset carries it. The English name is the
/// stable identity across the bundled, border and close-up place sets; the kind
/// keeps a city's name apart from a state of the same name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct PlaceNameKey {
    name: String,
    kind: LabelKind,
}

impl PlaceNameKey {
    pub(super) fn of(label: &MapLabel) -> Self {
        Self {
            name: label.en.to_lowercase(),
            kind: label.kind,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Remembered {
    /// The last placement pass that drew it.
    seen: Instant,
}

#[derive(Default)]
pub(super) struct PlaceNameMemory {
    entries: HashMap<PlaceNameKey, Remembered>,
    /// When the last placement pass finished.
    last_time: Option<Instant>,
    /// Changes with the remembered set, so a cached reservation can tell when to
    /// lay the names out again.
    revision: u64,
}

impl PlaceNameMemory {
    pub(super) fn get(&self, key: &PlaceNameKey) -> Option<Remembered> {
        self.entries.get(key).copied()
    }

    /// Names drawn by the last placement first, then names remembered from earlier.
    /// A returning name must be placed before the names of layers that arrived while
    /// it was away.
    pub(super) fn tier(&self, entry: Remembered) -> u8 {
        if Some(entry.seen) == self.last_time {
            0
        } else {
            1
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether no name is remembered at all.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records the names one placement pass drew at `now`. Names not drawn are kept
    /// until `FORGET_AFTER`.
    pub(super) fn finish(&mut self, now: Instant, drawn: impl IntoIterator<Item = PlaceNameKey>) {
        self.last_time = Some(now);
        for key in drawn {
            self.entries.insert(key, Remembered { seen: now });
        }
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.seen) < FORGET_AFTER);
        self.revision = self.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> PlaceNameKey {
        PlaceNameKey::of(&MapLabel {
            lat: 52.52,
            lon: 13.405,
            en: name.into(),
            pt: name.into(),
            kind: LabelKind::City,
            rank: 2,
            big: false,
            from_tiles: false,
        })
    }

    #[test]
    fn names_keep_their_tier_and_are_forgotten_later() {
        let mut memory = PlaceNameMemory::default();
        let start = Instant::now();
        memory.finish(start, vec![key("Berlin")]);
        let entry = memory.get(&key("Berlin")).unwrap();
        assert_eq!(memory.tier(entry), 0);
        // A pass without it keeps it for a moment, in the returning tier.
        memory.finish(start + Duration::from_millis(500), Vec::new());
        let entry = memory.get(&key("Berlin")).unwrap();
        assert_eq!(memory.tier(entry), 1);
        // After FORGET_AFTER it is gone.
        memory.finish(start + FORGET_AFTER, Vec::new());
        assert!(memory.get(&key("Berlin")).is_none());
    }

    #[test]
    fn names_are_matched_across_datasets_and_catalogs() {
        let mut memory = PlaceNameMemory::default();
        let mut label = MapLabel {
            lat: 52.52,
            lon: 13.405,
            en: "Berlin".into(),
            pt: "Berlim".into(),
            kind: LabelKind::City,
            rank: 2,
            big: false,
            from_tiles: false,
        };
        memory.finish(Instant::now(), [PlaceNameKey::of(&label)]);
        // The close-up place set carries the same name with another rank and a
        // translated text: it is still the same name.
        label.rank = 8;
        label.from_tiles = true;
        assert!(memory.get(&PlaceNameKey::of(&label)).is_some());
        label.kind = LabelKind::State;
        assert!(memory.get(&PlaceNameKey::of(&label)).is_none());
    }
}
