//! Map style: colour, width and dash per line type (borders, roads, transport, rivers,
//! parks and time zones), and name sizes.
use crate::app::App;
use eframe::egui::{self, Color32};
use globecraft_data::settings::map_style::{
    DashStyle, LabelType, LineGroup, LineType, MAX_LABEL_SIZE_POINTS, MAX_LINE_WIDTH_POINTS,
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
        LineType::MajorRoad => "settings-map-style-line-major-road",
        LineType::MinorRoad => "settings-map-style-line-minor-road",
        LineType::Roadworks => "settings-map-style-line-roadworks",
        LineType::Railway => "settings-map-style-line-railway",
        LineType::ServiceRail => "settings-map-style-line-service-rail",
        LineType::Subway => "settings-map-style-line-subway",
        LineType::Tram => "settings-map-style-line-tram",
        LineType::Ferry => "settings-map-style-line-ferry",
        LineType::Runway => "settings-map-style-line-runway",
        LineType::Taxiway => "settings-map-style-line-taxiway",
        LineType::River => "settings-map-style-line-river",
        LineType::Canal => "settings-map-style-line-canal",
        LineType::Park => "settings-map-style-line-park",
        LineType::TimeZone => "settings-map-style-line-time-zone",
    }
}

fn group_name(group: LineGroup) -> &'static str {
    match group {
        LineGroup::Borders => "settings-map-style-group-borders",
        LineGroup::Roads => "settings-map-style-group-roads",
        LineGroup::Transport => "settings-map-style-group-transport",
        LineGroup::WaterAndParks => "settings-map-style-group-water-parks",
        LineGroup::TimeZones => "settings-map-style-group-time-zones",
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
        LabelType::Road => "settings-map-style-label-road",
        LabelType::TimeZone => "settings-map-style-label-time-zone",
    }
}

/// Text IDs the settings search matches for this section.
pub(super) const KEYWORDS: &[&str] = &[
    "settings-map-style-line-country",
    "settings-map-style-line-state",
    "settings-map-style-line-disputed",
    "settings-map-style-line-municipal",
    "settings-map-style-line-major-road",
    "settings-map-style-line-minor-road",
    "settings-map-style-line-roadworks",
    "settings-map-style-line-railway",
    "settings-map-style-line-service-rail",
    "settings-map-style-line-subway",
    "settings-map-style-line-tram",
    "settings-map-style-line-ferry",
    "settings-map-style-line-runway",
    "settings-map-style-line-taxiway",
    "settings-map-style-line-river",
    "settings-map-style-line-canal",
    "settings-map-style-line-park",
    "settings-map-style-line-time-zone",
    "settings-map-style-group-roads",
    "settings-map-style-group-transport",
    "settings-map-style-group-water-parks",
    "settings-map-style-group-time-zones",
    "settings-map-style-color-label",
    "settings-map-style-width-label",
    "settings-map-style-dash-label",
    "settings-map-style-labels-title",
    "settings-map-style-label-city",
    "settings-map-style-label-road",
    "settings-map-style-label-time-zone",
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
                for group in LineGroup::ALL {
                    ui.strong(lg.text(group_name(group)));
                    ui.end_row();
                    for kind in LineType::ALL
                        .into_iter()
                        .filter(|kind| kind.group() == group)
                    {
                        line_row(ui, lg, self.settings.map_style.line_mut(kind), kind);
                    }
                }
            });
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

/// One line type's colour, width and dash.
fn line_row(
    ui: &mut egui::Ui,
    lg: globecraft_core::language::Lang,
    style: &mut globecraft_data::settings::map_style::LineStyle,
    kind: LineType,
) {
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
