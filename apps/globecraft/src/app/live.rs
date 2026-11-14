//! The live layers (roadmap 10.2: one of the feature states split out of `App`).
use super::{live_geometry, live_layers, live_maps, timezones, traffic};
use eframe::egui;
use globecraft_data::tiles::TileCache;
use std::sync::Arc;

/// The realtime feeds and what is drawn from them: aircraft, vessels, satellites,
/// the aurora, weather maps and traffic, with the airports, time zones and sea names
/// loaded alongside.
pub(super) struct LiveState {
    pub(super) aircraft: globecraft_data::aircraft::LiveAircraft,
    pub(super) aircraft_overlay: live_layers::AircraftOverlay,
    pub(super) vessels: globecraft_data::vessels::LiveVessels,
    pub(super) satellites: globecraft_data::satellites::LiveSatellites,
    /// The vessels and satellites as drawn.
    pub(super) world_shapes: live_layers::WorldShapes,
    pub(super) aurora: globecraft_data::aurora::LiveAurora,
    pub(super) aurora_shapes: live_layers::AuroraShapes,
    pub(super) airports: globecraft_data::airports::LiveAirports,
    pub(super) time_zones: globecraft_data::timezones::LiveTimeZones,
    pub(super) time_zone_shapes: timezones::TimeZoneShapes,
    pub(super) marine_names: globecraft_data::marine_names::LiveMarineNames,
    /// Where the sea and undersea names went this frame.
    pub(super) marine_placed: globecraft_render::overlays::PlacedLabels,
    /// Clouds and rain radar.
    pub(super) maps: live_maps::LiveMaps,
    pub(super) traffic: traffic::TrafficOverlay,
    pub(super) geometry: live_geometry::LiveOverlay,
}

impl LiveState {
    pub(super) fn new(
        runtime: &globecraft_data::runtime::ServiceRuntime,
        ctx: &egui::Context,
        cache: &Arc<TileCache>,
    ) -> Self {
        Self {
            aircraft: globecraft_data::aircraft::LiveAircraft::new(runtime.handle(), {
                let ctx = ctx.clone();
                move || ctx.request_repaint()
            }),
            aircraft_overlay: live_layers::AircraftOverlay::new(ctx.clone()),
            vessels: Default::default(),
            satellites: Default::default(),
            world_shapes: Default::default(),
            aurora: Default::default(),
            aurora_shapes: Default::default(),
            airports: Default::default(),
            time_zones: Default::default(),
            time_zone_shapes: Default::default(),
            marine_names: Default::default(),
            marine_placed: Default::default(),
            maps: Default::default(),
            traffic: Default::default(),
            geometry: live_geometry::LiveOverlay::new(ctx.clone(), Some(cache.clone())),
        }
    }
}
