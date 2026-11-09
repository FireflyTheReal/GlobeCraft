//! The About and Keyboard Shortcuts dialogs.
use super::surface::{Surface, Width};
use crate::app::App;
use eframe::egui;

/// The Keyboard Shortcuts dialog scrolls beyond this height.
const SHORTCUTS_MAX_HEIGHT_POINTS: f32 = 480.0;

/// Size of the Bloom logo above the About window's title.
const ABOUT_LOGO_POINTS: f32 = 72.0;

impl App {
    pub(in crate::app) fn menu_windows(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        if self.windows.about {
            let (open, _) = Surface::dialog(
                "about-globecraft",
                lang.text("menu-about-action"),
                Width::Small,
            )
            .show(ctx, lang, |ui| {
                ui.vertical_centered(|ui| {
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
                    ui.weak(env!("CARGO_PKG_LICENSE"));
                });
            });
            self.windows.about = open;
        }
        if self.windows.shortcuts {
            let (open, _) = Surface::dialog(
                "controls-help",
                lang.text("menu-controls-help"),
                Width::Medium,
            )
            .show(ctx, lang, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(SHORTCUTS_MAX_HEIGHT_POINTS)
                    .show(ui, |ui| {
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
            });
            self.windows.shortcuts = open;
        }
    }
}
