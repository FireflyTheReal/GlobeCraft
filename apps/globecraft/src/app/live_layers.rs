//! Aircraft geometry is prepared in render; styled shapes and hover stay in egui.
use globecraft_core::layers::MapLayer;
mod poi;
mod shapes;
mod transport;
pub(super) use poi::PoiShapes;
pub(super) use transport::TransportShapes;
mod world;
pub(super) use world::WorldShapes;
#[cfg(test)]
mod tests;
use super::App;
use eframe::egui::{self, Color32, Rect, Vec2};
use globecraft_core::{camera::Basis, language::Lang, math::now_unix};
use globecraft_render::aircraft::{
    AircraftGeometry, AircraftWorker, ProjectionInput, ProjectionView,
};
use globecraft_render::motion;
use std::{sync::Arc, time::Duration};

/// Repaint cadence while flights are on but not drawn yet (no view); drawn flights
/// repaint at their motion step.
pub(super) const FLIGHT_REPAINT_INTERVAL: Duration = Duration::from_secs(1);
const HOVER_RADIUS_POINTS: f32 = 15.0;
const HOVER_FONT_POINTS: f32 = 12.0;
const HOVER_OFFSET_POINTS: Vec2 = Vec2::new(18.0, 12.0);
const HOVER_EDGE_MARGIN_POINTS: f32 = 16.0;
const HOVER_MIN_MARGIN_POINTS: f32 = 6.0;
const HOVER_PADDING_POINTS: f32 = 6.0;
const HOVER_CORNER_POINTS: f32 = 5.0;
const HOVER_FILL_RGBA: [u8; 4] = [13, 24, 35, 235];

pub(super) struct AircraftOverlay {
    geometry: Option<Arc<AircraftGeometry>>,
    requested: Option<ProjectionInput>,
    worker: AircraftWorker,
    shapes: Vec<egui::Shape>,
    shape_key: Option<(usize, Lang)>,
}
impl AircraftOverlay {
    pub(super) fn invalidate_text(&mut self) {
        self.shape_key = None;
    }
    pub(super) fn new(ctx: egui::Context) -> Self {
        Self {
            geometry: None,
            requested: None,
            worker: AircraftWorker::new(move || ctx.request_repaint())
                .expect("start aircraft projection"),
            shapes: Vec::new(),
            shape_key: None,
        }
    }
    pub(super) fn accept(&mut self, geometry: Arc<AircraftGeometry>, unix_secs: f64) {
        if !geometry.is_recent(unix_secs)
            || self.geometry.as_ref().is_some_and(|old| {
                old.matches(geometry.view, &geometry.data)
                    && old.is_recent(unix_secs)
                    && old.view.tick(old.unix_secs) >= geometry.view.tick(geometry.unix_secs)
            })
        {
            return;
        }
        self.geometry = Some(geometry);
        self.shape_key = None;
    }
    fn draw(&mut self, painter: &egui::Painter, input: ProjectionInput, lang: Lang) {
        if let Some(completed) = self.worker.take_completed()
            && completed.matches(input.view, &input.data)
        {
            self.accept(completed, input.unix_secs);
        }
        let current = self.geometry.as_ref().filter(|geometry| {
            geometry.matches(input.view, &input.data) && geometry.is_recent(input.unix_secs)
        });
        let needs_refresh = current.is_none_or(|geometry| {
            input.view.tick(geometry.unix_secs) != input.view.tick(input.unix_secs)
        });
        if needs_refresh
            && self
                .requested
                .as_ref()
                .is_none_or(|old| !old.same_request(&input))
        {
            self.worker.submit(input.clone());
            self.requested = Some(input.clone());
        }
        let Some(geometry) = current else {
            return;
        };
        let key = (Arc::as_ptr(geometry) as usize, lang);
        if self.shape_key != Some(key) {
            self.shapes = shapes::build(painter, geometry);
            self.shape_key = Some(key);
        }
        painter.extend(self.shapes.iter().cloned());
        draw_hover(painter, geometry, lang);
    }
}

impl App {
    pub(super) fn aircraft_input(
        &self,
        basis: Basis,
        rect: Rect,
        altitude_km: f64,
        pixels_per_point: f32,
    ) -> ProjectionInput {
        ProjectionInput {
            view: ProjectionView {
                basis,
                rect,
                altitude_km,
                map_scale: self.settings.map_scale,
                pixels_per_point,
            },
            data: self.live_aircraft.snapshot(),
            unix_secs: now_unix(),
        }
    }
    pub(super) fn draw_live_layers(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        altitude_km: f64,
        geometry: Option<&globecraft_render::live_layers::LiveGeometry>,
    ) {
        if self.layer(MapLayer::Flights) {
            let input =
                self.aircraft_input(*basis, rect, altitude_km, painter.ctx().pixels_per_point());
            let step = input.view.motion_step();
            self.aircraft_overlay.draw(painter, input, self.lang);
            painter
                .ctx()
                .request_repaint_after(motion::repaint_after(step));
        }
        if self.layer(MapLayer::Vessels)
            && let Some(geometry) = geometry
        {
            self.draw_vessel_overlay(painter, geometry);
            painter
                .ctx()
                .request_repaint_after(motion::repaint_after(geometry.input.view.vessel_step()));
        }
        if self.layer(MapLayer::Satellites)
            && let Some(geometry) = geometry
        {
            self.draw_satellite_overlay(painter, geometry);
            // Orbit predictions need to move even without another network fetch.
            painter
                .ctx()
                .request_repaint_after(motion::repaint_after(geometry.input.view.satellite_step()));
        }
    }
}

fn draw_hover(painter: &egui::Painter, geometry: &AircraftGeometry, lang: Lang) {
    let view = geometry.view;
    let ms = view.map_scale;
    let Some(hover) = painter
        .ctx()
        .pointer_hover_pos()
        .filter(|point| view.rect.contains(*point))
    else {
        return;
    };
    let mut nearest = None;
    let mut minimum = (HOVER_RADIUS_POINTS * ms).powi(2);
    for flight in &geometry.flights {
        let distance = (hover - flight.position).length_sq();
        if distance < minimum {
            minimum = distance;
            nearest = Some(flight);
        }
    }
    let Some(flight) = nearest else {
        return;
    };
    let sample = &geometry.data.aircraft[flight.source_index];
    let code = if sample.callsign.is_empty() {
        sample.hex.to_uppercase()
    } else {
        sample.callsign.to_string()
    };
    let text = lang.format(
        "aircraft-hover-tooltip",
        &[
            ("code", code.into()),
            ("hex", sample.hex.to_uppercase().into()),
            (
                "altitude",
                sample
                    .altitude_feet
                    .map_or("—".into(), |alt| format!("{alt:.0} ft"))
                    .into(),
            ),
            (
                "speed",
                sample
                    .speed_knots
                    .map_or("—".into(), |speed| format!("{speed:.0} kt"))
                    .into(),
            ),
            (
                "heading",
                globecraft_core::compass::heading_text(sample.heading_deg, lang).into(),
            ),
            (
                "age",
                format!("{:.0}", (now_unix() - sample.observed_unix).max(0.0)).into(),
            ),
        ],
    );
    let galley = painter.layout_no_wrap(
        text,
        crate::app::fonts::map_font(painter.ctx(), HOVER_FONT_POINTS * ms),
        Color32::WHITE,
    );
    let position = egui::pos2(
        (flight.position.x + HOVER_OFFSET_POINTS.x * ms)
            .min(view.rect.max.x - galley.size().x - HOVER_EDGE_MARGIN_POINTS * ms)
            .max(view.rect.min.x + HOVER_MIN_MARGIN_POINTS * ms),
        (flight.position.y + HOVER_OFFSET_POINTS.y * ms)
            .min(view.rect.max.y - galley.size().y - HOVER_EDGE_MARGIN_POINTS * ms)
            .max(view.rect.min.y + HOVER_MIN_MARGIN_POINTS * ms),
    );
    let bounds = Rect::from_min_size(position, galley.size()).expand(HOVER_PADDING_POINTS * ms);
    let [r, g, b, a] = HOVER_FILL_RGBA;
    let fill = Color32::from_rgba_unmultiplied(r, g, b, a);
    painter.rect_filled(bounds, HOVER_CORNER_POINTS * ms, fill);
    painter.galley(position, galley, Color32::WHITE);
}

// Additional worldwide AIS and predicted orbital-position layers.
impl App {
    fn draw_vessel_overlay(
        &mut self,
        painter: &egui::Painter,
        geometry: &globecraft_render::live_layers::LiveGeometry,
    ) {
        let rect = geometry.input.view.rect;
        let Some(vessels) = &geometry.input.vessels else {
            return;
        };
        let ms = self.settings.map_scale;
        let hover = painter.ctx().pointer_hover_pos();
        let mut nearest: Option<(&globecraft_data::vessels::Vessel, egui::Pos2)> = None;
        let mut distance = (14.0 * ms).powi(2);
        painter.extend(self.world_shapes.vessels(painter, geometry).iter().cloned());
        for projected in geometry.vessels.iter() {
            let ship = &vessels[projected.source_index];
            let at = projected.position;
            if let Some(h) = hover {
                let d = (h - at).length_sq();
                if d < distance {
                    distance = d;
                    nearest = Some((ship, at));
                }
            }
        }
        if let Some((ship, at)) = nearest {
            let name = if ship.name.is_empty() {
                "AIS vessel"
            } else {
                &ship.name
            };
            let age_minutes = ((now_unix() - ship.observed_unix).max(0.0) / 60.0).round();
            let text = self.lang.format(
                "vessels-hover-tooltip",
                &[
                    ("name", name.into()),
                    ("mmsi", ship.mmsi.as_str().into()),
                    ("latitude", format!("{:.3}", ship.lat).into()),
                    ("longitude", format!("{:.3}", ship.lon).into()),
                    (
                        "course",
                        globecraft_core::compass::heading_text(ship.course, self.lang).into(),
                    ),
                    (
                        "speed",
                        ship.speed_knots
                            .map_or("?".into(), |speed| format!("{speed:.1}"))
                            .into(),
                    ),
                    ("minutes", format!("{age_minutes:.0}").into()),
                ],
            );
            hover_label(painter, rect, at, &text, ms);
        }
    }
    fn draw_satellite_overlay(
        &mut self,
        painter: &egui::Painter,
        geometry: &globecraft_render::live_layers::LiveGeometry,
    ) {
        let rect = geometry.input.view.rect;
        let Some(satellites) = &geometry.input.satellites else {
            return;
        };
        let ms = self.settings.map_scale;
        let hover = painter.ctx().pointer_hover_pos();
        let mut nearest: Option<(&globecraft_data::satellites::Satellite, egui::Pos2)> = None;
        let mut distance = (13.0 * ms).powi(2);
        painter.extend(
            self.world_shapes
                .satellites(painter, geometry)
                .iter()
                .cloned(),
        );
        for projected in geometry.satellites.iter() {
            let sat = &satellites[projected.source_index];
            let at = projected.position;
            if let Some(h) = hover {
                let d = (h - at).length_sq();
                if d < distance {
                    distance = d;
                    nearest = Some((sat, at));
                }
            }
        }
        if let Some((sat, at)) = nearest {
            let text = self.lang.format(
                "satellites-hover-tooltip",
                &[
                    ("name", sat.name.as_str().into()),
                    ("norad", sat.norad.as_str().into()),
                ],
            );
            hover_label(painter, rect, at, &text, ms);
        }
    }
}
fn hover_label(painter: &egui::Painter, rect: Rect, screen: egui::Pos2, text: &str, ms: f32) {
    let galley = painter.layout_no_wrap(
        text.to_string(),
        crate::app::fonts::map_font(painter.ctx(), 12.0 * ms),
        Color32::WHITE,
    );
    let pos = egui::pos2(
        (screen.x + 15.0)
            .min(rect.right() - galley.size().x - 10.0)
            .max(rect.left() + 5.0),
        (screen.y + 12.0)
            .min(rect.bottom() - galley.size().y - 10.0)
            .max(rect.top() + 5.0),
    );
    painter.rect_filled(
        Rect::from_min_size(pos, galley.size()).expand(5.0),
        5.0,
        Color32::from_rgba_unmultiplied(12, 22, 36, 236),
    );
    painter.galley(pos, galley, Color32::WHITE);
}
