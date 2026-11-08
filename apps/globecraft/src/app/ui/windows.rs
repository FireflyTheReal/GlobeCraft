//! About and controls help use normal app-owned egui windows.
use crate::app::App;
use eframe::egui;

/// Size of the Bloom logo above the About window's title.
const ABOUT_LOGO_POINTS: f32 = 72.0;

impl App {
    pub(in crate::app) fn menu_windows(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        egui::Window::new(lang.text("menu-about-action"))
            .id(egui::Id::new("about-globecraft"))
            .open(&mut self.windows.about)
            .resizable(false)
            .show(ctx, |ui| {
                let (logo, _) = ui.allocate_exact_size(
                    egui::vec2(ABOUT_LOGO_POINTS, ABOUT_LOGO_POINTS),
                    egui::Sense::hover(),
                );
                super::brand::paint_bloom(
                    ui.painter(),
                    logo,
                    super::brand::logo_ink(ui.visuals().dark_mode),
                );
                ui.heading("GlobeCraft");
                ui.label(lang.format(
                    "about-version-label",
                    &[("version", env!("CARGO_PKG_VERSION").into())],
                ));
                ui.label(lang.text("about-description"));
                ui.hyperlink_to(
                    lang.text("about-project-link"),
                    "https://github.com/FireflyTheReal/GlobeCraft",
                );
                ui.label(env!("CARGO_PKG_LICENSE"));
            });
        egui::Window::new(lang.text("menu-controls-help"))
            .id(egui::Id::new("controls-help"))
            .open(&mut self.windows.shortcuts)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label(lang.text("controls-help-text"));
                ui.separator();
                egui::Grid::new("shortcut-grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for spec in globecraft_core::commands::COMMANDS {
                            let shortcut = crate::app::commands::shortcut_text(spec.id);
                            if !shortcut.is_empty() {
                                ui.label(lang.text(spec.label));
                                ui.monospace(shortcut);
                                ui.end_row();
                            }
                        }
                    });
            });
    }
}
