//! Desktop state and orchestration; widgets live in focused child modules.
use globecraft_core::layers::MapLayer;

/// The window title. The titlebar text stays hidden in the merged header, but the
/// system still uses this value for Mission Control, the taskbar and window lists.
pub(crate) const WINDOW_TITLE: &str = "GlobeCraft";
/// Used when macOS cannot report the native traffic-light bounds. AppKit points.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHTS_WIDTH_FALLBACK_POINTS: f32 = 78.0;
#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod test_ui;
use eframe::egui::{self, Rect};
use globecraft_core::{
    camera::Cam,
    language::{Lang, LanguagePreference},
};
use globecraft_data::{
    boundaries::{BoundaryData, BoundaryUpdates, StatusLog, load_boundaries_async},
    search::Suggestion,
    settings::{Settings, load_language_preference},
    tiles::TileCache,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
mod antarctica;
mod assistant;
mod borders;
mod cache;
mod commands;
mod controllers;
mod credits;
mod diagnostics;
mod explorer;
mod files;
mod flat_earth;
mod fonts;
mod frame_timing;
mod globe;
mod here;
mod imagery;
mod layer_solo;
mod legend;
mod live_geometry;
mod live_layers;
mod live_maps;
mod live_services;
mod long_press;
mod marine_names;
mod measure;
#[cfg(any(target_os = "macos", test))]
mod menu;
mod navigation;
mod overlays;
mod places;
mod render_wake;
mod rendering;
mod road_names;
mod roads;
mod ruler_history;
mod ruler_profile;
mod search;
mod selected_flight;
mod selected_satellite;
mod selection;
mod still_shot;
mod time;
mod timezones;
mod tools;
mod traffic;
mod ui;
mod update_restart;
mod user_layers;
mod view_links;
mod wayback;
mod windows;
pub(crate) use update_restart::RestartRequest;

const OVERVIEW_WARMUP_ALTITUDE_KM: f64 = 14_000.0;
const PAN_SPEED_POINTS_PER_SECOND: f64 = 350.0;
const BOUNDARY_POLL_INTERVAL: Duration = Duration::from_millis(300);
/// How often the whole settings tree is compared with the saved copy. Settings change
/// by hand, so a few checks a second save promptly without a deep compare every frame.
const SETTINGS_CHECK_INTERVAL: Duration = Duration::from_millis(250);
/// State lines (which need border tiles of zoom 5 or finer) count as showing below this
/// share of their threshold altitude, where their fade-in reaches about 6 %.
const STATE_TILES_FROM_RATIO: f64 = 0.85;
/// Just after the scene's minute turns over (see [`clock_wake`]).
const CLOCK_WAKE_MARGIN_SECS: f64 = 0.05;

/// How long until the wall clock reaches the next scene minute, when something on
/// screen shows it.
fn clock_wake(now_unix: f64, shows_clock: bool) -> Option<Duration> {
    let minute = globecraft_render::view::SCENE_TIME_UPDATE_INTERVAL_SECS;
    shows_clock.then(|| {
        Duration::from_secs_f64(minute - now_unix.rem_euclid(minute) + CLOCK_WAKE_MARGIN_SECS)
    })
}

/// egui's theme for the saved choice.
fn egui_theme(
    theme: globecraft_data::settings::appearance::ThemePreference,
) -> egui::ThemePreference {
    use globecraft_data::settings::appearance::ThemePreference;
    match theme {
        ThemePreference::System => egui::ThemePreference::System,
        ThemePreference::Dark => egui::ThemePreference::Dark,
        ThemePreference::Light => egui::ThemePreference::Light,
    }
}

type SuggestionResponse = (String, Result<Vec<Suggestion>, globecraft_data::Error>);

pub(crate) struct App {
    /// egui's context, for commands that talk to the window (full screen, URLs, repaint).
    ctx: egui::Context,
    /// The UI scale the style was last built for.
    styled_scale: Option<f32>,
    assistant: assistant::AssistantState,
    command_search: Option<ui::palette::CommandSearch>,
    /// egui's font atlas fill at the start of the last frame (see `ui`).
    font_atlas_fill: f32,
    /// The ribbon group being drawn, and the shortcut dialog.
    ribbon_tools: ui::ribbon_tools::RibbonTools,
    /// The ribbon shown over the globe while collapsed (a tab was clicked).
    ribbon_peek: bool,
    tab_strip_rect: Option<Rect>,
    /// The native macOS traffic-light width, in AppKit points. Divide by egui's
    /// zoom factor to reserve the same room in egui points.
    #[cfg(target_os = "macos")]
    traffic_lights_width_native: f32,
    #[cfg(target_os = "macos")]
    native_menu: Option<menu::NativeMenu>,
    runtime: globecraft_data::runtime::ServiceRuntime,
    updater: globecraft_data::updater::UpdateService,
    update_restart: update_restart::UpdateRestart,
    explorer: explorer::Explorer,
    flat_earth: flat_earth::FlatEarth,
    lang: Lang,
    fonts: fonts::FontService,
    controllers: controllers::Controllers,
    sky: globecraft_data::sky::SkyService,
    settings_ui: ui::settings::SettingsUi,
    layers_ui: ui::layers_pane::LayersUi,
    language_preference: LanguagePreference,
    system_locale: String,
    cam: Cam,
    fly: Option<globecraft_core::flight::Flight>,
    cache: Arc<TileCache>,
    cache_controls: cache::CacheControls,
    auto_rotate: bool,
    /// Keys of the keyed services as typed in Settings -> Keys (the TomTom key is also
    /// typed in the Layers pane).
    service_keys: std::collections::BTreeMap<&'static str, String>,
    /// The borders and names: loading, the drawn set and its painted overlay.
    borders: borders::Borders,
    /// What is picked: a flight, a satellite, the What's here? card.
    selection: selection::Selection,
    /// The live data services that follow the view (see `live_services`).
    live: live_services::LiveFeeds,
    transport_shapes: live_layers::TransportShapes,
    poi_shapes: live_layers::PoiShapes,
    /// The layers the user added: their loading and drawing.
    user_layers: user_layers::UserLayers,
    /// The Add Data window, while open.
    add_data: Option<ui::add_data::AddDataDialog>,
    /// Press and hold on the globe opens the card (touch screens and trackpads).
    long_press: long_press::LongPress,
    /// Where the sea and undersea names went this frame.
    marine_placed: globecraft_render::overlays::PlacedLabels,
    time_zone_shapes: timezones::TimeZoneShapes,
    aurora_shapes: live_layers::AuroraShapes,
    traffic: traffic::TrafficOverlay,
    roads: roads::RoadOverlay,
    live_maps: live_maps::LiveMaps,
    aircraft_overlay: live_layers::AircraftOverlay,
    /// The colour keys window.
    legend: legend::Legend,
    live_geometry: live_geometry::LiveOverlay,
    world_shapes: live_layers::WorldShapes,

    /// The globe picture: the renderer, the frame shown and fading, the warmups.
    imagery: imagery::Imagery,
    scale_bar_cache: globecraft_render::scale_bar::ScaleBarCache,
    /// The streaming diagnostics overlay shows (`ui.tileDiagnostics`; not saved).
    tile_diagnostics: bool,
    download_meter: ui::tile_diagnostics::DownloadMeter,
    layer_arrivals: overlays::LayerArrivals,
    last_interact: Instant,

    status: String,
    status_file: Option<files::FileStatus>,
    system_units: globecraft_core::units::DistanceUnits,
    us_locale: bool,
    altitude_editor: Option<ui::altitude::AltitudeEditor>,

    file_jobs: globecraft_data::files::FileJobs,
    file_results: mpsc::Receiver<globecraft_data::files::FileCompletion>,
    /// The places and their KML file.
    places: places::PlacesState,
    /// The timeline bar.
    timeline: time::TimelineState,
    /// Which of the Settings, About and Shortcuts windows are open.
    windows: windows::WindowsState,
    /// The navigator's drags and eased turns.
    nav: navigation::NavState,
    /// A `globecraft://view` link from the command line, opened on the first frame.
    pending_view_link: Option<String>,
    /// The layer shown alone (`layer.solo`), and what was visible before.
    solo: Option<layer_solo::Solo>,

    cursor: Option<(f64, f64)>,

    /// The place search box: query, results and suggestions.
    search: search::SearchState,
    settings: Settings,
    saved_settings: Settings,
    /// When `settings` was last compared with `saved_settings` (see `persist_settings`).
    settings_checked: Instant,
    settings_writer: globecraft_data::settings::SettingsWriter,
    /// The ruler.
    measure: measure::MeasureState,
    /// Esri Wayback: the releases, the one shown and the swipe compare.
    wayback: wayback::WaybackState,

    /// The app's time: live, a moment, or playing (roadmap 4.H).
    pub(super) time: globecraft_core::time::AppTime,

    /// A high-quality screenshot rendering in the background.
    still_shot: Option<still_shot::StillShot>,
    /// The terrain height (km, exaggerated) the camera looks at, eased toward the
    /// elevation tiles so a finer tile arriving does not jolt the view.
    ground_km: f64,
    /// When the download totals were last written to the run log, and what they were.
    download_log: cache::DownloadLog,
}

impl App {
    /// Whether the Earth is on screen as a globe (not another body, not the flat map).
    pub(super) fn earth_shown(&self) -> bool {
        self.explorer.on_earth() && !self.flat_earth.open
    }
    /// Whether any layer draws from the border tiles (borders or place names).
    pub(super) fn uses_border_tiles(&self) -> bool {
        self.layer(MapLayer::CountryBorders)
            || self.layer(MapLayer::StateBorders)
            || self.layer(MapLayer::MunicipalBorders)
            || self.layer(MapLayer::PlaceLabels)
    }
    /// Whether a shown web data layer reloads on a timer (its refresh is checked
    /// only when the interface runs).
    fn user_layers_refresh(&self) -> bool {
        self.settings.user_layers.iter().any(|layer| {
            layer.visible
                && matches!(
                    layer.source,
                    globecraft_data::user_data::UserSource::Features { refresh_minutes, .. }
                        if refresh_minutes > 0
                )
        })
    }
    /// Whether a map layer is shown: its own switch and, for a refinement (grid labels),
    /// its parent's too.
    pub(crate) fn layer(&self, layer: MapLayer) -> bool {
        self.settings.layer_visible(layer)
            && layer
                .parent()
                .is_none_or(|parent| self.settings.layer_visible(parent))
    }
    fn distance_units(&self) -> globecraft_core::units::DistanceUnits {
        self.settings.distance_units.resolve(self.system_units)
    }

    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        restart: Arc<RestartRequest>,
    ) -> Self {
        let settings = restart
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or(settings);
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
        let renderer =
            rendering::Renderer::new(cache.clone(), cc.egui_ctx.clone(), settings.gpu_globe);
        let overlay_cache = overlays::OverlayCache::new(cc.egui_ctx.clone(), Some(cache.clone()));
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
            ribbon_tools: Default::default(),
            font_atlas_fill: 0.0,
            ribbon_peek: false,
            tab_strip_rect: None,
            #[cfg(target_os = "macos")]
            traffic_lights_width_native: {
                use raw_window_handle::HasWindowHandle as _;
                cc.window_handle()
                    .ok()
                    .and_then(|handle| {
                        eframe::WindowChromeMetrics::from_window_handle(&handle.as_raw())
                    })
                    .map(|metrics| metrics.traffic_lights_size.x)
                    .filter(|width| *width > 0.0)
                    .unwrap_or(TRAFFIC_LIGHTS_WIDTH_FALLBACK_POINTS)
            },
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
            layers_ui: ui::layers_pane::LayersUi::default(),
            language_preference,
            cam: settings.navigation.startup.camera(),
            fly: None,
            cache: cache.clone(),
            imagery: imagery::Imagery::new(renderer),
            scale_bar_cache: globecraft_render::scale_bar::ScaleBarCache::default(),
            cache_controls: cache::CacheControls::default(),
            borders: borders::Borders {
                drawn: Arc::new(BoundaryData::default()),
                source: Arc::new(BoundaryData::default()),
                place_names: Default::default(),
                place_names_shown: None,
                overlay_cache,
                updates: BoundaryUpdates::new(boundaries_rx, {
                    let ctx = cc.egui_ctx.clone();
                    move || ctx.request_repaint()
                })
                .expect("start boundary preparation"),
                loader: boundary_loader,
                log: boundary_log,
            },
            auto_rotate: false,
            service_keys: globecraft_data::keys::SERVICES
                .iter()
                .map(|service| {
                    (
                        service.id,
                        globecraft_data::keys::get(service.id).unwrap_or_default(),
                    )
                })
                .collect(),
            live: live_services::LiveFeeds::new(&runtime, &cc.egui_ctx),
            selection: selection::Selection::default(),
            transport_shapes: live_layers::TransportShapes::default(),
            poi_shapes: live_layers::PoiShapes::default(),
            user_layers: Default::default(),
            add_data: None,
            long_press: long_press::LongPress::default(),
            marine_placed: Default::default(),
            time_zone_shapes: timezones::TimeZoneShapes::default(),
            aurora_shapes: live_layers::AuroraShapes::default(),
            traffic: traffic::TrafficOverlay::default(),
            roads: roads::RoadOverlay::default(),
            live_maps: live_maps::LiveMaps::default(),
            aircraft_overlay: live_layers::AircraftOverlay::new(cc.egui_ctx.clone()),
            legend: Default::default(),
            live_geometry: live_geometry::LiveOverlay::new(
                cc.egui_ctx.clone(),
                Some(cache.clone()),
            ),
            world_shapes: live_layers::WorldShapes::default(),
            tile_diagnostics: false,
            download_meter: Default::default(),
            layer_arrivals: overlays::LayerArrivals::default(),
            last_interact: Instant::now(),
            search: search::SearchState::new(local_search),
            status: lang.text("app-ready-status").into(),
            status_file: None,
            system_units: globecraft_core::units::DistanceUnits::for_locale(&system_locale),
            us_locale: globecraft_core::units::is_us_locale(&system_locale),
            system_locale,
            altitude_editor: None,
            file_jobs,
            file_results,
            places: places::PlacesState::default(),
            timeline: time::TimelineState::default(),
            windows: windows::WindowsState::new(settings.ui_scale),
            nav: navigation::NavState::default(),
            pending_view_link: view_links::link_from_args(),
            solo: None,
            cursor: None,
            settings: settings.clone(),
            saved_settings: settings,
            settings_checked: Instant::now(),
            settings_writer: globecraft_data::settings::SettingsWriter::new({
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            })
            .expect("start settings writer"),
            measure: measure::MeasureState::default(),
            wayback: wayback::WaybackState::default(),
            time: globecraft_core::time::AppTime::default(),
            still_shot: None,
            ground_km: 0.0,
            download_log: cache::DownloadLog {
                at: Instant::now(),
                bytes: 0,
            },
            runtime,
        };
        app.restore_kml_session();
        if let Some(error) = &app.update_restart.error {
            app.status = error.clone();
        }
        if let Some(problem) = restart
            .load_problem
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            app.status = problem;
        }
        if let Some(version) = restart
            .installed_version
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref()
        {
            app.updater.resume_installed(version);
        }
        if let Some(path) = restart
            .planet
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
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
/// How long a quit waits for a running screenshot, a file save and tile cache writes
/// before the process ends (macOS ends it itself, see `on_exit`).
#[cfg(target_os = "macos")]
const QUIT_SCREENSHOT_WAIT: std::time::Duration = std::time::Duration::from_secs(20);
#[cfg(target_os = "macos")]
const QUIT_FILE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(target_os = "macos")]
const QUIT_DISK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

impl App {
    /// Lets work that a quit would cut short land first: a screenshot being rendered
    /// or written, a KML or planet file being saved, and tile cache writes. Each wait
    /// is bounded, so a stuck job cannot keep the app from quitting.
    #[cfg(target_os = "macos")]
    fn finish_background_work(&mut self) {
        use std::time::{Duration, Instant};
        let started = Instant::now();
        while self.screenshot_running() && started.elapsed() < QUIT_SCREENSHOT_WAIT {
            self.poll_still_shot();
            std::thread::sleep(Duration::from_millis(50));
        }
        let started = Instant::now();
        while self.file_jobs.is_busy() && started.elapsed() < QUIT_FILE_WAIT {
            std::thread::sleep(Duration::from_millis(20));
        }
        if !self.cache.wait_for_disk_writes(QUIT_DISK_WAIT) {
            globecraft_data::log_line!("Quit before every tile reached the disk cache");
        }
    }
}

impl App {
    /// Queues the settings for writing when they differ from what was last queued.
    fn persist_settings(&mut self) {
        self.settings_checked = Instant::now();
        if self.settings != self.saved_settings {
            // However the setting changed (a command, Settings, an import), requests follow.
            globecraft_data::network::set_offline(self.settings.offline);
            self.settings_writer.settings(&self.settings);
            self.saved_settings = self.settings.clone();
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let restarting = self
            .update_restart
            .request
            .requested
            .load(std::sync::atomic::Ordering::Acquire);
        self.persist_settings();
        match self.settings_writer.finish() {
            Ok(()) => {}
            Err(error) if restarting => {
                *self
                    .update_restart
                    .request
                    .error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!(
                    "Settings save before restart failed: {error}. Click Restart to update to retry."
                ));
            }
            Err(error) => {
                globecraft_data::log_line!("Settings persistence failed during shutdown: {error}");
            }
        }
        #[cfg(feature = "profiling")]
        globecraft_data::profiling::flush();
        #[cfg(target_os = "macos")]
        self.finish_background_work();
        // macOS: end the process here, before eframe tears down the window. AccessKit's
        // adapter restores the content view's class on teardown while AppKit's Touch Bar
        // support still observes that view; the next display cycle then throws
        // "not registered as an observer" and every quit ends in SIGABRT. Settings and
        // profiling data are already flushed above. A pending update restart must exec
        // the replacement here too, because returning to eframe's teardown aborts before
        // `main` can do it. Running screenshots, file saves and tile writes are given a
        // bounded time to finish first. On other platforms eframe returns and `main`
        // finishes normally.
        #[cfg(target_os = "macos")]
        if restarting {
            if let Some(error) = self
                .update_restart
                .request
                .error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                globecraft_data::log_line!(
                    "Update restart stopped: {error}. Relaunch GlobeCraft manually."
                );
                std::process::exit(1);
            }
            globecraft_data::log_line!("Restarting GlobeCraft to finish the update");
            match self_update::restart::restart() {
                Ok(never) => match never {},
                Err(error) => {
                    globecraft_data::log_line!(
                        "Restart failed: {error}. Relaunch GlobeCraft manually."
                    );
                    std::process::exit(1);
                }
            }
        }
        #[cfg(target_os = "macos")]
        if !restarting {
            std::process::exit(0);
        }
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
        // Whatever shows the wall-clock time moves on with the minute: the sun and
        // the sky, the timeline's clock, the local time under the cursor. One wake
        // just after the next minute, and none while nothing shows it (roadmap 14.6).
        let shows_clock = self.time.is_live()
            && (self.layer(MapLayer::Sunlight)
                || self.sky_ready()
                || self.timeline.open
                || (self.layer(MapLayer::TimeZones) && self.cursor.is_some()));
        if let Some(after) = clock_wake(globecraft_core::math::now_unix(), shows_clock) {
            ctx.request_repaint_after(after);
        }
        // A view link from the command line opens once the window is up.
        if let Some(link) = self.pending_view_link.take()
            && let Err(error) = self.run("view.openLink", serde_json::json!({"link": link}))
        {
            self.status = error;
        }
        self.updater.start_once();
        self.updater.poll();
        self.update_live_services(ctx);
        self.poll_files(ctx);
        self.poll_assistant();
        self.poll_update_restart(ctx);
        self.ensure_wayback(ctx);
        self.poll_wayback();
        self.wayback_follow_time();
        self.poll_suggestions(ctx);
        if let Some(error) = self.search.local.take_error() {
            self.status = format!("Local prefix index failed: {error}");
        }
        self.poll_search();
        self.poll_cache_clear(ctx);
        if let Some(snapshot) = self.borders.updates.take_latest() {
            self.merge_snapshot(snapshot);
        }
        if self.borders.loader.is_loading() {
            ctx.request_repaint_after(BOUNDARY_POLL_INTERVAL);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(feature = "profiling")]
        let mut profile = globecraft_data::profiling::Span::new("ui.update");
        #[cfg(feature = "profiling")]
        let mut watchdog = globecraft_data::profiling::PassWatchdog::new("ui.slow");
        profile_checkpoint!(watchdog, "ui.slow.setup");
        // egui recreates its font atlas when it fills up, before the frame: the atlas is
        // then emptier than at the last frame's start. Label shapes kept across frames
        // must be laid out again, or they show other glyphs.
        let atlas_fill = ui.ctx().fonts(|fonts| fonts.font_atlas_fill_ratio());
        if atlas_fill < self.font_atlas_fill {
            self.borders.overlay_cache.font_atlas_reset();
        }
        self.font_atlas_fill = atlas_fill;
        ui.set_theme(egui_theme(self.settings.appearance.theme));
        let dt = frame_timing::animation_delta_secs(ui);
        if self
            .styled_scale
            .is_none_or(|scale| (self.settings.ui_scale - scale).abs() > 1e-4)
        {
            ui::theme::apply(ui, self.settings.ui_scale);
            self.styled_scale = Some(self.settings.ui_scale);
        }
        // persist settings once a slider is released; frames with no input (animation)
        // check at most every SETTINGS_CHECK_INTERVAL, and quitting checks once more
        let (pointer_down, had_input) = ui.input(|i| (i.pointer.any_down(), !i.events.is_empty()));
        if !pointer_down
            && (had_input || self.settings_checked.elapsed() >= SETTINGS_CHECK_INTERVAL)
        {
            self.persist_settings();
        }
        if let Some(error) = self.settings_writer.take_error() {
            self.status = format!("Settings persistence failed: {error}");
        }
        profile_checkpoint!(watchdog, "ui.slow.input");
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
        profile_checkpoint!(watchdog, "ui.slow.header");
        self.header(ui);
        profile_checkpoint!(watchdog, "ui.slow.ribbon");
        self.ribbon(ui);
        profile_checkpoint!(watchdog, "ui.slow.status");
        self.status_bar(ui);
        profile_checkpoint!(watchdog, "ui.slow.panes");
        self.left_pane(ui);
        self.right_pane(ui);
        profile_checkpoint!(watchdog, "ui.slow.windows");
        self.settings_window(ui);
        self.menu_windows(ui);
        self.altitude_window(ui);
        self.add_data_window(ui);
        self.command_search_window(ui);
        self.shortcut_dialog(ui);
        ui::focus::paint_focus_ring(ui.ctx());
        profile_checkpoint!(watchdog, "ui.slow.live_maps");
        self.live_maps.update(
            ui,
            &self.runtime,
            self.layer(MapLayer::Clouds),
            self.layer(MapLayer::Radar),
        );
        profile_checkpoint!(watchdog, "ui.slow.boundary");
        let earth_visible = self.earth_shown();
        // The vector sets (transport, roads, places, points of interest) keep their
        // tiles on disk under the same limit as the border tiles, each.
        globecraft_data::vector_tiles::set_disk_cache_limit(self.settings.cache.border_bytes());
        // Borders and names stream from OpenStreetMap tiles around the view while any
        // border or name layer shows.
        self.borders
            .loader
            .request_osm_tiles(globecraft_data::boundaries::OsmTileView {
                enabled: earth_visible && self.uses_border_tiles(),
                lat: self.cam.lat,
                lon: self.cam.lon,
                altitude_km: self.cam.alt,
                tilt_deg: self.cam.tilt,
                cache_limit_bytes: self.settings.cache.border_bytes(),
                load_ahead: self.imagery.renderer.is_idle()
                    && !self.imagery.renderer.waits_for_tiles(),
                // From where the state lines' fade-in is past its first few percent.
                states_visible: self.layer(MapLayer::StateBorders)
                    && self.cam.alt
                        < STATE_TILES_FROM_RATIO
                            * self.settings.state_max_altitude_km.max(
                                globecraft_data::boundaries::STATE_BIG_VISIBLE_MAX_ALTITUDE_KM,
                            ),
            });
        if self.borders.loader.is_loading() {
            ui.request_repaint_after(BOUNDARY_POLL_INTERVAL);
        }
        self.log_downloads();
        self.poll_still_shot();
        self.follow_ground(ui);
        profile_checkpoint!(watchdog, "ui.slow.globe");
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
        profile_checkpoint!(watchdog, "ui.slow.native_menu");
        #[cfg(target_os = "macos")]
        self.refresh_native_menu(ui);
        #[cfg(feature = "profiling")]
        {
            let viewport = ui.ctx().input(|input| input.viewport_rect());
            let pixels_per_point = ui.ctx().pixels_per_point();
            profile.values.width = (viewport.width() * pixels_per_point).round().max(0.0) as usize;
            profile.values.height =
                (viewport.height() * pixels_per_point).round().max(0.0) as usize;
            profile.values.inflight = self.cache.inflight();
            profile.values.resident_bytes = self.cache.resident_bytes();
            watchdog.finish(profile.values);
        }
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    #[test]
    fn the_clock_wakes_just_after_the_next_minute_and_only_when_shown() {
        assert_eq!(clock_wake(120.0, false), None);
        let after = clock_wake(125.5, true).unwrap().as_secs_f64();
        assert!(
            (after - (54.5 + CLOCK_WAKE_MARGIN_SECS)).abs() < 1e-9,
            "{after}"
        );
        let at_turn = clock_wake(180.0, true).unwrap().as_secs_f64();
        assert!((at_turn - (60.0 + CLOCK_WAKE_MARGIN_SECS)).abs() < 1e-9);
    }
}
