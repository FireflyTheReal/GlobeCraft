// GlobeCraft — a 3D globe in the style of Google Earth (Windows, macOS, Linux)
//
// Rendering: CPU ray-casting (parallel via rayon) of a sphere textured with
// Web Mercator tiles downloaded on demand (Esri / OSM / OpenTopoMap).
// UI: egui/eframe.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

/// `watchdog.checkpoint(name)` in profiling builds and nothing otherwise, so a pass
/// can be marked in one line.
macro_rules! profile_checkpoint {
    ($watchdog:ident, $name:literal) => {
        #[cfg(feature = "profiling")]
        $watchdog.checkpoint($name);
    };
}

mod app;
mod presentation;
use app::{App, RestartRequest, WINDOW_TITLE};
use eframe::egui;
use std::sync::{Arc, atomic::Ordering};
const INITIAL_WINDOW_SIZE_POINTS: [f32; 2] = [1280.0, 800.0];
const APP_ICON_PNG: &[u8] = std::cfg_select! {
    target_os = "macos" => include_bytes!("../../../assets/packaging/macos/icon.png"),
    _ => include_bytes!("../../../assets/packaging/icon_256.png"),
};

fn main() -> Result<(), eframe::Error> {
    globecraft_data::run_log::install_panic_hook();
    #[cfg(feature = "profiling")]
    globecraft_data::profiling::init();
    // The cache folder moves to the system's cache location, once.
    let cache_move = globecraft_data::migrate_cache();
    // First start after the rename: bring settings and caches over from EarthCraft. Before
    // the run log opens, which creates the data folder (an existing folder ends migration).
    let migration = globecraft_data::migrate_legacy_data();
    // Then settings and keys leave the cache folder for the configuration folder.
    let config_move = globecraft_data::migrate_config();
    globecraft_data::run_log::init();
    if let Err(error) = cache_move {
        globecraft_data::log_line!("Could not move the cache folder: {error}");
    }
    if let Err(error) = migration {
        globecraft_data::log_line!("Could not copy EarthCraft settings and caches: {error}");
    }
    if let Err(error) = config_move {
        globecraft_data::log_line!("Could not move settings to the configuration folder: {error}");
    }
    // Downloads of the border pipeline removed on 2026-10-25.
    std::thread::spawn(globecraft_data::remove_retired_caches);
    let restart = Arc::new(RestartRequest::default());
    let mut renderer = eframe::Renderer::Wgpu;
    loop {
        let (settings, load_problem) = globecraft_data::settings::Settings::load();
        globecraft_data::network::set_offline(settings.offline);
        // Before anything reads a key: they are read from where this setting keeps them.
        globecraft_data::keys::keep_in_keychain(settings.keys_in_keychain);
        if let Some(problem) = load_problem {
            globecraft_data::log_line!("{problem}");
            *restart
                .load_problem
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(problem);
        }
        let mut viewport = egui::ViewportBuilder::default()
            .with_inner_size(INITIAL_WINDOW_SIZE_POINTS)
            .with_title(WINDOW_TITLE)
            .with_app_id("globecraft");
        // macOS: the header is the titlebar, with the native traffic lights over a
        // transparent titlebar. Windows and Linux keep their native title bar and its
        // own minimize, maximize and close buttons.
        #[cfg(target_os = "macos")]
        {
            viewport = viewport
                .with_fullsize_content_view(true)
                .with_titlebar_shown(false)
                .with_title_shown(false)
                .with_titlebar_buttons_shown(true);
        }
        if let Ok(img) = image::load_from_memory(APP_ICON_PNG) {
            let img = img.to_rgba8();
            viewport = viewport.with_icon(egui::IconData {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            });
        }
        let options = eframe::NativeOptions {
            viewport,
            renderer,
            wgpu_options: presentation::configuration(settings.uncap_fps),
            ..Default::default()
        };
        let app_restart = restart.clone();
        let result = eframe::run_native(
            WINDOW_TITLE,
            options,
            Box::new(move |cc| Ok(Box::new(App::new(cc, settings, app_restart)))),
        );
        if let Err(error) = &result
            && presentation::retry_with_glow(renderer, error)
        {
            globecraft_data::log_line!(
                "The wgpu interface renderer could not start ({error}); using OpenGL"
            );
            renderer = eframe::Renderer::Glow;
            continue;
        }
        result?;
        // run_native has closed the window and dropped App, flushing preferences and
        // stopping its workers before the updated executable replaces this process.
        if !restart.requested.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        if restart
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            continue;
        }
        match self_update::restart::restart() {
            Ok(never) => match never {},
            Err(error) => {
                *restart
                    .error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!(
                    "Restart failed: {error}. Save your work and relaunch GlobeCraft manually, or click Restart to update to retry."
                ));
            }
        }
    }
}
