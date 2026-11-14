//! The map data layers (roadmap 10.2: one of the feature states split out of `App`).
use super::{live_layers, roads};

/// Roads, transport and places of interest: their tiles around the view and the
/// shapes drawn from them.
#[derive(Default)]
pub(super) struct MapDataState {
    pub(super) transport: globecraft_data::transport::TransportTiles,
    pub(super) transport_shapes: live_layers::TransportShapes,
    pub(super) poi: globecraft_data::poi::PoiTiles,
    pub(super) poi_shapes: live_layers::PoiShapes,
    pub(super) roads: roads::RoadOverlay,
}
