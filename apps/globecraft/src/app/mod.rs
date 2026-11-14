//! Desktop state and orchestration; widgets live in focused child modules.
use globecraft_core::layers::MapLayer;

/// The window title. The titlebar text stays hidden in the merged header, but the
/// system still uses this value for Mission Control, the taskbar and window lists.
pub(crate) const WINDOW_TITLE: &str = "GlobeCraft";
#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod test_ui;
use eframe::egui;
use globecraft_core::{camera::Cam, language::Lang};
use globecraft_data::{
    boundaries::{BoundaryUpdates, StatusLog, load_boundaries_async},
    search::Suggestion,
    settings::Settings,
    tiles::TileCache,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
mod antarctica;
mod assistant;
mod borders;
mod cache;
mod chrome;
mod commands;
mod controllers;
mod credits;
mod explorer;
mod files;
mod flat_earth;
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
mod fonts;
mod frame;
mod frame_timing;
mod globe;
mod here;
mod keys;
mod layer_solo;
mod legend;
mod live;
mod live_geometry;
mod live_layers;
mod live_maps;
mod locale;
mod long_press;
mod map_data;
mod marine_names;
mod measure;
#[cfg(any(target_os = "macos", test))]
mod menu;
mod motion;
mod navigation;
mod overlays;
mod panes;
mod picked;
mod places;
mod pointer;
mod render_wake;
mod rendering;
mod road_names;
mod roads;
mod ruler_history;
mod ruler_profile;
mod saved;
mod search;
mod selected_flight;
mod selected_satellite;
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

type SuggestionResponse = (String, Result<Vec<Suggestion>, globecraft_data::Error>);

pub(crate) struct App {
    /// egui's context, for commands that talk to the window (full screen, URLs, repaint).
    ctx: egui::Context,
    /// The header and the ribbon.
    chrome: chrome::ChromeState,
    assistant: assistant::AssistantState,
    runtime: globecraft_data::runtime::ServiceRuntime,
    updates: update_restart::UpdatesState,
    explorer: explorer::Explorer,
    flat_earth: flat_earth::FlatEarth,
    /// Borders and place names: their data, loading and overlay geometry.
    borders: borders::BordersState,
    /// The language in use; the choice behind it is in `locale`.
    lang: Lang,
    fonts: fonts::FontService,
    controllers: controllers::Controllers,
    sky: globecraft_data::sky::SkyService,
    /// What the Settings pages, the Layers pane and the colour keys show.
    panes: panes::PanesState,
    locale: locale::LocaleState,
    /// Where the camera is; how it is moving is in `motion`.
    cam: Cam,
    motion: motion::MotionState,
    /// The imagery cache, its controls and the tiles loaded ahead.
    tiles: cache::TilesState,
    keys: keys::KeysState,
    /// The realtime layers: aircraft, vessels, satellites, aurora, weather, traffic.
    live: live::LiveState,
    /// Roads, transport and places of interest.
    map_data: map_data::MapDataState,
    /// The layers the user added: their loading and drawing.
    user_layers: user_layers::UserLayers,
    /// The pointer on the globe, and the "What's here?" card.
    pointer: pointer::PointerState,
    /// The flight and the satellite picked on the globe.
    picked: picked::PickedState,
    /// The globe's picture: renderer, displayed frame, screenshots.
    frame: frame::FrameState,

    status: String,
    files: files::FilesState,
    /// The places and their KML file.
    places: places::PlacesState,
    /// The timeline bar.
    timeline: time::TimelineState,
    /// Which windows and dialogs are open.
    windows: windows::WindowsState,
    /// The navigator's drags and eased turns.
    nav: navigation::NavState,
    /// The layer shown alone (`layer.solo`), and what was visible before.
    solo: Option<(
        globecraft_core::layers::MapLayer,
        globecraft_core::layers::LayerSet,
    )>,

    /// The place search box: query, results and suggestions.
    search: search::SearchState,
    /// The settings in use; what is on disk is in `saved`.
    settings: Settings,
    saved: saved::SavedSettings,
    /// The ruler.
    measure: measure::MeasureState,
    /// Esri Wayback: the releases, the one shown and the swipe compare.
    wayback: wayback::WaybackState,

    /// The app's time: live, a moment, or playing (roadmap 4.H).
    pub(super) time: globecraft_core::time::AppTime,
}

impl App {
    /// Whether a map layer is shown (its own switch; parents are checked by callers that care).
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
        self.settings
            .distance_units
            .resolve(self.locale.system_units)
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
        // Wakes the interface when background work lands.
        let repaint = || {
            let ctx = cc.egui_ctx.clone();
            move || ctx.request_repaint()
        };
        let runtime =
            globecraft_data::runtime::ServiceRuntime::new().expect("start network runtime");
        let locale = locale::LocaleState::detect();
        let lang = locale.language();
        let boundary_log: StatusLog = Arc::new(Mutex::new(Vec::new()));
        let (boundaries_rx, boundary_loader) =
            load_boundaries_async(boundary_log.clone(), runtime.handle(), repaint());
        let cache = TileCache::with_budgets(
            repaint(),
            runtime.handle(),
            globecraft_data::tiles::CacheBudgets {
                decoded_bytes: settings.cache.decoded_bytes(),
                disk_bytes: settings.cache.disk_bytes(),
            },
        );
        let renderer =
            rendering::Renderer::new(cache.clone(), cc.egui_ctx.clone(), settings.gpu_globe);
        let overlay_cache = overlays::OverlayCache::new(cc.egui_ctx.clone(), Some(cache.clone()));
        let (file_jobs, file_results) =
            globecraft_data::files::FileJobs::new(runtime.handle(), repaint());
        let local_search = globecraft_data::search::LocalSearch::new(runtime.handle(), repaint());
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            chrome: chrome::ChromeState::new(cc),
            assistant: assistant::AssistantState::default(),
            updates: update_restart::UpdatesState {
                service: globecraft_data::updater::UpdateService::new(runtime.handle(), repaint()),
                restart: update_restart::UpdateRestart::new(restart.clone()),
            },
            explorer: explorer::Explorer::default(),
            flat_earth: flat_earth::FlatEarth::default(),
            borders: borders::BordersState::new(
                boundary_loader,
                BoundaryUpdates::new(boundaries_rx, repaint()).expect("start boundary preparation"),
                boundary_log,
                overlay_cache,
            ),
            lang,
            fonts: fonts::FontService::new(cc.egui_ctx.clone()),
            controllers: controllers::Controllers::new(),
            sky: globecraft_data::sky::SkyService::default(),
            panes: panes::PanesState::default(),
            locale,
            cam: settings.navigation.startup.camera(),
            motion: motion::MotionState::default(),
            keys: keys::KeysState::load(),
            live: live::LiveState::new(&runtime, &cc.egui_ctx, &cache),
            map_data: map_data::MapDataState::default(),
            user_layers: Default::default(),
            pointer: pointer::PointerState::default(),
            picked: picked::PickedState::default(),
            tiles: cache::TilesState::new(cache),
            frame: frame::FrameState::new(renderer),
            search: search::SearchState::new(local_search),
            status: lang.text("app-ready-status").into(),
            files: files::FilesState {
                jobs: file_jobs,
                results: file_results,
                status: None,
                pending_view_link: view_links::link_from_args(),
            },
            places: places::PlacesState::default(),
            timeline: time::TimelineState::default(),
            windows: windows::WindowsState::new(settings.ui_scale),
            nav: navigation::NavState::default(),
            solo: None,
            settings: settings.clone(),
            saved: saved::SavedSettings {
                settings,
                writer: globecraft_data::settings::SettingsWriter::new(repaint())
                    .expect("start settings writer"),
            },
            measure: measure::MeasureState::default(),
            wayback: wayback::WaybackState::default(),
            time: globecraft_core::time::AppTime::default(),
            runtime,
        };
        app.restore_kml_session();
        if let Some(error) = &app.updates.restart.error {
            app.status = error.clone();
        }
        if let Some(version) = restart.installed_version.lock().unwrap().as_deref() {
            app.updates.service.resume_installed(version);
        }
        if let Some(path) = restart.planet.lock().unwrap().take() {
            app.explorer.request_open(path, &app.files.jobs);
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
        while self.files.jobs.is_busy() && started.elapsed() < QUIT_FILE_WAIT {
            std::thread::sleep(Duration::from_millis(20));
        }
        if !self.tiles.cache.wait_for_disk_writes(QUIT_DISK_WAIT) {
            eprintln!("Quit before every tile reached the disk cache");
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let restarting = self
            .updates
            .restart
            .request
            .requested
            .load(std::sync::atomic::Ordering::Acquire);
        match self.saved.writer.finish() {
            Ok(()) => {}
            Err(error) if restarting => {
                *self.updates.restart.request.error.lock().unwrap() = Some(format!(
                    "Settings save before restart failed: {error}. Click Restart to update to retry."
                ));
            }
            Err(error) => {
                eprintln!("Settings persistence failed during shutdown: {error}");
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
            if let Some(error) = self.updates.restart.request.error.lock().unwrap().clone() {
                eprintln!("Update restart stopped: {error}. Relaunch GlobeCraft manually.");
                std::process::exit(1);
            }
            eprintln!("Restarting GlobeCraft to finish the update");
            match self_update::restart::restart() {
                Ok(never) => match never {},
                Err(error) => {
                    eprintln!("Restart failed: {error}. Relaunch GlobeCraft manually.");
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
                || (self.layer(MapLayer::TimeZones) && self.pointer.cursor.is_some()));
        if let Some(after) = clock_wake(globecraft_core::math::now_unix(), shows_clock) {
            ctx.request_repaint_after(after);
        }
        // A view link from the command line opens once the window is up.
        if let Some(link) = self.files.pending_view_link.take()
            && let Err(error) = self.run("view.openLink", serde_json::json!({"link": link}))
        {
            self.status = error;
        }
        self.updates.service.start_once();
        self.updates.service.poll();
        let aircraft_enabled = self.layer(MapLayer::Flights)
            && self.explorer.active == globecraft_core::body::Body::Earth
            && !self.flat_earth.open;
        self.live.update_aircraft(ctx, aircraft_enabled, &self.cam);
        self.update_selected_flight(ctx, aircraft_enabled);
        // Independent realtime layers, loaded only for the globe while enabled.
        let earth =
            self.explorer.active == globecraft_core::body::Body::Earth && !self.flat_earth.open;
        self.live.update_feeds(
            &self.runtime,
            ctx,
            live::LiveQuery {
                earth,
                cam: &self.cam,
                vessels: self.layer(MapLayer::Vessels),
                satellites: self.layer(MapLayer::Satellites),
                aurora: self.layer(MapLayer::Aurora),
                traffic: self.layer(MapLayer::Traffic),
                // Airports by code: for the search box and a selected flight's route.
                airports: self.picked.flight.selected().is_some()
                    || !self.search.query.trim().is_empty(),
                tomtom_key: &self.keys.tomtom,
            },
        );
        self.update_time_zones(ctx, earth);
        self.update_marine_names(ctx, earth);
        self.map_data.update(
            &self.runtime,
            ctx,
            map_data::MapDataQuery {
                earth,
                view: (self.cam.lat, self.cam.lon, self.cam.alt, self.cam.tilt),
                transport: self.transport_shown(),
                rail_or_airports: self.layer(MapLayer::Railways) || self.layer(MapLayer::Airports),
                poi: self.poi_layers().any(),
                roads: self.layer(MapLayer::Roads),
            },
        );
        let scene = self.imagery_time();
        self.user_layers.update(
            ctx,
            &self.runtime,
            &self.settings.user_layers,
            (self.cam.lat, self.cam.lon, self.cam.alt, self.cam.tilt),
            earth,
            scene,
        );
        if self.time.speed() != 0.0 {
            // A playing time moves the sun and the timed points every frame.
            ctx.request_repaint();
        }
        let place_wake = ctx.clone();
        self.borders.place_names.update(
            &self.runtime,
            earth && self.layer(MapLayer::PlaceLabels),
            (self.cam.lat, self.cam.lon, self.cam.alt, self.cam.tilt),
            move || place_wake.request_repaint(),
        );
        self.refresh_place_names();
        // The services below refresh only when the interface runs: a heartbeat
        // while one is on, so they do not wait for the user to move.
        if earth
            && (self.layer(MapLayer::Vessels)
                || self.layer(MapLayer::Satellites)
                || self.layer(MapLayer::Roads)
                || self.layer(MapLayer::Traffic)
                || self.layer(MapLayer::Aurora)
                || self.user_layers_refresh())
        {
            ctx.request_repaint_after(Duration::from_secs(30));
        }
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
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.setup");
        // egui recreates its font atlas when it fills up, before the frame: the atlas is
        // then emptier than at the last frame's start. Label shapes kept across frames
        // must be laid out again, or they show other glyphs.
        let atlas_fill = ui.ctx().fonts(|fonts| fonts.font_atlas_fill_ratio());
        if atlas_fill < self.chrome.font_atlas_fill {
            self.borders.overlay_cache.font_atlas_reset();
        }
        self.chrome.font_atlas_fill = atlas_fill;
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
            .chrome
            .styled_scale
            .is_none_or(|scale| (self.settings.ui_scale - scale).abs() > 1e-4)
        {
            ui::theme::apply(ui, self.settings.ui_scale);
            self.chrome.styled_scale = Some(self.settings.ui_scale);
        }
        // persist settings once a slider is released
        if self.settings != self.saved.settings && !ui.input(|i| i.pointer.any_down()) {
            self.saved.writer.settings(&self.settings);
            self.saved.settings = self.settings.clone();
        }
        if let Some(error) = self.saved.writer.take_error() {
            self.status = format!("Settings persistence failed: {error}");
        }
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.input");
        if self.updates.restart.is_saving() {
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
        if self.motion.fly.is_some() {
            ui.request_repaint();
        }
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.header");
        self.header(ui);
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.ribbon");
        self.ribbon(ui);
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.status");
        self.status_bar(ui);
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.panes");
        self.left_pane(ui);
        self.right_pane(ui);
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.windows");
        self.settings_window(ui);
        self.menu_windows(ui);
        self.altitude_window(ui);
        self.add_data_window(ui);
        self.command_search_window(ui);
        self.shortcut_dialog(ui);
        ui::focus::paint_focus_ring(ui.ctx());
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.live_maps");
        self.live.maps.update(
            ui,
            &self.runtime,
            self.layer(MapLayer::Clouds),
            self.layer(MapLayer::Radar),
        );
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.boundary");
        let earth_visible =
            self.explorer.active == globecraft_core::body::Body::Earth && !self.flat_earth.open;
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
                load_ahead: self.frame.renderer.is_idle() && !self.frame.renderer.waits_for_tiles(),
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
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.globe");
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
        #[cfg(feature = "profiling")]
        watchdog.checkpoint("ui.slow.native_menu");
        #[cfg(target_os = "macos")]
        self.refresh_native_menu(ui);
        #[cfg(feature = "profiling")]
        {
            let viewport = ui.ctx().input(|input| input.viewport_rect());
            let pixels_per_point = ui.ctx().pixels_per_point();
            profile.values.width = (viewport.width() * pixels_per_point).round().max(0.0) as usize;
            profile.values.height =
                (viewport.height() * pixels_per_point).round().max(0.0) as usize;
            profile.values.inflight = self.tiles.cache.inflight();
            profile.values.resident_bytes = self.tiles.cache.resident_bytes();
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
