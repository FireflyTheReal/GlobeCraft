//! Borders and place names (roadmap 10.2: one of the feature states split out of `App`).
use super::overlays::{LayerArrivals, OverlayCache};
use globecraft_data::boundaries::{
    BoundaryData, BoundaryLoader, BoundaryUpdates, PlaceNameTiles, StatusLog,
};
use std::sync::Arc;

/// The boundary data, its loading, and the overlay geometry built from it.
pub(super) struct BordersState {
    pub(super) loader: BoundaryLoader,
    /// The boundaries drawn: `source` with the close-up place names.
    pub(super) shown: Arc<BoundaryData>,
    /// The boundaries as the loader sent them.
    pub(super) source: Arc<BoundaryData>,
    /// Zoom 13–14 place names around the view.
    pub(super) place_names: PlaceNameTiles,
    /// The place-name snapshot `shown` was built with (its address; 0 for none).
    pub(super) place_names_shown: usize,
    pub(super) overlay_cache: OverlayCache,
    pub(super) updates: BoundaryUpdates,
    pub(super) log: StatusLog,
    pub(super) arrivals: LayerArrivals,
}

impl BordersState {
    pub(super) fn new(
        loader: BoundaryLoader,
        updates: BoundaryUpdates,
        log: StatusLog,
        overlay_cache: OverlayCache,
    ) -> Self {
        Self {
            loader,
            shown: Arc::new(BoundaryData::default()),
            source: Arc::new(BoundaryData::default()),
            place_names: Default::default(),
            place_names_shown: 0,
            overlay_cache,
            updates,
            log,
            arrivals: LayerArrivals::default(),
        }
    }
}
