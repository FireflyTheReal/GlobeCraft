//! What the user has picked: a flight or satellite on the globe, the What's here? card
//! and the point a context menu was opened on.
use super::here;

#[derive(Default)]
pub(super) struct Selection {
    /// The flight picked on the globe, with its track and route.
    pub(super) flight: globecraft_data::aircraft::route::FlightSelection,
    pub(super) satellite: super::selected_satellite::SatelliteSelection,
    /// The flights with the selected one's newest position: (list it was made from,
    /// that position's time, flights).
    pub(super) flights_shown: Option<(
        usize,
        f64,
        std::sync::Arc<globecraft_data::aircraft::AircraftSnapshot>,
    )>,
    /// The "What's here?" card, while open.
    pub(super) here: Option<here::HereCard>,
    /// The card's state and municipality, with the border tile revision they were
    /// looked up at (the lookup reads lines, so it is not repeated every frame).
    pub(super) here_admin: Option<(here::HereCard, u64, globecraft_data::boundaries::AdminAreas)>,
    /// Where a globe context menu was opened (latitude, longitude).
    pub(super) menu_pick: Option<(f64, f64)>,
}
