//! What the user has picked: a flight or satellite on the globe, the What's here? card
//! and the point a context menu was opened on.
use super::here;
use globecraft_data::aircraft::AircraftSnapshot;
use std::sync::Arc;

/// The flight list with the selected flight's newest position merged in.
pub(super) struct MergedFlights {
    /// The list it was made from, kept (not its address) so a new list never matches.
    pub(super) from: Arc<AircraftSnapshot>,
    /// When the merged position was observed (Unix seconds).
    pub(super) observed_unix: f64,
    pub(super) flights: Arc<AircraftSnapshot>,
}

/// The What's here? card's state and municipality, with what they were looked up for.
pub(super) struct HereAdmin {
    pub(super) card: here::HereCard,
    /// The border tile revision they were looked up at.
    pub(super) border_revision: u64,
    pub(super) areas: globecraft_data::boundaries::AdminAreas,
}

#[derive(Default)]
pub(super) struct Selection {
    /// The flight picked on the globe, with its track and route.
    pub(super) flight: globecraft_data::aircraft::route::FlightSelection,
    pub(super) satellite: super::selected_satellite::SatelliteSelection,
    /// The flights with the selected one's newest position merged in.
    pub(super) flights_shown: Option<MergedFlights>,
    /// The "What's here?" card, while open.
    pub(super) here: Option<here::HereCard>,
    /// The card's state and municipality (the lookup reads lines, so it is not repeated
    /// every frame).
    pub(super) here_admin: Option<HereAdmin>,
    /// Where a globe context menu was opened (latitude, longitude).
    pub(super) menu_pick: Option<(f64, f64)>,
}
