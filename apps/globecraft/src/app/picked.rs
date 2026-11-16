//! What is picked on the globe (roadmap 10.2: one of the feature states split out of
//! `App`).
use super::selected_satellite::SatelliteSelection;
use globecraft_data::aircraft::{AircraftSnapshot, route::FlightSelection};
use std::sync::Arc;

/// The flight and the satellite picked on the globe.
#[derive(Default)]
pub(super) struct PickedState {
    /// The flight, with its track and route.
    pub(super) flight: FlightSelection,
    pub(super) satellite: SatelliteSelection,
    /// The flights with the picked one's newest position merged in.
    pub(super) snapshot: Option<MergedFlights>,
}

/// The flight list with the picked flight's newest position merged in.
pub(super) struct MergedFlights {
    /// The list it was made from, kept (not its address) so a new list never matches.
    pub(super) from: Arc<AircraftSnapshot>,
    /// When the merged position was observed (Unix seconds).
    pub(super) observed_unix: f64,
    pub(super) flights: Arc<AircraftSnapshot>,
}
