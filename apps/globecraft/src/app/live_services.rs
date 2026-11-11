//! The live services and tile streams that follow the view: each frame asks every one
//! for the camera's [`ViewQuery`] while its layer is shown on the globe, and each wakes
//! the interface through [`repainter`] when new data arrives.
use super::{App, MapLayer, live_layers};
use eframe::egui;
use globecraft_data::ViewQuery;
use std::time::Duration;

/// Live services refresh only while the interface runs: this heartbeat keeps them
/// going while one is shown, so they do not wait for the user to move.
const LIVE_SERVICE_HEARTBEAT: Duration = Duration::from_secs(30);
/// The ground radius asked for live aircraft: half the view's diagonal on the ground (a
/// 16:10 window's half diagonal is about 1.89 times its half height) ...
const AIRCRAFT_QUERY_HALF_DIAGONAL_RATIO: f64 = 1.89;
/// ... longer when tilted, by one more radius per this many degrees of tilt ...
const AIRCRAFT_QUERY_TILT_STEP_DEG: f64 = 45.0;
/// ... and never more than this.
const AIRCRAFT_QUERY_MAX_RADIUS_KM: f64 = 2500.0;

/// The live data services: each loads for the view while its layer is shown. Drawing
/// state (shapes, overlays) stays with the layers that draw it.
pub(super) struct LiveFeeds {
    pub(super) aircraft: globecraft_data::aircraft::LiveAircraft,
    pub(super) vessels: globecraft_data::vessels::LiveVessels,
    pub(super) satellites: globecraft_data::satellites::LiveSatellites,
    pub(super) aurora: globecraft_data::aurora::LiveAurora,
    pub(super) airports: globecraft_data::airports::LiveAirports,
    pub(super) time_zones: globecraft_data::timezones::LiveTimeZones,
    pub(super) marine_names: globecraft_data::marine_names::LiveMarineNames,
    pub(super) transport: globecraft_data::transport::TransportTiles,
    pub(super) poi: globecraft_data::poi::PoiTiles,
}

impl LiveFeeds {
    pub(super) fn new(
        runtime: &globecraft_data::runtime::ServiceRuntime,
        ctx: &egui::Context,
    ) -> Self {
        Self {
            aircraft: globecraft_data::aircraft::LiveAircraft::new(
                runtime.handle(),
                repainter(ctx),
            ),
            vessels: Default::default(),
            satellites: Default::default(),
            aurora: Default::default(),
            airports: Default::default(),
            time_zones: Default::default(),
            marine_names: Default::default(),
            transport: Default::default(),
            poi: Default::default(),
        }
    }
}

/// The callback a service calls from its worker when it has something new to show.
pub(super) fn repainter(ctx: &egui::Context) -> impl Fn() + Send + Sync + Clone + 'static {
    let ctx = ctx.clone();
    move || ctx.request_repaint()
}

/// The ground radius live aircraft are asked for around the camera, km.
fn aircraft_radius_km(view: ViewQuery) -> f64 {
    (view.altitude_km
        * globecraft_core::VERTICAL_FOV_HALF_TAN
        * AIRCRAFT_QUERY_HALF_DIAGONAL_RATIO
        * (1.0 + view.tilt_deg.max(0.0) / AIRCRAFT_QUERY_TILT_STEP_DEG))
        .min(AIRCRAFT_QUERY_MAX_RADIUS_KM)
}

impl App {
    pub(super) fn update_live_services(&mut self, ctx: &egui::Context) {
        let view = ViewQuery::from(self.cam);
        let earth = self.earth_shown();
        let aircraft_enabled =
            self.layer(MapLayer::Flights) && self.explorer.on_earth() && !self.flat_earth.open;
        self.live
            .aircraft
            .update(globecraft_data::aircraft::AircraftQuery {
                enabled: aircraft_enabled,
                latitude: view.latitude,
                longitude: view.longitude,
                altitude_km: view.altitude_km,
                radius_km: aircraft_radius_km(view),
            });
        if aircraft_enabled {
            ctx.request_repaint_after(live_layers::FLIGHT_REPAINT_INTERVAL);
        }
        self.update_selected_flight(ctx, aircraft_enabled);
        // Independent realtime layers, loaded only for the globe while enabled.
        self.live.vessels.update(
            &self.runtime,
            earth && self.layer(MapLayer::Vessels),
            repainter(ctx),
        );
        self.live.satellites.update(
            &self.runtime,
            earth && self.layer(MapLayer::Satellites),
            repainter(ctx),
        );
        self.live.aurora.update(
            &self.runtime,
            earth && self.layer(MapLayer::Aurora),
            repainter(ctx),
        );
        // Airports by code: for the search box and a selected flight's route.
        self.live.airports.update(
            &self.runtime,
            self.selection.flight.selected().is_some() || !self.search.query.trim().is_empty(),
            repainter(ctx),
        );
        self.update_time_zones(ctx, earth);
        self.update_marine_names(ctx, earth);
        self.live.transport.update(
            &self.runtime,
            globecraft_data::transport::TransportQuery {
                enabled: earth && self.transport_shown(),
                rail_or_airports: self.layer(MapLayer::Railways) || self.layer(MapLayer::Airports),
                view,
            },
            repainter(ctx),
        );
        self.live.poi.update(
            &self.runtime,
            globecraft_data::poi::PoiQuery {
                enabled: earth && self.poi_layers().any(),
                view,
            },
            repainter(ctx),
        );
        let scene = self.imagery_time();
        self.user_layers.update(
            ctx,
            &self.runtime,
            &self.settings.user_layers,
            view,
            earth,
            scene,
        );
        if self.time.speed() != 0.0 {
            // A playing time moves the sun and the timed points every frame.
            ctx.request_repaint();
        }
        self.borders.place_names.update(
            &self.runtime,
            earth && self.layer(MapLayer::PlaceLabels),
            view,
            repainter(ctx),
        );
        self.refresh_place_names();
        let roads_shown = earth && self.layer(MapLayer::Roads);
        self.roads
            .tiles
            .update(&self.runtime, roads_shown, view, repainter(ctx));
        self.roads.current = if roads_shown {
            self.roads.tiles.roads()
        } else {
            None
        };
        self.traffic.service.update(
            &self.runtime,
            globecraft_data::traffic::TrafficQuery {
                enabled: earth && self.layer(MapLayer::Traffic),
                key: &self.tomtom_key,
                view,
            },
            repainter(ctx),
        );
        if earth
            && (self.layer(MapLayer::Vessels)
                || self.layer(MapLayer::Satellites)
                || self.layer(MapLayer::Roads)
                || self.layer(MapLayer::Traffic)
                || self.layer(MapLayer::Aurora)
                || self.user_layers_refresh())
        {
            ctx.request_repaint_after(LIVE_SERVICE_HEARTBEAT);
        }
    }
}
