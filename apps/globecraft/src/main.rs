// GlobeCraft — a 3D globe in the style of Google Earth (Windows, macOS, Linux)
//
// Rendering: CPU ray-casting (parallel via rayon) of a sphere textured with
// Web Mercator tiles downloaded on demand (Esri / OSM / OpenTopoMap).
// UI: egui/eframe.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

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
    #[cfg(feature = "profiling")]
    globecraft_data::profiling::init();
    // First start after the rename: bring settings and caches over from EarthCraft.
    if let Err(error) = globecraft_data::migrate_legacy_data() {
        eprintln!("Could not copy EarthCraft settings and caches: {error}");
    }
    // Downloads of the border pipeline removed on 2026-10-25.
    std::thread::spawn(globecraft_data::remove_retired_caches);
    let restart = Arc::new(RestartRequest::default());
    loop {
        let settings = globecraft_data::settings::Settings::load();
        let mut viewport = egui::ViewportBuilder::default()
            .with_inner_size(INITIAL_WINDOW_SIZE_POINTS)
            .with_title(WINDOW_TITLE)
            .with_app_id("globecraft");
        // The header is the titlebar: macOS keeps its native traffic lights over a
        // transparent titlebar, while Windows and Linux draw their own controls.
        #[cfg(target_os = "macos")]
        {
            viewport = viewport
                .with_fullsize_content_view(true)
                .with_titlebar_shown(false)
                .with_title_shown(false)
                .with_titlebar_buttons_shown(true);
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            viewport = viewport.with_decorations(false);
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
            wgpu_options: presentation::configuration(settings.uncap_fps),
            ..Default::default()
        };
        let app_restart = restart.clone();
        eframe::run_native(
            WINDOW_TITLE,
            options,
            Box::new(move |cc| Ok(Box::new(App::new(cc, settings, app_restart)))),
        )?;
        // run_native has closed the window and dropped App, flushing preferences and
        // stopping its workers before the updated executable replaces this process.
        if !restart.requested.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        if restart.error.lock().unwrap().is_some() {
            continue;
        }
        match self_update::restart::restart() {
            Ok(never) => match never {},
            Err(error) => {
                *restart.error.lock().unwrap() = Some(format!(
                    "Restart failed: {error}. Save your work and relaunch GlobeCraft manually, or click Restart to update to retry."
                ));
            }
        }
    }
}
