//! The Measure pane: the ruler's switch, unit, totals, bearing and history.
use super::theme::{Tokens, semibold};
use super::widgets::{section_heading, small};
use crate::app::App;
use eframe::egui;
use serde_json::Value;

impl App {
    pub(in crate::app) fn measure_pane(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let lg = self.lang;
        ui.horizontal(|ui| {
            small(ui, self, "ruler.enable", Value::Null);
            small(ui, self, "ruler.editPoints", Value::Null);
        });
        let selected =
            super::ruler::unit_selector(ui, self.settings.ruler_unit, self.us_locale, lg);
        if selected != self.settings.ruler_unit {
            self.settings.ruler_unit = selected;
        }
        ui.add_space(4.0);
        let model = self.active_earth_model();
        let total = self.ruler_paths.distance_km(model);
        let distance = self.settings.ruler_unit.format_with_angle(
            total,
            self.ruler_paths.angular_distance_degrees(model),
            self.distance_units(),
            lg,
        );
        egui::Frame::NONE
            .fill(t.pane_header)
            .stroke(egui::Stroke::new(1.0, t.border))
            .corner_radius(8)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(lg.text("measure-total-label"))
                        .size(11.0)
                        .color(t.text_dim),
                );
                ui.label(
                    egui::RichText::new(distance)
                        .font(semibold(20.0))
                        .color(t.text),
                );
                ui.label(
                    egui::RichText::new(lg.format(
                        "measure-points-label",
                        &[("count", self.ruler_paths.point_count().to_string().into())],
                    ))
                    .size(11.5)
                    .color(t.text_dim),
                );
                if let Some(pair) = self.ruler_paths.last_leg() {
                    let bearing = model
                        .initial_bearing(pair[0], pair[1])
                        .map(|bearing| globecraft_core::compass::heading_text(bearing, lg))
                        .unwrap_or_else(|| "—".into());
                    ui.label(lg.format("ruler-bearing-label", &[("bearing", bearing.into())]))
                        .on_hover_text(lg.text("ruler-bearing-tooltip"));
                }
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            small(ui, self, "edit.undo", Value::Null);
            small(ui, self, "edit.redo", Value::Null);
            small(ui, self, "ruler.clear", Value::Null);
        });
        ui.add_space(6.0);
        section_heading(ui, lg.text("measure-how-title"));
        ui.label(
            egui::RichText::new(lg.text(if cfg!(target_os = "macos") {
                "ruler-point-edit-help-macos"
            } else {
                "ruler-point-edit-help"
            }))
            .size(11.5)
            .color(t.text_dim),
        );
    }
}
