//! The legend: what the colours and symbols on the globe mean. It opens by itself when
//! something coded appears (aircraft, ships, satellites, a selected flight, rain
//! radar, aurora, places of interest),
//! closes with its ✕, and `ui.legend` (View tab, Command Search) brings it back.
//! Each key takes its colours from the code that draws them, so the two never differ.
use super::App;
use super::live_layers::{
    PoiGroup, VesselClass, aurora_colour, class_symbol, satellite_symbol, vessel_symbol,
};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Stroke, Vec2};
use globecraft_core::layers::MapLayer;
use globecraft_data::settings::workspace::RightPane;
use globecraft_data::{
    aircraft::AircraftClass, live_maps::RADAR_BANDS, satellites::SatelliteGroup,
};
use serde_json::{Value, json};

const RAMP_SIZE_POINTS: Vec2 = Vec2::new(220.0, 12.0);
const RAMP_STEPS: usize = 64;
const TICK_HEIGHT_POINTS: f32 = 14.0;
const TICK_FONT_POINTS: f32 = 11.0;
const SWATCH_POINTS: f32 = 12.0;
/// The aircraft and satellite symbols are drawn in a square this large.
const SYMBOL_POINTS: f32 = 22.0;
const SATELLITE_SYMBOL_POINTS: f32 = 5.0;
/// The track's colours span the ground to this height.
const TRACK_TOP_FEET: f64 = 40_000.0;
/// The aurora glow starts at this chance (%) and is full at the second.
const AURORA_CHANCE_SPAN: (f32, f32) = (4.0, 60.0);

/// A colour key the legend can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LegendKind {
    Aircraft,
    Vessels,
    Satellites,
    FlightAltitude,
    Radar,
    Aurora,
    PlacesOfInterest,
}

/// Which keys were on the globe last frame (a key that was not opens the legend's pane).
#[derive(Default)]
pub(super) struct Legend {
    seen: Vec<LegendKind>,
}

/// The radar bands' names, lowest first.
const RADAR_BAND_KEYS: [&str; 5] = [
    "legend-radar-light",
    "legend-radar-moderate",
    "legend-radar-heavy",
    "legend-radar-very-heavy",
    "legend-radar-extreme",
];

/// A colour bar from `colour(0.0)` to `colour(1.0)`, with labels under it.
fn ramp(ui: &mut egui::Ui, colour: impl Fn(f32) -> Color32, ticks: &[(f32, &str)]) {
    let (rect, _) = ui.allocate_exact_size(RAMP_SIZE_POINTS, Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let mut mesh = egui::Mesh::default();
    let step = rect.width() / RAMP_STEPS as f32;
    for i in 0..RAMP_STEPS {
        let middle = (i as f32 + 0.5) / RAMP_STEPS as f32;
        let x = rect.left() + i as f32 * step;
        mesh.add_colored_rect(
            // Edge to edge: an overlap would show as stripes where the colour is see-through.
            Rect::from_min_max(
                egui::pos2(x, rect.top()),
                egui::pos2(x + step, rect.bottom()),
            ),
            colour(middle),
        );
    }
    painter.add(mesh);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Outside,
    );
    let (labels, _) = ui.allocate_exact_size(
        egui::vec2(RAMP_SIZE_POINTS.x, TICK_HEIGHT_POINTS),
        Sense::hover(),
    );
    let painter = ui.painter();
    for (index, (at, text)) in ticks.iter().enumerate() {
        let anchor = if index == 0 {
            Align2::LEFT_TOP
        } else if index + 1 == ticks.len() {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(
            egui::pos2(labels.left() + at * labels.width(), labels.top()),
            anchor,
            *text,
            FontId::proportional(TICK_FONT_POINTS),
            ui.visuals().text_color(),
        );
    }
}

/// A symbol and what it stands for.
fn symbol(ui: &mut egui::Ui, shapes: impl FnOnce(egui::Pos2) -> Vec<egui::Shape>, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(SYMBOL_POINTS, SYMBOL_POINTS), Sense::hover());
        ui.painter().extend(shapes(rect.center()));
        ui.label(text);
    });
}

/// Items in two columns.
fn two_columns<T>(
    ui: &mut egui::Ui,
    id: &str,
    items: &[T],
    mut item: impl FnMut(&mut egui::Ui, &T),
) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for (index, value) in items.iter().enumerate() {
                item(ui, value);
                if index % 2 == 1 {
                    ui.end_row();
                }
            }
        });
}

/// A coloured square and what it stands for.
fn swatch(ui: &mut egui::Ui, colour: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(SWATCH_POINTS, SWATCH_POINTS), Sense::hover());
        ui.painter().rect_filled(rect, 3.0, colour);
        ui.label(text);
    });
}

impl App {
    /// The colour keys of what is on the globe now.
    pub(super) fn legend_kinds(&self) -> Vec<LegendKind> {
        let earth =
            self.explorer.active == globecraft_core::body::Body::Earth && !self.flat_earth.open;
        if !earth {
            return Vec::new();
        }
        [
            (LegendKind::Aircraft, self.layer(MapLayer::Flights)),
            (LegendKind::Vessels, self.layer(MapLayer::Vessels)),
            (LegendKind::Satellites, self.layer(MapLayer::Satellites)),
            (
                LegendKind::FlightAltitude,
                self.layer(MapLayer::Flights) && self.flight_selection.selected().is_some(),
            ),
            (LegendKind::Radar, self.layer(MapLayer::Radar)),
            (LegendKind::Aurora, self.layer(MapLayer::Aurora)),
            (
                LegendKind::PlacesOfInterest,
                self.layer(MapLayer::PointsOfInterest),
            ),
        ]
        .into_iter()
        .filter_map(|(kind, shown)| shown.then_some(kind))
        .collect()
    }

    pub(super) fn run_legend(&mut self, p: &Value) -> Value {
        let on = p
            .get("on")
            .and_then(Value::as_bool)
            .unwrap_or(!self.legend_open());
        if on {
            self.settings.workspace.open_right(RightPane::Legend);
        } else {
            self.settings.workspace.close_right(RightPane::Legend);
        }
        json!({"visible": on})
    }

    pub(super) fn legend_open(&self) -> bool {
        self.settings.workspace.right_is_open(RightPane::Legend)
    }

    /// Opens the legend's pane when a key appears on the globe (without taking the view
    /// from another pane).
    pub(super) fn legend_watch(&mut self) {
        let kinds = self.legend_kinds();
        if kinds.iter().any(|kind| !self.legend.seen.contains(kind)) {
            self.settings.workspace.add_right(RightPane::Legend);
        }
        self.legend.seen = kinds;
    }

    /// The legend's pane: a key per layer on the globe that has one.
    pub(super) fn legend_pane(&mut self, ui: &mut egui::Ui) {
        let kinds = self.legend_kinds();
        let lang = self.lang;
        if kinds.is_empty() {
            ui.label(lang.text("legend-empty"));
        }
        for (index, kind) in kinds.iter().enumerate() {
            if index > 0 {
                ui.separator();
            }
            match kind {
                LegendKind::Aircraft => {
                    ui.strong(lang.text("legend-aircraft"));
                    two_columns(ui, "legend-aircraft", &AircraftClass::ALL, |ui, class| {
                        symbol(
                            ui,
                            |at| vec![class_symbol(*class, at, SYMBOL_POINTS)],
                            lang.text(class.label_key()),
                        );
                    });
                }
                LegendKind::Vessels => {
                    ui.strong(lang.text("legend-vessels"));
                    two_columns(ui, "legend-vessels", &VesselClass::ALL, |ui, class| {
                        symbol(
                            ui,
                            |at| vec![vessel_symbol(*class, at, SYMBOL_POINTS)],
                            lang.text(class.label_key()),
                        );
                    });
                }
                LegendKind::Satellites => {
                    ui.strong(lang.text("legend-satellites"));
                    two_columns(
                        ui,
                        "legend-satellites",
                        &SatelliteGroup::ALL,
                        |ui, group| {
                            symbol(
                                ui,
                                |at| satellite_symbol(*group, at, SATELLITE_SYMBOL_POINTS, 1.0),
                                lang.text(group.label_key()),
                            );
                        },
                    );
                }
                LegendKind::FlightAltitude => {
                    ui.strong(lang.text("legend-flight-altitude"));
                    ramp(
                        ui,
                        |t| {
                            super::selected_flight::altitude_color(Some(
                                f64::from(t) * TRACK_TOP_FEET,
                            ))
                        },
                        &[
                            (0.0, "0"),
                            (0.25, "10k"),
                            (0.5, "20k"),
                            (0.75, "30k"),
                            (1.0, "40k ft"),
                        ],
                    );
                    swatch(
                        ui,
                        super::selected_flight::altitude_color(None),
                        lang.text("legend-on-ground"),
                    );
                }
                LegendKind::Radar => {
                    ui.strong(lang.text("legend-radar"));
                    for (index, (lowest, [r, g, b])) in RADAR_BANDS.iter().enumerate() {
                        let range = match RADAR_BANDS.get(index + 1) {
                            Some((next, _)) => format!("{lowest:.0}–{next:.0} dBZ"),
                            None => format!("{lowest:.0}+ dBZ"),
                        };
                        let text = lang.format(
                            "legend-radar-band",
                            &[
                                ("name", lang.text(RADAR_BAND_KEYS[index]).into()),
                                ("range", range.into()),
                            ],
                        );
                        swatch(ui, Color32::from_rgb(*r, *g, *b), &text);
                    }
                }
                LegendKind::Aurora => {
                    ui.strong(lang.text("legend-aurora"));
                    let (low, high) = AURORA_CHANCE_SPAN;
                    ramp(
                        ui,
                        |t| aurora_colour((low + t * (high - low)).round() as u8),
                        &[(0.0, "4 %"), (0.5, "30 %"), (1.0, "60 %+")],
                    );
                }
                LegendKind::PlacesOfInterest => {
                    ui.strong(lang.text("legend-poi"));
                    two_columns(ui, "legend-poi-groups", &PoiGroup::ALL, |ui, group| {
                        swatch(ui, group.color(), lang.text(group.label_key()));
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_radar_band_has_a_name() {
        assert_eq!(RADAR_BANDS.len(), RADAR_BAND_KEYS.len());
        assert!(RADAR_BANDS.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
}
