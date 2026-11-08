//! Aircraft geometry is prepared in render; styled shapes and hover stay in egui.
use globecraft_core::layers::MapLayer;
mod aurora;
pub(super) use aurora::AuroraShapes;
pub(super) use aurora::legend_colour as aurora_colour;
mod fade;
pub(super) use fade::{TiledShapes, fresh_text, stroke_runs, styled_line};
mod poi;
mod poi_icons;
pub(super) use poi_icons::Group as PoiGroup;
pub(super) use world::{VesselClass, satellite_colour, satellite_symbol, vessel_symbol};
mod poi_memory;
mod shapes;
pub(super) use shapes::class_symbol;
mod transport;
pub(super) use poi::PoiShapes;
#[cfg(test)]
pub(super) use poi::river_scene;
pub(super) use transport::TransportShapes;
mod world;
pub(super) use world::WorldShapes;
#[cfg(test)]
mod tests;
use super::App;
use eframe::egui::{self, Color32, Rect, Vec2};
use globecraft_core::{camera::Basis, language::Lang, math::now_unix};
use globecraft_data::airports::Airport;
use globecraft_render::aircraft::{
    AircraftGeometry, AircraftWorker, ProjectionInput, ProjectionView,
};
use globecraft_render::motion;
use std::{sync::Arc, time::Duration};

/// Repaint cadence while flights are on but not drawn yet (no view); drawn flights
/// repaint at their motion step.
pub(super) const FLIGHT_REPAINT_INTERVAL: Duration = Duration::from_secs(1);
const HOVER_RADIUS_POINTS: f32 = 15.0;
/// The other planes' opacity while a flight is selected.
const OTHER_PLANES_OPACITY: f32 = 0.55;
const HOVER_FONT_POINTS: f32 = 12.0;
const HOVER_OFFSET_POINTS: Vec2 = Vec2::new(18.0, 12.0);
const HOVER_EDGE_MARGIN_POINTS: f32 = 16.0;
const HOVER_MIN_MARGIN_POINTS: f32 = 6.0;
const HOVER_PADDING_POINTS: f32 = 6.0;
const HOVER_CORNER_POINTS: f32 = 5.0;
const HOVER_FILL_RGBA: [u8; 4] = [13, 24, 35, 235];
/// Climbs and descents below this read as level flight.
const LEVEL_VERTICAL_RATE_FPM: f64 = 50.0;

pub(super) struct AircraftOverlay {
    geometry: Option<Arc<AircraftGeometry>>,
    requested: Option<ProjectionInput>,
    worker: AircraftWorker,
    shapes: Vec<egui::Shape>,
    shape_key: Option<(usize, Lang)>,
    /// The projection drawn this frame, for the hover card and the selected plane.
    drawn: Option<Arc<AircraftGeometry>>,
}
impl AircraftOverlay {
    pub(super) fn invalidate_text(&mut self) {
        self.shape_key = None;
    }
    #[cfg(test)]
    pub(super) fn new(ctx: egui::Context) -> Self {
        Self::with_worker(
            AircraftWorker::new(move || ctx.request_repaint()).expect("start aircraft projection"),
        )
    }
    /// Planes keep their height above the ground in `cache`.
    pub(super) fn with_elevation(
        ctx: egui::Context,
        cache: Arc<globecraft_data::tiles::TileCache>,
    ) -> Self {
        Self::with_worker(
            AircraftWorker::with_elevation(cache, move || ctx.request_repaint())
                .expect("start aircraft projection"),
        )
    }
    fn with_worker(worker: AircraftWorker) -> Self {
        Self {
            geometry: None,
            requested: None,
            worker,
            shapes: Vec::new(),
            shape_key: None,
            drawn: None,
        }
    }
    /// The plane drawn under `point`, if any.
    pub(super) fn flight_at(
        &self,
        point: egui::Pos2,
    ) -> Option<globecraft_data::aircraft::Aircraft> {
        let geometry = self.geometry.as_ref()?;
        let reach = (HOVER_RADIUS_POINTS * geometry.view.map_scale).powi(2);
        geometry
            .flights
            .iter()
            .map(|flight| ((point - flight.position).length_sq(), flight))
            .filter(|(distance, _)| *distance < reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, flight)| geometry.data.aircraft[flight.source_index].clone())
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
    /// Draws the planes, at `opacity` (dimmed while a flight is selected).
    fn draw(&mut self, painter: &egui::Painter, input: ProjectionInput, lang: Lang, opacity: f32) {
        self.drawn = None;
        // A projection for this view is shown even if a newer list came meanwhile.
        if let Some(completed) = self.worker.take_completed()
            && completed.view == input.view
        {
            self.accept(completed, input.unix_secs);
        }
        // New flight data keeps the planes of the same view on screen until it is
        // projected (a few milliseconds), instead of leaving a gap.
        let current = self
            .geometry
            .as_ref()
            .filter(|geometry| geometry.view == input.view && geometry.is_recent(input.unix_secs));
        let needs_refresh = current.is_none_or(|geometry| {
            !Arc::ptr_eq(&geometry.data, &input.data)
                || input.view.tick(geometry.unix_secs) != input.view.tick(input.unix_secs)
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
        let Some(geometry) = current.cloned() else {
            return;
        };
        let key = (Arc::as_ptr(&geometry) as usize, lang);
        if self.shape_key != Some(key) {
            self.shapes = shapes::build(painter, &geometry);
            self.shape_key = Some(key);
        }
        let mut faded = painter.clone();
        faded.multiply_opacity(opacity);
        faded.extend(self.shapes.iter().cloned());
        self.drawn = Some(geometry);
    }

    /// The hover card of the plane under the pointer, above everything else.
    fn hover(&self, painter: &egui::Painter, lang: Lang) {
        if let Some(geometry) = &self.drawn {
            draw_hover(painter, geometry, lang);
        }
    }

    /// Where the plane with this address is drawn this frame.
    pub(super) fn plane_position(&self, hex: &str) -> Option<egui::Pos2> {
        let geometry = self.drawn.as_ref()?;
        geometry
            .flights
            .iter()
            .find(|flight| &*geometry.data.aircraft[flight.source_index].hex == hex)
            .map(|flight| flight.position)
    }

    /// The plane with this address as drawn this frame, in the selected colours.
    pub(super) fn selected_plane(&self, hex: &str) -> Option<egui::Shape> {
        let geometry = self.drawn.as_ref()?;
        let flight = geometry
            .flights
            .iter()
            .find(|flight| &*geometry.data.aircraft[flight.source_index].hex == hex)?;
        Some(shapes::selected_plane(
            flight,
            geometry.data.aircraft[flight.source_index].class,
            geometry.view.map_scale,
        ))
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
                heights: !self.settings.layers.flat_live_objects,
                relief: self.overlay_ground(),
            },
            data: self.aircraft_snapshot(),
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
            // A selected flight's track, route and plane go above the other planes,
            // which are dimmed so they stand out; the hover card goes above all.
            let selected = self.flight_selection.selected().is_some();
            let opacity = if selected { OTHER_PLANES_OPACITY } else { 1.0 };
            self.aircraft_overlay
                .draw(painter, input, self.lang, opacity);
            self.paint_selected_flight(painter, basis, rect);
            self.aircraft_overlay.hover(painter, self.lang);
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

fn vertical_rate_text(rate: Option<f64>) -> String {
    rate.map_or_else(
        || "—".to_owned(),
        |rate| {
            if rate.abs() < LEVEL_VERTICAL_RATE_FPM {
                "0 ft/min".to_owned()
            } else {
                format!("{rate:+.0} ft/min")
            }
        },
    )
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
                "vertical_rate",
                vertical_rate_text(sample.vertical_rate_fpm).into(),
            ),
            (
                "age",
                format!("{:.0}", (now_unix() - sample.observed_unix).max(0.0)).into(),
            ),
        ],
    );
    let mut text = text;
    // Its registration and type, and a declared emergency (ADSB.lol only).
    if let Some(details) = sample.details.as_deref() {
        let names: Vec<&str> = [
            details.registration.as_deref(),
            details.type_code.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !names.is_empty() {
            text.push('\n');
            text.push_str(&names.join(" · "));
        }
        if let Some(kind) = details.emergency.as_deref() {
            text.push('\n');
            text.push_str(&lang.format("flight-card-emergency", &[("kind", kind.into())]));
        }
    }
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
            let text = world::vessel_tooltip(self.lang, ship);
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
        let mut nearest: Option<(
            &globecraft_data::satellites::Satellite,
            egui::Pos2,
            f64,
            &Vec<usize>,
        )> = None;
        let mut distance = (13.0 * ms).powi(2);
        painter.extend(
            self.world_shapes
                .satellites(painter, geometry)
                .iter()
                .cloned(),
        );
        self.satellite_selection
            .set_drawn(
                geometry
                    .satellites
                    .iter()
                    .filter(|p| p.in_view)
                    .map(|projected| {
                        (
                            projected.position,
                            satellites[projected.source_index].norad.clone(),
                        )
                    }),
            );
        self.paint_selected_satellite(
            painter,
            &geometry.input.view.basis,
            rect,
            geometry.input.satellite_heights,
        );
        for projected in geometry.satellites.iter().filter(|p| p.in_view) {
            let sat = &satellites[projected.source_index];
            let at = projected.position;
            if let Some(h) = hover {
                let d = (h - at).length_sq();
                if d < distance {
                    distance = d;
                    nearest = Some((sat, at, projected.altitude_km, &projected.together));
                }
            }
        }
        if let Some((sat, at, altitude_km, together)) = nearest {
            let together: Vec<&str> = together
                .iter()
                .map(|index| satellites[*index].name.as_str())
                .collect();
            let text = world::satellite_tooltip(self.lang, sat, Some(altitude_km), &together);
            hover_label(painter, rect, at, &text, ms);
        }
    }
}
pub(super) fn hover_label(
    painter: &egui::Painter,
    rect: Rect,
    screen: egui::Pos2,
    text: &str,
    ms: f32,
) {
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

impl App {
    /// The aurora glow, under every line and name, while the app's time is near the
    /// forecast's moment.
    pub(super) fn paint_aurora(&mut self, painter: &egui::Painter, basis: &Basis, rect: Rect) {
        if !self.layer(MapLayer::Aurora) {
            return;
        }
        let Some(grid) = self.live_aurora.grid() else {
            return;
        };
        let scene = self.scene_unix();
        if !self.time.is_live() && (scene - grid.forecast).abs() > aurora::SHOWN_WITHIN_SECS {
            return;
        }
        let sunlight = self.layer(MapLayer::Sunlight);
        self.aurora_shapes
            .paint(painter, basis, rect, &grid, scene, sunlight);
    }
}

/// An airport as the hover card names it: its code, name and town.
pub(super) fn airport_label(airport: &Airport) -> String {
    let mut label = format!("{} · {}", airport.code(), airport.name);
    if !airport.municipality.is_empty() && !airport.name.contains(&*airport.municipality) {
        label.push_str(", ");
        label.push_str(&airport.municipality);
    }
    label
}
