//! The satellite picked on the globe (user, 2026-11-07): one full revolution of its
//! orbit at its height, the half it has flown solid and the half to come dashed, the
//! satellite itself enlarged, and a card with what it is and how it moves. The orbit
//! is predicted with SGP4 from the elements already loaded: nothing is downloaded.
use super::App;
use super::live_layers::{satellite_colour, satellite_symbol};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};
use globecraft_core::{camera::Basis, math::now_unix};
use globecraft_data::satellites::{Satellite, SatellitePosition};
use serde_json::{Value, json};

/// Points along one revolution: enough for a smooth curve at any zoom.
const ORBIT_SAMPLES: usize = 360;
const ORBIT_POINTS: f32 = 2.0;
const CASING_POINTS: f32 = 2.0;
const CASING: Color32 = Color32::from_rgba_premultiplied(8, 10, 16, 200);
const DASH_POINTS: f32 = 8.0;
const GAP_POINTS: f32 = 6.0;
/// The selected satellite's symbol, this much larger than the others.
const SELECTED_SCALE: f32 = 1.8;
const SYMBOL_POINTS: f32 = 5.0;
const RING: Color32 = Color32::WHITE;
/// Clicks this close to a satellite pick it.
const PICK_RADIUS_POINTS: f32 = 13.0;
const CARD_MARGIN_POINTS: f32 = 12.0;
const CARD_WIDTH_POINTS: f32 = 260.0;
const SECONDS_PER_DAY: f64 = 86_400.0;
const KM_PER_MILE: f64 = 1.609_344;

/// Which satellite is selected, and where the satellites were drawn last (to pick).
#[derive(Default)]
pub(super) struct SatelliteSelection {
    norad: Option<String>,
    drawn: Vec<(Pos2, String)>,
}

impl SatelliteSelection {
    pub(super) fn selected(&self) -> Option<&str> {
        self.norad.as_deref()
    }

    /// Where the satellites are drawn this frame (replaces the last frame's).
    pub(super) fn set_drawn(&mut self, drawn: impl IntoIterator<Item = (Pos2, String)>) {
        self.drawn.clear();
        self.drawn.extend(drawn);
    }

    /// The satellite drawn nearest `point`, within reach.
    pub(super) fn at(&self, point: Pos2, map_scale: f32) -> Option<&str> {
        let reach = (PICK_RADIUS_POINTS * map_scale).powi(2);
        self.drawn
            .iter()
            .map(|(at, norad)| ((point - *at).length_sq(), norad))
            .filter(|(distance, _)| *distance < reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, norad)| norad.as_str())
    }
}

/// A point of the orbit on screen, at its height or (flat) on the ground below.
fn orbit_point(basis: &Basis, rect: Rect, at: &SatellitePosition, heights: bool) -> Option<Pos2> {
    if heights {
        basis.project_above(basis.raised_position(at.lat, at.lon, at.altitude_km), rect)
    } else {
        basis.project(basis.surface_position(at.lat, at.lon), rect)
    }
}

/// Runs of screen points, cut where the orbit goes behind the Earth or the camera.
fn runs(points: impl Iterator<Item = Option<Pos2>>) -> Vec<Vec<Pos2>> {
    let mut runs: Vec<Vec<Pos2>> = vec![Vec::new()];
    for point in points {
        match point {
            Some(at) => runs.last_mut().expect("one run at least").push(at),
            None if runs.last().is_some_and(|run| !run.is_empty()) => runs.push(Vec::new()),
            None => {}
        }
    }
    runs.retain(|run| run.len() >= 2);
    runs
}

impl App {
    pub(super) fn run_satellite(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        match id {
            "satellite.select" => {
                let norad = p
                    .get("norad")
                    .and_then(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .or_else(|| v.as_u64().map(|n| n.to_string()))
                    })
                    .ok_or("norad (the satellite's catalogue number) is required")?;
                let Some(satellite) = self
                    .live_satellites
                    .snapshot()
                    .iter()
                    .find(|satellite| satellite.norad == norad)
                else {
                    return Err(format!("no satellite {norad} is loaded"));
                };
                let name = satellite.name.clone();
                self.satellite_selection.norad = Some(norad.clone());
                Ok(json!({"norad": norad, "name": name}))
            }
            "satellite.clear" => {
                self.satellite_selection.norad = None;
                Ok(json!({"cleared": true}))
            }
            _ => Err(format!("unknown command {id}")),
        }
    }

    fn selected_satellite(&self) -> Option<&Satellite> {
        let norad = self.satellite_selection.selected()?;
        self.live_satellites
            .snapshot()
            .iter()
            .find(|satellite| satellite.norad == norad)
    }

    /// The selected satellite's orbit and enlarged symbol, above the others.
    pub(super) fn paint_selected_satellite(
        &self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        heights: bool,
    ) {
        let Some(satellite) = self.selected_satellite() else {
            return;
        };
        let scale = self.settings.map_scale;
        let now = now_unix();
        let path = satellite.orbit_path(now, ORBIT_SAMPLES);
        let colour = satellite_colour(satellite.group);
        let width = ORBIT_POINTS * scale;
        let casing = Stroke::new(width + CASING_POINTS * scale, CASING);
        let stroke = Stroke::new(width, colour);
        let (flown, coming): (Vec<_>, Vec<_>) = path.iter().partition(|(at, _)| *at <= now);
        let mut shapes = Vec::new();
        // The half to come, dashed, from where it is now.
        let ahead = flown.last().into_iter().chain(coming.iter());
        for run in runs(ahead.map(|(_, at)| orbit_point(basis, rect, at, heights))) {
            let (dash, gap) = (DASH_POINTS * scale, GAP_POINTS * scale);
            shapes.extend(Shape::dashed_line(&run, casing, dash, gap));
            shapes.extend(Shape::dashed_line(&run, stroke, dash, gap));
        }
        for run in runs(
            flown
                .iter()
                .map(|(_, at)| orbit_point(basis, rect, at, heights)),
        ) {
            shapes.push(Shape::line(run.clone(), casing));
            shapes.push(Shape::line(run, stroke));
        }
        if let Some(at) = satellite
            .position(now)
            .and_then(|at| orbit_point(basis, rect, &at, heights))
        {
            let size = SYMBOL_POINTS * SELECTED_SCALE * scale;
            shapes.push(Shape::circle_stroke(
                at,
                size * 1.6,
                Stroke::new(1.5 * scale, RING),
            ));
            shapes.extend(satellite_symbol(satellite.group, at, size, scale));
        }
        painter.extend(shapes);
    }

    /// The selected satellite's card, at the top left of the globe.
    pub(super) fn selected_satellite_card(&mut self, ctx: &egui::Context, rect: Rect) {
        let Some(satellite) = self.selected_satellite() else {
            return;
        };
        let lang = self.lang;
        let now = now_unix();
        let units = self.distance_units();
        let position = satellite.position(now);
        let mut lines = vec![lang.format(
            "satellite-card-catalogue",
            &[
                ("norad", satellite.norad.as_str().into()),
                ("group", lang.text(satellite.group.label_key()).into()),
            ],
        )];
        if let Some(at) = position {
            lines.push(lang.format(
                "satellite-card-height",
                &[("height", units.altitude(at.altitude_km).into())],
            ));
            if let Some(km_s) = satellite.speed_km_s(at.altitude_km) {
                let speed = match units {
                    globecraft_core::units::DistanceUnits::Metric => {
                        format!("{km_s:.2} km/s ({:.0} km/h)", km_s * 3600.0)
                    }
                    globecraft_core::units::DistanceUnits::Imperial => {
                        let mi_s = km_s / KM_PER_MILE;
                        format!("{mi_s:.2} mi/s ({:.0} mph)", mi_s * 3600.0)
                    }
                };
                lines.push(lang.format("satellite-card-speed", &[("speed", speed.into())]));
            }
            if let Ok(place) = self.settings.coordinate_display.format(at.lat, at.lon) {
                lines.push(lang.format("satellite-card-over", &[("place", place.into())]));
            }
        }
        if let Some(minutes) = satellite.orbit.period_minutes() {
            lines.push(lang.format(
                "satellite-card-orbit",
                &[
                    ("period", format!("{minutes:.1}").into()),
                    (
                        "inclination",
                        format!("{:.1}", satellite.orbit.inclination_deg).into(),
                    ),
                ],
            ));
        }
        let age_days = (now - satellite.orbit.epoch_unix_secs) / SECONDS_PER_DAY;
        let source = lang.format(
            "satellite-card-source",
            &[
                (
                    "model",
                    if satellite.sgp4.is_some() {
                        "sgp4"
                    } else {
                        "simple"
                    }
                    .into(),
                ),
                ("days", format!("{age_days:.1}").into()),
            ],
        );
        let title = satellite.name.clone();
        let mut open = true;
        egui::Window::new(egui::RichText::new(title).strong())
            .id(egui::Id::new("selected-satellite-card"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .constrain_to(rect)
            .default_pos(rect.left_top() + egui::vec2(CARD_MARGIN_POINTS, CARD_MARGIN_POINTS))
            .default_width(CARD_WIDTH_POINTS)
            .show(ctx, |ui| {
                for line in &lines {
                    ui.label(line);
                }
                ui.small(source);
            });
        if !open {
            // Clearing cannot fail.
            let _ = self.run("satellite.clear", json!({}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbits_break_where_they_go_behind_the_earth() {
        let at = |x: f32| Some(Pos2::new(x, 0.0));
        let cut = runs(
            [
                at(0.0),
                at(1.0),
                None,
                None,
                at(2.0),
                at(3.0),
                at(4.0),
                None,
                at(5.0),
            ]
            .into_iter(),
        );
        assert_eq!(cut.len(), 2, "{cut:?}");
        assert_eq!(cut[1].len(), 3);
    }

    #[test]
    fn a_click_picks_the_nearest_satellite_within_reach() {
        let mut selection = SatelliteSelection::default();
        selection.set_drawn([
            (Pos2::new(100.0, 100.0), "25544".to_owned()),
            (Pos2::new(108.0, 100.0), "48274".to_owned()),
        ]);
        assert_eq!(selection.at(Pos2::new(106.0, 101.0), 1.0), Some("48274"));
        assert_eq!(selection.at(Pos2::new(160.0, 100.0), 1.0), None);
    }
}
