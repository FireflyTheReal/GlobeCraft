//! Menus on the status bar's readouts: the coordinate format on the coordinates, the
//! units on the elevation and the altitude. The values are read there, so they are
//! changed there too (right-click).
use super::widgets::{Look, menu_item_with};
use crate::app::App;
use eframe::egui::{self, Response};
use globecraft_core::coordinates::CoordinateFormat;
use serde_json::json;

impl App {
    /// The coordinate formats, on a right-click of `response`.
    pub(super) fn coordinate_format_menu(&mut self, response: &Response) {
        let lang = self.lang;
        response.context_menu(|ui| {
            ui.set_min_width(220.0);
            ui.weak(lang.text("coordinates-format-label"));
            for format in CoordinateFormat::ALL {
                menu_item_with(
                    ui,
                    self,
                    "settings.coordinateFormat",
                    json!({"format": format.code()}),
                    Look {
                        icon: Some("goto_xy"),
                        label: Some(lang.text(format.name_id())),
                        ..Look::default()
                    },
                );
            }
        });
    }

    /// The distance units, on a right-click of `response`.
    pub(super) fn units_menu(&mut self, response: &Response) {
        let lang = self.lang;
        let system_unit = match self.system_units {
            globecraft_core::units::DistanceUnits::Metric => "km",
            globecraft_core::units::DistanceUnits::Imperial => "mi",
        };
        let system = lang.format("settings-units-system", &[("unit", system_unit.into())]);
        response.context_menu(|ui: &mut egui::Ui| {
            ui.set_min_width(200.0);
            ui.weak(lang.text("settings-distance-units-label"));
            for (code, label) in [
                ("system", system.as_str()),
                ("metric", lang.text("settings-units-metric")),
                ("imperial", lang.text("settings-units-imperial")),
            ] {
                menu_item_with(
                    ui,
                    self,
                    "settings.units",
                    json!({"units": code}),
                    Look {
                        icon: Some("ruler_units"),
                        label: Some(label),
                        ..Look::default()
                    },
                );
            }
        });
    }
}
