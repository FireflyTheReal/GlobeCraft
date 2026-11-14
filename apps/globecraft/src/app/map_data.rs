//! The map data layers (roadmap 10.2: one of the feature states split out of `App`).
use super::{live_layers, roads};
use eframe::egui;
use globecraft_data::runtime::ServiceRuntime;

/// What the map data layers need to know of the view, each frame.
pub(super) struct MapDataQuery {
    /// The globe shows the Earth (not another world or the flat map).
    pub(super) earth: bool,
    /// The camera: latitude, longitude, altitude (km) and tilt (degrees).
    pub(super) view: (f64, f64, f64, f64),
    /// A transport layer shows.
    pub(super) transport: bool,
    /// Railways or airports show (not only ferries).
    pub(super) rail_or_airports: bool,
    /// A places of interest layer shows.
    pub(super) poi: bool,
    pub(super) roads: bool,
}

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

impl MapDataState {
    /// Loads the tiles around the view for the layers that show, and wakes the
    /// interface when they land.
    pub(super) fn update(
        &mut self,
        runtime: &ServiceRuntime,
        ctx: &egui::Context,
        query: MapDataQuery,
    ) {
        let wake = || {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        let (latitude, longitude, altitude_km, tilt_deg) = query.view;
        self.transport.update(
            runtime,
            globecraft_data::transport::TransportQuery {
                enabled: query.earth && query.transport,
                rail_or_airports: query.rail_or_airports,
                latitude,
                longitude,
                altitude_km,
                tilt_deg,
            },
            wake(),
        );
        self.poi.update(
            runtime,
            globecraft_data::poi::PoiQuery {
                enabled: query.earth && query.poi,
                latitude,
                longitude,
                altitude_km,
                tilt_deg,
            },
            wake(),
        );
        let roads = query.earth && query.roads;
        self.roads.tiles.update(runtime, roads, query.view, wake());
        self.roads.current = if roads {
            self.roads.tiles.roads()
        } else {
            None
        };
    }
}
