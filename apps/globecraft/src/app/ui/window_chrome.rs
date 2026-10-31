//! The window's own controls stay the operating system's: macOS keeps its native
//! traffic lights over a transparent titlebar merged with the header, Windows and
//! Linux keep their native title bar (minimize, maximize/restore, close, dragging and
//! resizing all come from the system).
use crate::app::App;
use eframe::egui::{self, Ui};

impl App {
    /// Room the native macOS traffic lights take at the left of the header; none in
    /// full screen, where macOS hides them, and none on other systems.
    pub(in crate::app) fn traffic_lights_width(&self, ctx: &egui::Context) -> f32 {
        #[cfg(target_os = "macos")]
        {
            if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
                return 0.0;
            }
            self.traffic_lights_width_native / ctx.zoom_factor()
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = ctx;
            0.0
        }
    }

    /// On macOS the header is the titlebar: dragging an empty part of it moves the
    /// window and double-clicking it zooms, as the native titlebar would. Buttons added
    /// after the header's background interaction stay on top, so pressing them still
    /// clicks the button. Elsewhere the system title bar does this.
    pub(in crate::app) fn header_drag(&self, ui: &Ui, drag: &egui::Response) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let primary = egui::PointerButton::Primary;
        if drag.drag_started_by(primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        } else if drag.double_clicked_by(primary) {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
    }
}
