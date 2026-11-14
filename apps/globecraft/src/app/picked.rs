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
    /// The flights with the picked one's newest position: (list it was made from,
    /// that position's time, flights).
    pub(super) snapshot: Option<(usize, f64, Arc<AircraftSnapshot>)>,
}
