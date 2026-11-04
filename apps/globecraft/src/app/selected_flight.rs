//! The flight picked on the globe: its track so far (coloured by altitude, as on
//! ADSB.lol's map), the rest of its way to its destination, both airports and a card
//! with its route. Its position comes every few seconds.
use super::App;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Shape, Stroke};
use globecraft_core::{camera::Basis, language::Lang, layers::MapLayer, math::now_unix};
use globecraft_data::aircraft::{
    Aircraft, AircraftSnapshot,
    route::{RouteAirport, SelectedFlight, TrackPoint},
};
use globecraft_render::overlays::{SEGMENT_CLIP_MARGIN_POINTS, clip_segment, project_unhidden};
use serde_json::{Value, json};
use std::sync::Arc;

const TRACK_POINTS: f32 = 2.5;
const REMAINING_POINTS: f32 = 1.6;
const REMAINING: Color32 = Color32::from_rgb(235, 235, 235);
const RING_POINTS: f32 = 11.0;
const AIRPORT_POINTS: f32 = 5.0;
const AIRPORT_FILL: Color32 = Color32::from_rgb(255, 205, 110);
const HALO: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 200);
const LABEL_POINTS: f32 = 12.0;
const CARD_MARGIN_POINTS: f32 = 12.0;
const CARD_WIDTH_POINTS: f32 = 300.0;
/// The great circle to the destination is drawn with a point every this many km.
const REMAINING_STEP_KM: f64 = 40.0;
/// A position newer than this is moved on along its track; an older one stays.
const MAX_FLY_SECS: f64 = 120.0;
const KM_PER_NAUTICAL_MILE: f64 = 1.852;

/// Where the selected plane is now: its newest report, flown on along its track.
pub(super) fn flown_on(
    aircraft: &Aircraft,
    model: globecraft_core::earth_model::EarthModel,
    unix: f64,
) -> (f64, f64) {
    let age = (unix - aircraft.observed_unix).clamp(0.0, MAX_FLY_SECS);
    let speed_km_s = aircraft.speed_knots.unwrap_or(0.0).max(0.0) * KM_PER_NAUTICAL_MILE / 3600.0;
    globecraft_render::aircraft::fly(
        model,
        (aircraft.latitude, aircraft.longitude),
        aircraft.heading_deg,
        aircraft.turn_rate_deg_s.unwrap_or(0.0),
        speed_km_s,
        age,
    )
    .0
}

/// The track's colour at an altitude: orange near the ground, through green and
/// blue to violet at cruise, grey on the ground.
fn altitude_color(altitude_feet: Option<f64>) -> Color32 {
    let Some(feet) = altitude_feet else {
        return Color32::from_gray(150);
    };
    let hue = 30.0 + (feet / 40_000.0).clamp(0.0, 1.0) * 250.0;
    egui::ecolor::Hsva::new((hue / 360.0) as f32, 0.8, 0.95, 1.0).into()
}

/// A line on screen: projected, cut where it leaves the view or the visible side.
fn screen_runs(
    basis: &Basis,
    rect: Rect,
    line: impl IntoIterator<Item = (f64, f64)>,
) -> Vec<Vec<Pos2>> {
    let clip = rect.expand(SEGMENT_CLIP_MARGIN_POINTS);
    let mut runs = Vec::new();
    let mut run: Vec<Pos2> = Vec::new();
    let mut previous = None;
    for (lat, lon) in line {
        let current = project_unhidden(basis, rect, None, None, lat, lon);
        match previous
            .zip(current)
            .and_then(|(a, b)| clip_segment([a, b], clip))
        {
            Some([a, b]) if run.last() == Some(&a) => run.push(b),
            Some([a, b]) => {
                if run.len() >= 2 {
                    runs.push(std::mem::take(&mut run));
                }
                run = vec![a, b];
            }
            None => {
                if run.len() >= 2 {
                    runs.push(std::mem::take(&mut run));
                }
                run.clear();
            }
        }
        previous = current;
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

fn duration_text(secs: f64) -> String {
    let minutes = (secs / 60.0).round().max(0.0) as u64;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    }
}

fn halo_text(
    shapes: &mut Vec<Shape>,
    painter: &egui::Painter,
    at: Pos2,
    text: &str,
    size: f32,
    color: Color32,
) {
    let font = super::fonts::map_font(painter.ctx(), size);
    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    let halo = painter.layout_no_wrap(text.to_owned(), font, HALO);
    let corner = Align2::LEFT_CENTER.anchor_size(at, galley.size()).min;
    for offset in [
        egui::vec2(1.0, 1.0),
        egui::vec2(-1.0, -1.0),
        egui::vec2(1.0, -1.0),
        egui::vec2(-1.0, 1.0),
    ] {
        shapes.push(Shape::galley(corner + offset, halo.clone(), HALO));
    }
    shapes.push(Shape::galley(corner, galley, color));
}

impl App {
    /// Downloads for the selected flight; none while flights are off.
    pub(super) fn update_selected_flight(&mut self, ctx: &egui::Context, earth: bool) {
        if !(earth && self.layer(MapLayer::Flights)) {
            if self.flight_selection.selected().is_some() {
                self.flight_selection.clear();
            }
            self.selected_snapshot = None;
            return;
        }
        let wake = ctx.clone();
        self.flight_selection
            .update(&self.runtime, self.live_airports.airports(), move || {
                wake.request_repaint()
            });
        self.selected_snapshot = self.merged_snapshot();
    }

    /// The flights with the selected one's newest position, when it is newer than
    /// the list's (it is asked for every few seconds) or missing from it.
    fn merged_snapshot(&self) -> Option<(usize, f64, Arc<AircraftSnapshot>)> {
        let position = self.flight_selection.selected()?.position.clone()?;
        let snapshot = self.live_aircraft.snapshot();
        let key = (Arc::as_ptr(&snapshot) as usize, position.observed_unix);
        if let Some((pointer, observed, merged)) = &self.selected_snapshot
            && (*pointer, *observed) == key
        {
            return Some((key.0, key.1, merged.clone()));
        }
        let listed = snapshot.aircraft.iter().position(|a| a.hex == position.hex);
        if listed.is_some_and(|i| snapshot.aircraft[i].observed_unix >= position.observed_unix) {
            return None;
        }
        let mut aircraft = snapshot.aircraft.to_vec();
        match listed {
            Some(i) => aircraft[i] = position,
            None => aircraft.push(position),
        }
        Some((
            key.0,
            key.1,
            Arc::new(AircraftSnapshot {
                aircraft: aircraft.into(),
                global: snapshot.global.clone(),
                local: snapshot.local.clone(),
                error: snapshot.error.clone(),
                valid_until: snapshot.valid_until,
            }),
        ))
    }

    /// The flights to draw: with the selected one's newest position.
    pub(super) fn aircraft_snapshot(&self) -> Arc<AircraftSnapshot> {
        let snapshot = self.live_aircraft.snapshot();
        match &self.selected_snapshot {
            Some((pointer, _, merged)) if *pointer == Arc::as_ptr(&snapshot) as usize => {
                merged.clone()
            }
            _ => snapshot,
        }
    }

    pub(super) fn run_flight(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        match id {
            "flight.select" => {
                let hex = p
                    .get("hex")
                    .and_then(Value::as_str)
                    .map(|hex| hex.trim().to_ascii_lowercase())
                    .filter(|hex| {
                        let digits = hex.strip_prefix('~').unwrap_or(hex);
                        digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit())
                    })
                    .ok_or("hex: the plane's transponder address, 6 hex digits")?;
                let listed = self
                    .aircraft_snapshot()
                    .aircraft
                    .iter()
                    .find(|aircraft| *aircraft.hex == *hex)
                    .cloned();
                let callsign = p
                    .get("callsign")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|callsign| !callsign.is_empty())
                    .map(str::to_owned)
                    .or_else(|| {
                        listed
                            .as_ref()
                            .map(|aircraft| aircraft.callsign.trim().to_owned())
                    })
                    .unwrap_or_default();
                if !self.layer(MapLayer::Flights) {
                    self.settings.set_layer_visible(MapLayer::Flights, true);
                }
                self.flight_selection.select(&hex, &callsign, listed);
                self.selected_snapshot = None;
            }
            "flight.clear" => {
                self.flight_selection.clear();
                self.selected_snapshot = None;
            }
            _ => return Err(format!("unknown flight command {id}")),
        }
        Ok(self.flight_json())
    }

    fn flight_json(&self) -> Value {
        let Some(flight) = self.flight_selection.selected() else {
            return json!({"selected": null});
        };
        let airport = |airport: &RouteAirport| json!({"code": airport.code, "name": airport.name});
        json!({
            "selected": {
                "hex": flight.hex,
                "callsign": flight.callsign,
                "origin": flight.route.as_ref().map(|route| airport(&route.origin)),
                "destination": flight.route.as_ref().map(|route| airport(&route.destination)),
                "track_points": flight.track.len(),
            }
        })
    }

    /// The selected flight's track, the rest of its way and its airports, under the
    /// planes.
    pub(super) fn paint_selected_flight(&self, painter: &egui::Painter, basis: &Basis, rect: Rect) {
        let Some(flight) = self.flight_selection.selected() else {
            return;
        };
        let scale = self.settings.map_scale;
        let now = now_unix();
        let current = flight
            .position
            .as_ref()
            .map(|aircraft| flown_on(aircraft, basis.model, now))
            .or_else(|| flight.track.last().map(|p| (p.lat, p.lon)));
        let mut shapes = Vec::new();
        track_shapes(&mut shapes, basis, rect, &flight.track, current, scale);
        if let (Some(route), Some(at)) = (&flight.route, current) {
            let to = (route.destination.lat, route.destination.lon);
            let steps = (globecraft_core::math::haversine_km(at, to) / REMAINING_STEP_KM)
                .ceil()
                .clamp(2.0, 2000.0) as usize;
            let line = globecraft_core::math::great_circle(at, to, steps)
                .into_iter()
                .map(globecraft_core::math::v3_to_ll);
            let stroke = Stroke::new(REMAINING_POINTS * scale, REMAINING);
            for run in screen_runs(basis, rect, line) {
                shapes.extend(Shape::dashed_line(&run, stroke, 7.0 * scale, 5.0 * scale));
            }
        }
        if let Some(route) = &flight.route {
            for airport in [&route.origin, &route.destination] {
                let Some(at) = project_unhidden(basis, rect, None, None, airport.lat, airport.lon)
                    .map(|p| Pos2::new(p.x as f32, p.y as f32))
                    .filter(|p| rect.contains(*p))
                else {
                    continue;
                };
                shapes.push(Shape::circle_filled(
                    at,
                    AIRPORT_POINTS * scale,
                    AIRPORT_FILL,
                ));
                shapes.push(Shape::circle_stroke(
                    at,
                    AIRPORT_POINTS * scale,
                    Stroke::new(1.5 * scale, HALO),
                ));
                halo_text(
                    &mut shapes,
                    painter,
                    at + egui::vec2(9.0 * scale, 0.0),
                    &airport.code,
                    LABEL_POINTS * scale,
                    AIRPORT_FILL,
                );
            }
        }
        if let Some(at) = current
            .and_then(|(lat, lon)| project_unhidden(basis, rect, None, None, lat, lon))
            .map(|p| Pos2::new(p.x as f32, p.y as f32))
        {
            shapes.push(Shape::circle_stroke(
                at,
                RING_POINTS * scale,
                Stroke::new(3.5 * scale, HALO),
            ));
            shapes.push(Shape::circle_stroke(
                at,
                RING_POINTS * scale,
                Stroke::new(2.0 * scale, AIRPORT_FILL),
            ));
        }
        painter.extend(shapes);
    }

    /// The card of the selected flight, at the top left of the globe.
    pub(super) fn selected_flight_card(&mut self, ctx: &egui::Context, rect: Rect) {
        let Some(flight) = self.flight_selection.selected() else {
            return;
        };
        let lang = self.lang;
        let lines = card_lines(
            flight,
            lang,
            self.distance_units(),
            self.flight_selection.loading(),
        );
        let mut close = false;
        egui::Area::new(egui::Id::new("selected-flight-card"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_top() + egui::vec2(CARD_MARGIN_POINTS, CARD_MARGIN_POINTS))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(CARD_WIDTH_POINTS);
                    ui.horizontal(|ui| {
                        ui.heading(&lines.title);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close = ui
                                .small_button("✕")
                                .on_hover_text(lang.text("flight-card-close"))
                                .clicked();
                        });
                    });
                    ui.label(egui::RichText::new(&lines.route).strong());
                    for line in &lines.details {
                        ui.label(line);
                    }
                    ui.small(lang.text("flight-card-source"));
                });
            });
        if close {
            // Clearing cannot fail.
            let _ = self.run("flight.clear", json!({}));
        }
    }
}

fn track_shapes(
    shapes: &mut Vec<Shape>,
    basis: &Basis,
    rect: Rect,
    track: &[TrackPoint],
    current: Option<(f64, f64)>,
    scale: f32,
) {
    let clip = rect.expand(SEGMENT_CLIP_MARGIN_POINTS);
    let width = TRACK_POINTS * scale;
    let mut previous: Option<(glam::DVec2, Option<f64>)> = None;
    let last_altitude = track.last().and_then(|p| p.altitude_feet);
    let points = track
        .iter()
        .map(|p| (p.lat, p.lon, p.altitude_feet))
        .chain(current.map(|(lat, lon)| (lat, lon, last_altitude)));
    for (lat, lon, altitude) in points {
        let here = project_unhidden(basis, rect, None, None, lat, lon);
        if let (Some((a, altitude_a)), Some(b)) = (previous, here)
            && let Some(segment) = clip_segment([a, b], clip)
        {
            shapes.push(Shape::line_segment(
                segment,
                Stroke::new(width, altitude_color(altitude_a)),
            ));
        }
        previous = here.map(|at| (at, altitude));
    }
}

struct CardLines {
    title: String,
    route: String,
    details: Vec<String>,
}

fn card_lines(
    flight: &SelectedFlight,
    lang: Lang,
    units: globecraft_core::units::DistanceUnits,
    loading: bool,
) -> CardLines {
    let title = if flight.callsign.is_empty() {
        flight.hex.to_uppercase()
    } else {
        format!("{} · {}", flight.callsign, flight.hex.to_uppercase())
    };
    let place = |airport: &RouteAirport| {
        if airport.name.is_empty() {
            airport.code.clone()
        } else {
            format!("{} ({})", airport.name, airport.code)
        }
    };
    let route = match &flight.route {
        Some(route) => lang.format(
            "flight-card-route",
            &[
                ("from", place(&route.origin).into()),
                ("to", place(&route.destination).into()),
            ],
        ),
        None if !flight.route_known => lang.text("flight-card-route-loading").into(),
        None => lang.text("flight-card-route-unknown").into(),
    };
    let mut details = Vec::new();
    if flight.route.as_ref().is_some_and(|route| !route.plausible) {
        details.push(lang.text("flight-card-route-unsure").into());
    }
    if let Some(aircraft) = &flight.position {
        let mut parts = Vec::new();
        if let Some(feet) = aircraft.altitude_feet {
            parts.push(format!("{feet:.0} ft"));
        }
        if let Some(knots) = aircraft.speed_knots {
            parts.push(format!("{knots:.0} kt"));
        }
        if let Some(rate) = aircraft.vertical_rate_fpm.filter(|rate| rate.abs() >= 50.0) {
            parts.push(format!("{rate:+.0} ft/min"));
        }
        if !parts.is_empty() {
            details.push(parts.join(" · "));
        }
    }
    let mut progress = Vec::new();
    if let (Some(first), Some(last)) = (flight.track.first(), flight.track.last()) {
        let end = flight
            .position
            .as_ref()
            .map_or(last.unix, |p| p.observed_unix.max(last.unix));
        progress.push(lang.format(
            "flight-card-flown",
            &[("time", duration_text(end - first.unix).into())],
        ));
    }
    let at = flight
        .position
        .as_ref()
        .map(|p| (p.latitude, p.longitude))
        .or_else(|| flight.track.last().map(|p| (p.lat, p.lon)));
    if let (Some(route), Some(at)) = (&flight.route, at) {
        let km =
            globecraft_core::math::haversine_km(at, (route.destination.lat, route.destination.lon));
        let distance = match units {
            globecraft_core::units::DistanceUnits::Metric => format!("{km:.0} km"),
            globecraft_core::units::DistanceUnits::Imperial => format!("{:.0} mi", km / 1.609_344),
        };
        progress.push(lang.format("flight-card-to-go", &[("distance", distance.into())]));
    }
    if !progress.is_empty() {
        details.push(progress.join(" · "));
    }
    if flight.track.is_empty() && loading {
        details.push(lang.text("flight-card-track-loading").into());
    } else if flight.track.is_empty()
        && let Some(error) = &flight.error
    {
        details.push(lang.format("flight-card-error", &[("error", error.clone().into())]));
    }
    CardLines {
        title,
        route,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_track_is_coloured_by_altitude() {
        assert_eq!(altitude_color(None), Color32::from_gray(150));
        let low = egui::ecolor::Hsva::from(altitude_color(Some(0.0)));
        let high = egui::ecolor::Hsva::from(altitude_color(Some(38_000.0)));
        assert!(low.h < 0.12, "{low:?}");
        assert!(high.h > 0.65, "{high:?}");
    }

    #[test]
    fn a_flight_time_reads_in_hours_and_minutes() {
        assert_eq!(duration_text(40.0 * 60.0), "40 min");
        assert_eq!(duration_text(125.0 * 60.0), "2 h 05 min");
    }
}
