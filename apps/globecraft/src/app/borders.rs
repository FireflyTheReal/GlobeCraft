//! The borders and names: what the loader sent, the set drawn (with the close-up place
//! names merged in) and the overlay painted from it.
use globecraft_data::boundaries::{BoundaryData, BoundaryUpdates, StatusLog};
use std::sync::Arc;

pub(super) struct Borders {
    /// The boundaries drawn: `source` with the close-up place names.
    pub(super) drawn: Arc<BoundaryData>,
    /// The boundaries as the loader sent them.
    pub(super) source: Arc<BoundaryData>,
    /// Zoom 13–14 place names around the view.
    pub(super) place_names: globecraft_data::boundaries::PlaceNameTiles,
    /// The place-name snapshot `drawn` was built with. Kept, not just its address, so
    /// a new snapshot can never reuse the address and look unchanged.
    pub(super) place_names_shown: Option<globecraft_data::boundaries::PlaceNameSnapshot>,
    pub(super) overlay_cache: super::overlays::OverlayCache,
    pub(super) updates: BoundaryUpdates,
    pub(super) loader: globecraft_data::boundaries::BoundaryLoader,
    pub(super) log: StatusLog,
}
