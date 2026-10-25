//! Map style: colour, width and dash per border type, and name sizes.
use crate::app::App;
use eframe::egui::{self, Color32};
use globecraft_data::settings::map_style::{
    DashStyle, LabelType, LineType, MAX_LABEL_SIZE_POINTS, MAX_LINE_WIDTH_POINTS,
    MIN_LABEL_SIZE_POINTS, MIN_LINE_WIDTH_POINTS, Rgba,
};
use serde_json::Value;

const WIDTH_STEP_POINTS: f64 = 0.1;
const LABEL_STEP_POINTS: f64 = 0.5;

fn line_name(kind: LineType) -> &'static str {
    match kind {
        LineType::Country => "settings-map-style-line-country",
        LineType::State => "settings-map-style-line-state",
        LineType::Municipal => "settings-map-style-line-municipal",
        LineType::Disputed => "settings-map-style-line-disputed",
    }
}

fn dash_name(dash: DashStyle) -> &'static str {
    match dash {
        DashStyle::Solid => "settings-map-style-dash-solid",
        DashStyle::Dashed => "settings-map-style-dash-dashed",
        DashStyle::Dotted => "settings-map-style-dash-dotted",
    }
}

fn label_name(kind: LabelType) -> &'static str {
    match kind {
        LabelType::Country => "settings-map-style-label-country",
        LabelType::State => "settings-map-style-label-state",
        LabelType::City => "settings-map-style-label-city",
    }
}

/// Text IDs the settings search matches for this section.
pub(super) const KEYWORDS: &[&str] = &[
    "settings-map-style-line-country",
    "settings-map-style-line-state",
    "settings-map-style-line-disputed",
    "settings-map-style-disputed-claims",
    "settings-map-style-line-municipal",
    "settings-map-style-color-label",
    "settings-map-style-width-label",
    "settings-map-style-dash-label",
    "settings-map-style-labels-title",
    "settings-map-style-label-city",
];

impl App {
    pub(super) fn settings_map_style(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        egui::Grid::new("settings_map_style_lines")
            .num_columns(4)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("");
                ui.weak(lg.text("settings-map-style-color-label"));
                ui.weak(lg.text("settings-map-style-width-label"));
                ui.weak(lg.text("settings-map-style-dash-label"));
                ui.end_row();
                for kind in LineType::ALL {
                    let style = self.settings.map_style.line_mut(kind);
                    ui.label(lg.text(line_name(kind)));
                    let [r, g, b, a] = style.color.0;
                    let mut color = Color32::from_rgba_unmultiplied(r, g, b, a);
                    if egui::color_picker::color_edit_button_srgba(
                        ui,
                        &mut color,
                        egui::color_picker::Alpha::OnlyBlend,
                    )
                    .changed()
                    {
                        style.color = Rgba(color.to_srgba_unmultiplied());
                    }
                    ui.add(
                        egui::Slider::new(
                            &mut style.width_points,
                            MIN_LINE_WIDTH_POINTS..=MAX_LINE_WIDTH_POINTS,
                        )
                        .step_by(WIDTH_STEP_POINTS)
                        .suffix(" pt"),
                    );
                    egui::ComboBox::from_id_salt(("map_style_dash", kind.code()))
                        .selected_text(lg.text(dash_name(style.dash)))
                        .show_ui(ui, |ui| {
                            super::super::selection::size_menu(
                                ui,
                                DashStyle::ALL.map(|dash| lg.text(dash_name(dash))),
                            );
                            for dash in DashStyle::ALL {
                                super::super::selection::full_width_value(
                                    ui,
                                    &mut style.dash,
                                    dash,
                                    lg.text(dash_name(dash)),
                                );
                            }
                        });
                    ui.end_row();
                }
            });
        let mut claims = self.settings.map_style.disputed_claims;
        if ui
            .checkbox(&mut claims, lg.text("settings-map-style-disputed-claims"))
            .on_hover_text(lg.text("settings-map-style-disputed-claims-help"))
            .changed()
        {
            self.run_reporting(
                "style.disputedClaims",
                serde_json::json!({ "enabled": claims }),
            );
        }
        ui.weak(lg.text("settings-map-style-line-help"));
        ui.add_space(ui.spacing().item_spacing.y);
        ui.strong(lg.text("settings-map-style-labels-title"));
        egui::Grid::new("settings_map_style_labels")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                for kind in LabelType::ALL {
                    ui.label(lg.text(label_name(kind)));
                    let mut size = self.settings.map_style.labels.get(kind);
                    if ui
                        .add(
                            egui::Slider::new(
                                &mut size,
                                MIN_LABEL_SIZE_POINTS..=MAX_LABEL_SIZE_POINTS,
                            )
                            .step_by(LABEL_STEP_POINTS)
                            .suffix(" pt"),
                        )
                        .changed()
                    {
                        self.settings.map_style.labels.set(kind, size);
                    }
                    ui.end_row();
                }
            });
        let reset = self.command_state("style.reset", &Value::Null);
        if ui
            .add_enabled(
                reset.enabled,
                egui::Button::new(lg.text("settings-map-style-reset-action")),
            )
            .clicked()
        {
            self.run_reporting("style.reset", Value::Null);
        }
    }
}
