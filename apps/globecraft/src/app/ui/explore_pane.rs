//! The Globe pane: search results, quick navigation, export and the controls help.
use super::paths::short_folder;
use super::theme::Tokens;
use super::widgets::{Look, section_heading, small, small_with};
use crate::app::App;
use eframe::egui;
use serde_json::{Value, json};

impl App {
    pub(in crate::app) fn explore_pane(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let lg = self.lang;
        let mut goto = None;
        section_heading(ui, lg.text("search-results-label"));
        if self.results.is_empty() {
            ui.label(
                egui::RichText::new(lg.text("explore-results-empty"))
                    .size(11.5)
                    .color(t.text_dim),
            );
        }
        for r in &self.results {
            let short: String = r.name.chars().take(64).collect();
            let resp = super::widgets::row_button(
                ui,
                true,
                egui::Button::new(egui::RichText::new(short).size(12.0))
                    .frame(false)
                    .wrap(),
            );
            if resp.on_hover_text(&r.name).clicked() {
                goto = Some((r.lat, r.lon, r.alt));
            }
        }
        ui.add_space(6.0);
        section_heading(ui, lg.text("navigation-section-title"));
        ui.horizontal_wrapped(|ui| {
            for id in [
                "camera.resetNorth",
                "camera.globalView",
                "camera.autoRotate",
                "camera.zoomIn",
                "camera.zoomOut",
            ] {
                small(ui, self, id, Value::Null);
            }
        });
        ui.add_space(6.0);
        section_heading(ui, lg.text("export-section-title"));
        small_with(ui, self, "file.screenshot", Value::Null, Look::default());
        ui.label(
            egui::RichText::new(lg.format(
                "export-folder-label",
                &[("path", short_folder(&self.settings.screenshot_dir).into())],
            ))
            .size(11.0)
            .color(t.text_dim),
        )
        .on_hover_text(&self.settings.screenshot_dir);
        small_with(
            ui,
            self,
            "ui.settings",
            Value::Null,
            Look {
                label: Some(lg.text("settings-folders-action")),
                ..Look::default()
            },
        );
        ui.add_space(6.0);
        section_heading(ui, lg.text("menu-controls-help"));
        ui.label(
            egui::RichText::new(lg.text("controls-help-text"))
                .size(11.5)
                .color(t.text_dim),
        );
        if let Some((lat, lon, alt)) = goto {
            let _ = self.run(
                "camera.flyTo",
                json!({"lat": lat, "lon": lon, "altitude_km": alt}),
            );
        }
    }
}
