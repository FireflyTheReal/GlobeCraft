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
    /// Deleted places Undo can put back, newest last, each with how many steps the ruler's
    /// own history held then: the one history the user sees runs through both.
    pub(super) undo: Vec<(Deleted, usize)>,
    /// Places Undo put back, which Redo deletes again, with the ruler's redo steps then.
    pub(super) redo: Vec<(u64, usize)>,
}

/// A deleted place or folder (with what was inside it) and where it was.
pub(super) struct Deleted {
    parent: Option<u64>,
    index: usize,
    node: globecraft_core::placemark::PlaceNode,
}

/// Undo keeps this many deletions.
const MAX_PLACE_STEPS: usize = 100;

impl PlacesState {
    /// Removes place or folder `id` (with what is inside it) so that Undo can put it back;
    /// a Redo passes `redo` to keep the steps it can still redo.
    pub(super) fn delete(&mut self, id: u64, ruler_steps: usize, redo: bool) {
        let Some((parent, index, node)) = self.items.take(id) else {
            return;
        };
        if self.undo.len() == MAX_PLACE_STEPS {
            self.undo.remove(0);
        }
        self.undo.push((
            Deleted {
                parent,
                index,
                node,
            },
            ruler_steps,
        ));
        if !redo {
            self.redo.clear();
        }
        if self.selection == Some(id) {
            self.selection = None;
        }
    }
    /// Puts the last deleted place back where it was. Its id, for Redo.
    pub(super) fn restore(&mut self) -> Option<u64> {
        let (deleted, _) = self.undo.pop()?;
        let id = deleted.node.id();
        self.items
            .insert_at(deleted.parent, deleted.index, deleted.node);
        Some(id)
    }
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
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}
