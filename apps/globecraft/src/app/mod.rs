//! Desktop state and orchestration; widgets live in focused child modules.
use globecraft_core::layers::MapLayer;
#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod test_ui;
use eframe::egui::{self, Rect};
use globecraft_core::{
    camera::Cam,
    language::{Lang, LanguagePreference},
    placemark::Places,
};
use globecraft_data::{
    boundaries::{BoundaryData, BoundaryUpdates, StatusLog, load_boundaries_async},
    search::{GeoResult, Suggestion},
    settings::{Settings, load_language_preference},
    tiles::{TileCache, TileWarmup},
    wayback::WaybackRelease,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
mod assistant;
mod cache;
mod commands;
mod controllers;
mod explorer;
mod files;
mod flat_earth;
const SKY_CLOCK_REPAINT_INTERVAL: Duration = Duration::from_secs(30);
mod fonts;
mod frame_timing;
mod globe;
mod live_geometry;
mod live_layers;
mod live_maps;
#[cfg(any(target_os = "macos", test))]
mod menu;
mod navigation;
mod overlays;
mod render_wake;
mod rendering;
mod roads;
mod ruler_history;
mod search;
mod traffic;
mod ui;
mod update_restart;
mod wayback;
pub(crate) use update_restart::RestartRequest;
const OVERVIEW_WARMUP_ALTITUDE_KM: f64 = 14_000.0;
const PAN_SPEED_POINTS_PER_SECOND: f64 = 350.0;
const BOUNDARY_POLL_INTERVAL: Duration = Duration::from_millis(300);

type SuggestionResponse = (String, Result<Vec<Suggestion>, globecraft_data::Error>);

pub(crate) struct App {
    /// egui's context, for commands that talk to the window (full screen, URLs, repaint).
    ctx: egui::Context,
    /// The UI scale the style was last built for.
    styled_scale: Option<f32>,
    assistant: assistant::AssistantState,
    command_search: Option<ui::palette::CommandSearch>,
    /// The ribbon shown over the globe while collapsed (a tab was clicked).
    ribbon_peek: bool,
    tab_strip_rect: Option<Rect>,
    /// The title last sent to the window, so it is only sent when it changes.
    window_title: String,
    /// Fly to the best result when a search completes.
    search_fly: bool,
    /// Focus the place search field on the next frame.
    focus_search: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<menu::NativeMenu>,
    runtime: globecraft_data::runtime::ServiceRuntime,
    updater: globecraft_data::updater::UpdateService,
    update_restart: update_restart::UpdateRestart,
    explorer: explorer::Explorer,
    flat_earth: flat_earth::FlatEarth,
    boundary_loader: globecraft_data::boundaries::BoundaryLoader,
    lang: Lang,
    fonts: fonts::FontService,
    controllers: controllers::Controllers,
    sky: globecraft_data::sky::SkyService,
    settings_ui: ui::settings::SettingsUi,
    language_preference: LanguagePreference,
    system_locale: String,
    cam: Cam,
    fly: Option<globecraft_core::flight::Flight>,
    cache: Arc<TileCache>,
    cache_controls: cache::CacheControls,
    tile_warmup: TileWarmup,
    boundaries: Arc<BoundaryData>,
    overlay_cache: overlays::OverlayCache,
    boundaries_rx: BoundaryUpdates,
    boundary_log: StatusLog,
    auto_rotate: bool,
    tomtom_key: String,
    live_vessels: globecraft_data::vessels::LiveVessels,
    live_satellites: globecraft_data::satellites::LiveSatellites,
    traffic: traffic::TrafficOverlay,
    roads: roads::RoadOverlay,
    live_maps: live_maps::LiveMaps,
    live_aircraft: globecraft_data::aircraft::LiveAircraft,
    aircraft_overlay: live_layers::AircraftOverlay,
    live_geometry: live_geometry::LiveOverlay,
    world_shapes: live_layers::WorldShapes,

    renderer: rendering::Renderer,
    scale_bar_cache: globecraft_render::scale_bar::ScaleBarCache,
    texture: Option<egui::TextureHandle>,
    image: Option<(usize, usize, Arc<Vec<u8>>)>,
    last_state: u64,
    wake: render_wake::RenderWakeup,
    layer_arrivals: overlays::LayerArrivals,
    last_render: Instant,
    last_interact: Instant,

    local_search: globecraft_data::search::LocalSearch,
    query: String,
    results: Vec<GeoResult>,
    search_rx: Option<mpsc::Receiver<Result<Vec<GeoResult>, globecraft_data::Error>>>,
    status: String,
    status_file: Option<files::FileStatus>,
    system_units: globecraft_core::units::DistanceUnits,
    us_locale: bool,
    altitude_editor: Option<ui::altitude::AltitudeEditor>,

    file_jobs: globecraft_data::files::FileJobs,
    file_results: mpsc::Receiver<globecraft_data::files::FileCompletion>,
    placemarks: Places,
    place_selection: Option<u64>,
    place_reveal_requested: bool,
    new_name: String,
    kml_path: String,
    kml_session: files::KmlSession,
    menu_pick: Option<(f64, f64)>,

    ruler: bool,
    ruler_reveal_requested: bool,
    ruler_drag: Option<globecraft_core::ruler::RulerPointId>,
    ruler_paths: globecraft_core::ruler::Ruler,
    ruler_point_editor: Option<ui::ruler::PointEditor>,
    cursor: Option<(f64, f64)>,

    settings: Settings,
    saved_settings: Settings,
    settings_writer: globecraft_data::settings::SettingsWriter,
    ui_scale_edit: f32, // slider value while dragging (committed on release)
    show_settings: bool,
    show_about: bool,
    show_shortcuts: bool,

    wayback_list: Vec<WaybackRelease>,
    wayback_idx: usize,
    wayback_rx: Option<mpsc::Receiver<Result<Vec<WaybackRelease>, globecraft_data::Error>>>,
    wayback_started: bool,
    wayback_err: Option<String>,

    suggestions: Vec<Suggestion>,
    remote_sugg: Vec<Suggestion>,
    sugg_open: bool,
    sugg_sel: Option<usize>,
    sugg_last_edit: Instant,
    sugg_sent: String,
    sugg_rx: Option<mpsc::Receiver<SuggestionResponse>>,

    kml_drop_rect: Rect,
    ring_drag: bool,
}

impl App {
    /// Whether a map layer is shown (its own switch; parents are checked by callers that care).
    pub(crate) fn layer(&self, layer: MapLayer) -> bool {
        match layer {
            // Refinements draw only with their parent.
            MapLayer::GridLabels | MapLayer::CinematicSky => {
                self.settings.visible_layers().effective(layer)
            }
            _ => self.settings.layer_visible(layer),
        }
    }
    fn distance_units(&self) -> globecraft_core::units::DistanceUnits {
        self.settings.distance_units.resolve(self.system_units)
    }

    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        restart: Arc<RestartRequest>,
    ) -> Self {
        let settings = restart.settings.lock().unwrap().take().unwrap_or(settings);
        // Every font family the interface names exists from the first frame; installed system
        // faces replace these once the font worker has loaded them.
        cc.egui_ctx.set_fonts(fonts::base_definitions());
        let runtime =
            globecraft_data::runtime::ServiceRuntime::new().expect("start network runtime");
        let language_preference = load_language_preference();
        let system_locale = sys_locale::get_locale().unwrap_or_default();
        let lang = language_preference.resolve(&system_locale);
        let boundary_log: StatusLog = Arc::new(Mutex::new(Vec::new()));
        let (boundaries_rx, boundary_loader) =
            load_boundaries_async(boundary_log.clone(), runtime.handle(), {
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            });
        let cache = TileCache::with_budgets(
            {
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            },
            runtime.handle(),
            globecraft_data::tiles::CacheBudgets {
                decoded_bytes: settings.cache.decoded_bytes(),
                disk_bytes: settings.cache.disk_bytes(),
            },
        );
        let renderer = rendering::Renderer::new(cache.clone(), cc.egui_ctx.clone());
        let (file_jobs, file_results) = globecraft_data::files::FileJobs::new(runtime.handle(), {
            let ctx = cc.egui_ctx.clone();
            move || ctx.request_repaint()
        });
        let local_search = globecraft_data::search::LocalSearch::new(runtime.handle(), {
            let ctx = cc.egui_ctx.clone();
            move || ctx.request_repaint()
        });
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            styled_scale: None,
            assistant: assistant::AssistantState::default(),
            command_search: None,
            ribbon_peek: false,
            tab_strip_rect: None,
            window_title: String::new(),
            search_fly: true,
            focus_search: false,
            updater: globecraft_data::updater::UpdateService::new(runtime.handle(), {
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            }),
            update_restart: update_restart::UpdateRestart::new(restart.clone()),
            #[cfg(target_os = "macos")]
            native_menu: None,
            explorer: explorer::Explorer::default(),
            flat_earth: flat_earth::FlatEarth::default(),
            lang,
            fonts: fonts::FontService::new(cc.egui_ctx.clone()),
            controllers: controllers::Controllers::new(),
            sky: globecraft_data::sky::SkyService::default(),
            settings_ui: ui::settings::SettingsUi::default(),
            language_preference,
            cam: settings.navigation.startup.camera(),
            fly: None,
            cache,
            renderer,
            scale_bar_cache: globecraft_render::scale_bar::ScaleBarCache::default(),
            tile_warmup: TileWarmup::startup_satellite(),
            cache_controls: cache::CacheControls::default(),
            boundaries: Arc::new(BoundaryData::default()),
            overlay_cache: overlays::OverlayCache::new(cc.egui_ctx.clone()),
            boundaries_rx: BoundaryUpdates::new(boundaries_rx, {
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            })
            .expect("start boundary preparation"),
            boundary_loader,
            boundary_log,
            auto_rotate: false,
            tomtom_key: std::env::var("GLOBECRAFT_TOMTOM_KEY").unwrap_or_default(),
            live_vessels: globecraft_data::vessels::LiveVessels::default(),
            live_satellites: globecraft_data::satellites::LiveSatellites::default(),
            traffic: traffic::TrafficOverlay::default(),
            roads: roads::RoadOverlay::default(),
            live_maps: live_maps::LiveMaps::default(),
            live_aircraft: globecraft_data::aircraft::LiveAircraft::new(runtime.handle(), {
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            }),
            aircraft_overlay: live_layers::AircraftOverlay::new(cc.egui_ctx.clone()),
            live_geometry: live_geometry::LiveOverlay::new(cc.egui_ctx.clone()),
            world_shapes: live_layers::WorldShapes::default(),
            texture: None,
            image: None,
            last_state: 0,
            wake: render_wake::RenderWakeup::default(),
            layer_arrivals: overlays::LayerArrivals::default(),
            last_render: Instant::now(),
            last_interact: Instant::now(),
            local_search,
            query: String::new(),
            results: vec![],
            search_rx: None,
            status: lang.text("app-ready-status").into(),
            status_file: None,
            system_units: globecraft_core::units::DistanceUnits::for_locale(&system_locale),
            us_locale: globecraft_core::units::is_us_locale(&system_locale),
            system_locale,
            altitude_editor: None,
            placemarks: Places::default(),
            place_selection: None,
            place_reveal_requested: false,
            file_jobs,
            file_results,
            new_name: String::new(),
            kml_path: "placemarks.kml".into(),
            kml_session: files::KmlSession::default(),
            menu_pick: None,
            ruler: false,
            ruler_reveal_requested: false,
            ruler_drag: None,
            ruler_paths: globecraft_core::ruler::Ruler::default(),
            ruler_point_editor: None,
            cursor: None,
            ui_scale_edit: settings.ui_scale,
            settings: settings.clone(),
            saved_settings: settings,
            settings_writer: globecraft_data::settings::SettingsWriter::new({
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            })
            .expect("start settings writer"),
            show_settings: false,
            show_about: false,
            show_shortcuts: false,
            wayback_list: vec![],
            wayback_idx: 0,
            wayback_rx: None,
            wayback_started: false,
            wayback_err: None,
            suggestions: vec![],
            remote_sugg: vec![],
            sugg_open: false,
            sugg_sel: None,
            sugg_last_edit: Instant::now(),
            sugg_sent: String::new(),
            sugg_rx: None,
            kml_drop_rect: Rect::NOTHING,
            ring_drag: false,
            runtime,
        };
        app.restore_kml_session();
        if let Some(error) = &app.update_restart.error {
            app.status = error.clone();
        }
        if let Some(version) = restart.installed_version.lock().unwrap().as_deref() {
            app.updater.resume_installed(version);
        }
        if let Some(path) = restart.planet.lock().unwrap().take() {
            app.explorer.request_open(path, &app.file_jobs);
        }
        #[cfg(target_os = "macos")]
        {
            app.with_native_menu(cc)
        }
        #[cfg(not(target_os = "macos"))]
        {
            app
        }
    }
}
impl eframe::App for App {
    fn on_exit(&mut self) {
        if self
            .update_restart
            .request
            .requested
            .load(std::sync::atomic::Ordering::Acquire)
            && let Err(error) = self.settings_writer.finish()
        {
            *self.update_restart.request.error.lock().unwrap() = Some(format!(
                "Settings save before restart failed: {error}. Click Restart to update to retry."
            ));
        }
        #[cfg(feature = "profiling")]
        globecraft_data::profiling::flush();
    }
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.fonts.poll(
            ctx,
            &self.settings.appearance.ui_font,
            &self.settings.appearance.map_font,
        ) {
            self.invalidate_font_layout();
        }
        let sky_wake = ctx.clone();
        self.sky.update(
            self.runtime.handle(),
            self.settings.sky.enabled,
            move || sky_wake.request_repaint(),
        );
        if self.settings.sky.enabled && self.sky.image.is_some() {
            ctx.request_repaint_after(SKY_CLOCK_REPAINT_INTERVAL);
        }
        self.updater.start_once();
        self.updater.poll();
        let aircraft_enabled = self.layer(MapLayer::Flights)
            && self.explorer.active == globecraft_core::body::Body::Earth
            && !self.flat_earth.open;
        self.live_aircraft
            .update(globecraft_data::aircraft::AircraftQuery {
                enabled: aircraft_enabled,
                latitude: self.cam.lat,
                longitude: self.cam.lon,
                altitude_km: self.cam.alt,
            });
        if aircraft_enabled {
            ctx.request_repaint_after(live_layers::FLIGHT_REPAINT_INTERVAL);
        }
        // Independent realtime layers, loaded only for the globe while enabled.
        let earth =
            self.explorer.active == globecraft_core::body::Body::Earth && !self.flat_earth.open;
        let vessel_wake = ctx.clone();
        self.live_vessels.update(
            &self.runtime,
            earth && self.layer(MapLayer::Vessels),
            move || vessel_wake.request_repaint(),
        );
        let satellite_wake = ctx.clone();
        self.live_satellites.update(
            &self.runtime,
            earth && self.layer(MapLayer::Satellites),
            move || satellite_wake.request_repaint(),
        );
        let road_wake = ctx.clone();
        self.roads.service.update(
            &self.runtime,
            earth && self.layer(MapLayer::Roads),
            self.cam.lat,
            self.cam.lon,
            self.cam.alt,
            move || road_wake.request_repaint(),
        );
        let traffic_wake = ctx.clone();
        self.traffic.service.update(
            &self.runtime,
            globecraft_data::traffic::TrafficQuery {
                enabled: earth && self.layer(MapLayer::Traffic),
                key: &self.tomtom_key,
                latitude: self.cam.lat,
                longitude: self.cam.lon,
                altitude_km: self.cam.alt,
            },
            move || traffic_wake.request_repaint(),
        );
        if earth
            && (self.layer(MapLayer::Vessels)
                || self.layer(MapLayer::Satellites)
                || self.layer(MapLayer::Roads)
                || self.layer(MapLayer::Traffic))
        {
            ctx.request_repaint_after(Duration::from_secs(30));
        }
        self.poll_files(ctx);
        self.poll_assistant();
        self.poll_update_restart(ctx);
        self.ensure_wayback(ctx);
        self.poll_wayback();
        self.poll_suggestions(ctx);
        if let Some(error) = self.local_search.take_error() {
            self.status = format!("Local prefix index failed: {error}");
        }
        self.poll_search();
        self.poll_cache_clear(ctx);
        if let Some(snapshot) = self.boundaries_rx.take_latest() {
            self.merge_snapshot(snapshot);
        }
        if self.boundary_loader.is_loading() {
            ctx.request_repaint_after(BOUNDARY_POLL_INTERVAL);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(feature = "profiling")]
        let mut profile = globecraft_data::profiling::Span::new("ui.update");
        ui.set_theme(match self.settings.appearance.theme {
            globecraft_data::settings::appearance::ThemePreference::System => {
                egui::ThemePreference::System
            }
            globecraft_data::settings::appearance::ThemePreference::Dark => {
                egui::ThemePreference::Dark
            }
            globecraft_data::settings::appearance::ThemePreference::Light => {
                egui::ThemePreference::Light
            }
        });
        let dt = frame_timing::animation_delta_secs(ui);
        if self
            .styled_scale
            .is_none_or(|scale| (self.settings.ui_scale - scale).abs() > 1e-4)
        {
            ui::theme::apply(ui, self.settings.ui_scale);
            self.styled_scale = Some(self.settings.ui_scale);
        }
        // persist settings once a slider is released
        if self.settings != self.saved_settings && !ui.input(|i| i.pointer.any_down()) {
            self.settings_writer.settings(&self.settings);
            self.saved_settings = self.settings.clone();
        }
        if let Some(error) = self.settings_writer.take_error() {
            self.status = format!("Settings persistence failed: {error}");
        }
        if self.update_restart.is_saving() {
            ui.disable();
        } else {
            #[cfg(target_os = "macos")]
            self.poll_native_menu(ui);
            self.command_shortcuts(ui);
            self.ruler_shortcuts(ui);
            self.flat_earth_shortcuts(ui.ctx());
            self.handle_dropped_files(ui);
        }
        self.step_fly(dt);
        if self.fly.is_some() {
            ui.request_repaint();
        }
        self.header(ui);
        self.ribbon(ui);
        self.status_bar(ui);
        self.left_pane(ui);
        self.right_pane(ui);
        self.settings_window(ui);
        self.menu_windows(ui);
        self.explorer_window(ui);
        self.altitude_window(ui);
        self.ruler_points_window(ui);
        self.command_search_window(ui);
        self.live_maps.update(
            ui,
            &self.runtime,
            self.layer(MapLayer::Clouds),
            self.layer(MapLayer::Radar),
        );
        let earth_visible =
            self.explorer.active == globecraft_core::body::Body::Earth && !self.flat_earth.open;
        let country = self
            .boundaries
            .country_at(self.cam.lat, self.cam.lon)
            .map(str::to_owned);
        let is_brazil = earth_visible && country.as_deref() == Some("BRA");
        self.boundary_loader.request_municipal(
            globecraft_data::boundaries::MunicipalView {
                enabled: self.layer(MapLayer::MunicipalBorders) && earth_visible,
                altitude_km: self.cam.alt,
                lat: self.cam.lat,
                lon: self.cam.lon,
                countries: &self.settings.municipal_countries,
            },
            &self.boundaries,
        );
        // Brazil's municipal outlines stream per state like every other country;
        // this whole-country pass only supplies names.
        self.boundary_loader.request_visible_layers(
            false,
            self.layer(MapLayer::PlaceLabels) && is_brazil,
            self.cam.alt,
        );
        self.boundary_loader.request_world_detail(self.cam.alt);
        if earth_visible
            && (self.layer(MapLayer::StateBorders) || self.layer(MapLayer::PlaceLabels))
        {
            self.boundary_loader
                .request_states(self.cam.alt, self.settings.state_max_altitude_km);
        }
        if self.boundary_loader.is_loading() {
            ui.request_repaint_after(BOUNDARY_POLL_INTERVAL);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ui::theme::Tokens::get(ui).canvas))
            .show(ui, |ui| {
                if self.flat_earth.open {
                    let ctx = ui.ctx().clone();
                    self.flat_earth_view(ui, &ctx);
                } else {
                    self.globe_view(ui);
                }
            });
        #[cfg(target_os = "macos")]
        self.refresh_native_menu(ui);
        #[cfg(feature = "profiling")]
        {
            profile.values.inflight = self.cache.inflight();
            profile.values.resident_bytes = self.cache.resident_bytes();
        }
    }
}
