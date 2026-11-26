//! The live layers (roadmap 10.2: one of the feature states split out of `App`).
use super::{live_geometry, live_layers, live_maps, timezones, traffic};
use eframe::egui;
use globecraft_core::camera::Cam;
use globecraft_data::{runtime::ServiceRuntime, tiles::TileCache};
use std::sync::Arc;

/// Half the view's diagonal on the ground, in camera altitudes per unit of the
/// vertical field of view's half tangent (a 16:10 window).
const VIEW_HALF_DIAGONAL_FACTOR: f64 = 1.89;
/// The tilt at which the aircraft feed looks twice as far: a tilted view reaches
/// towards the horizon.
const TILT_DOUBLE_REACH_DEG: f64 = 45.0;
/// The aircraft feed is asked for no more than this around the view's centre.
const MAX_AIRCRAFT_RADIUS_KM: f64 = 2500.0;

/// What the feeds other than aircraft need to know of the view, each frame.
pub(super) struct LiveQuery<'a> {
    /// The globe shows the Earth (not another world or the flat map).
    pub(super) earth: bool,
    pub(super) cam: &'a Cam,
    /// The layers shown.
    pub(super) vessels: bool,
    pub(super) satellites: bool,
    pub(super) aurora: bool,
    pub(super) traffic: bool,
    /// Something looks airports up by code.
    pub(super) airports: bool,
    pub(super) tomtom_key: &'a str,
}

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
    /// Asks the aircraft feed for the flights around the view; `enabled` while they
    /// show on the globe, which then repaints at their pace.
    pub(super) fn update_aircraft(&mut self, ctx: &egui::Context, enabled: bool, cam: &Cam) {
        self.aircraft
            .update(globecraft_data::aircraft::AircraftQuery {
                enabled,
                latitude: cam.lat,
                longitude: cam.lon,
                altitude_km: cam.alt,
                // Half the view's diagonal on the ground, longer when tilted towards
                // the horizon.
                radius_km: (cam.alt
                    * globecraft_core::VERTICAL_FOV_HALF_TAN
                    * VIEW_HALF_DIAGONAL_FACTOR
                    * (1.0 + cam.tilt.max(0.0) / TILT_DOUBLE_REACH_DEG))
                    .min(MAX_AIRCRAFT_RADIUS_KM),
            });
        if enabled {
            ctx.request_repaint_after(live_layers::FLIGHT_REPAINT_INTERVAL);
        }
    }

    /// Starts, refreshes or pauses the independent realtime feeds: each loads only
    /// for the globe while its layer shows, and wakes the interface when data lands.
    pub(super) fn update_feeds(
        &mut self,
        runtime: &ServiceRuntime,
        ctx: &egui::Context,
        query: LiveQuery<'_>,
    ) {
        let wake = || {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        self.vessels
            .update(runtime, query.earth && query.vessels, wake());
        self.satellites
            .update(runtime, query.earth && query.satellites, wake());
        self.aurora
            .update(runtime, query.earth && query.aurora, wake());
        self.airports.update(runtime, query.airports, wake());
        self.traffic.service.update(
            runtime,
            globecraft_data::traffic::TrafficQuery {
                enabled: query.earth && query.traffic,
                key: query.tomtom_key,
                view: globecraft_data::ViewQuery::from(*query.cam),
            },
            wake(),
        );
    }

    pub(super) fn new(
        runtime: &ServiceRuntime,
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
