use super::App;
use eframe::egui::{self, Align2, Color32, Rect, Stroke, Vec2};
use globecraft_core::camera::Basis;
use globecraft_core::layers::MapLayer;
use globecraft_data::boundaries::BoundarySnapshot;
#[cfg(test)]
mod benchmarks;
mod cache;
mod galleys;
mod meshes;
mod projected;
pub(super) use cache::OverlayCache;
use cache::{ARRIVAL_FADE_STEPS, ArrivalFade, OverlayLayers, OverlayStyle, OverlayView};
use globecraft_data::boundaries::BoundaryData;
pub(super) use meshes::PreparedOverlay;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a streamed boundary layer takes to fade in once its data arrives.
const ARRIVAL_FADE: Duration = Duration::from_millis(600);
/// How long a new border set takes to replace the one before it (fade in, then the
/// old set's leftover lines fade out).
const EXCHANGE_FADE: Duration = Duration::from_millis(500);

/// When each streamed boundary layer last went from empty to loaded.
#[derive(Default)]
pub(super) struct LayerArrivals {
    states: Option<Instant>,
    states_big: Option<Instant>,
    municipal: Option<Instant>,
    /// The border set shown before the current one, and when it was replaced: a new
    /// tile level adds and drops lines (coarser OpenMapTiles levels leave out short
    /// borders), which popped in and out without this exchange.
    exchanged: Option<(Arc<BoundaryData>, Instant)>,
}
impl LayerArrivals {
    fn observe(&mut self, old: &BoundaryData, new: &BoundaryData, now: Instant) {
        let arrived = |old: bool, new: bool| old && !new;
        let states = |data: &BoundaryData| data.osm_borders.state.is_empty();
        let states_big = |data: &BoundaryData| data.osm_borders.state_big.is_empty();
        if arrived(states(old), states(new)) {
            self.states = Some(now);
        }
        if arrived(states_big(old), states_big(new)) {
            self.states_big = Some(now);
        }
        let municipal = |data: &BoundaryData| data.osm_borders.municipal.is_empty();
        if arrived(municipal(old), municipal(new)) {
            self.municipal = Some(now);
        }
        let lines = |data: &BoundaryData| {
            let osm = &data.osm_borders;
            !(osm.country.is_empty() && osm.state.is_empty() && osm.state_big.is_empty())
        };
        if lines(old) && !Arc::ptr_eq(&old.osm_borders, &new.osm_borders) {
            self.exchanged = Some((Arc::new(old.clone()), now));
        }
    }
    /// The border set fading out, while it does.
    pub(super) fn fading_out(&mut self, now: Instant) -> Option<Arc<BoundaryData>> {
        if self
            .exchanged
            .as_ref()
            .is_some_and(|(_, at)| now.duration_since(*at) >= EXCHANGE_FADE)
        {
            self.exchanged = None;
        }
        self.exchanged.as_ref().map(|(data, _)| data.clone())
    }
    fn fade(&self, now: Instant) -> ArrivalFade {
        let step = |arrived: Option<Instant>| {
            arrived.map_or(ARRIVAL_FADE_STEPS, |at| {
                let progress = now.duration_since(at).as_secs_f32() / ARRIVAL_FADE.as_secs_f32();
                (progress.min(1.0) * f32::from(ARRIVAL_FADE_STEPS)).round() as u8
            })
        };
        let exchange = self
            .exchanged
            .as_ref()
            .map_or(ARRIVAL_FADE_STEPS, |(_, at)| {
                let progress = now.duration_since(*at).as_secs_f32() / EXCHANGE_FADE.as_secs_f32();
                (progress.min(1.0) * f32::from(ARRIVAL_FADE_STEPS)).round() as u8
            });
        ArrivalFade {
            states: step(self.states),
            states_big: step(self.states_big),
            municipal: step(self.municipal),
            exchange,
        }
    }
}

impl App {
    pub(super) fn invalidate_font_layout(&mut self) {
        self.overlay_cache.invalidate();
        self.aircraft_overlay.invalidate_text();
        self.world_shapes = super::live_layers::WorldShapes::default();
        self.roads.invalidate_text();
    }
    pub(super) fn overlay_layers(&self) -> globecraft_render::overlays::OverlayLayers {
        OverlayLayers {
            countries: self.layer(MapLayer::CountryBorders),
            states: self.layer(MapLayer::StateBorders),
            cities: self.layer(MapLayer::MunicipalBorders),
            labels: self.layer(MapLayer::PlaceLabels),
            sun: self.layer(MapLayer::Sunlight).then(|| {
                let interval = globecraft_render::view::SCENE_TIME_UPDATE_INTERVAL_SECS;
                let time = (globecraft_core::math::now_unix() / interval).floor() * interval;
                globecraft_core::math::subsolar(time)
            }),
        }
    }

    pub(super) fn merge_snapshot(&mut self, snapshot: BoundarySnapshot) {
        self.layer_arrivals
            .observe(&self.boundaries, &snapshot.data, Instant::now());
        self.boundaries = snapshot.data;
        if let Err(error) = self.local_search.update_shared(snapshot.names) {
            self.status = format!("Local prefix index failed: {error}");
        }
        // Data-only change: hold the previous overlay on screen until the
        // replacement is prepared instead of flashing the borders away.
        self.overlay_cache.merge_boundaries();
    }

    pub(super) fn draw_overlays(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        altitude_km: f64,
    ) {
        let arrival = self.layer_arrivals.fade(Instant::now());
        if !arrival.is_settled() {
            painter.ctx().request_repaint();
        }
        let style = OverlayStyle {
            layers: self.overlay_layers(),
            map_scale: self.settings.map_scale,
            lang: self.lang,
            arrival,
            map_style: self.settings.map_style,
        };
        meshes::BorderWidths::of(&style.map_style).publish(painter.ctx());
        self.overlay_cache.draw(
            painter,
            OverlayView {
                basis: *basis,
                rect,
                altitude_km,
                state_max_km: self.settings.state_max_altitude_km,
                grid_step_deg: if self.layer(MapLayer::GridLabels) {
                    self.renderer.displayed_grid_step()
                } else {
                    None
                },
                pixels_per_point: painter.ctx().pixels_per_point(),
                ground: self.renderer.displayed_ground(self.overlay_ground()),
            },
            &self.boundaries,
            style,
        );
        let ms = self.settings.map_scale;
        // The ruler and placemarks sit on the relief, where they are picked.
        let source = self.ground_source();
        let heights = source.heights();
        let ground = heights
            .as_ref()
            .map(|heights| heights as &dyn globecraft_render::overlays::Ground);
        let on_ground = |lat: f64, lon: f64| {
            globecraft_render::overlays::ground_position(basis, ground, lat, lon)
        };
        // ruler
        let stroke = Stroke::new(2.5_f32, Color32::from_rgb(255, 80, 80));
        for leg in globecraft_render::overlays::ruler::project_ruler(
            basis,
            rect,
            &self.ruler_paths,
            ground,
        ) {
            for segment in leg.segments {
                painter.line_segment(segment, stroke);
            }
            if let Some(mid) = leg.label_position {
                let txt = self.settings.ruler_unit.format_with_angle(
                    leg.distance_km,
                    leg.angle_degrees,
                    self.distance_units(),
                    self.lang,
                );
                painter.text(
                    mid + Vec2::new(1.0, 1.0),
                    Align2::CENTER_BOTTOM,
                    &txt,
                    crate::app::fonts::map_font(painter.ctx(), 13.0 * ms),
                    Color32::BLACK,
                );
                painter.text(
                    mid,
                    Align2::CENTER_BOTTOM,
                    &txt,
                    crate::app::fonts::map_font(painter.ctx(), 13.0 * ms),
                    Color32::WHITE,
                );
            }
        }
        for p in self.ruler_paths.points() {
            if let Some(s) = basis.project(on_ground(p.0, p.1), rect) {
                painter.circle_filled(s, 4.0, Color32::WHITE);
                painter.circle_stroke(s, 4.0, Stroke::new(1.5_f32, Color32::from_rgb(255, 80, 80)));
            }
        }
        // placemarks
        for pm in self.placemarks.markers(true) {
            if let Some(s) = basis.project(on_ground(pm.lat, pm.lon), rect)
                && rect.contains(s)
            {
                painter.circle_filled(s, 6.0, Color32::from_rgb(255, 200, 0));
                painter.circle_stroke(s, 6.0, Stroke::new(1.5_f32, Color32::BLACK));
                let tp = s + Vec2::new(9.0, -9.0);
                painter.text(
                    tp + Vec2::new(1.0, 1.0),
                    Align2::LEFT_BOTTOM,
                    &pm.name,
                    crate::app::fonts::map_font(painter.ctx(), 13.0 * ms),
                    Color32::BLACK,
                );
                painter.text(
                    tp,
                    Align2::LEFT_BOTTOM,
                    &pm.name,
                    crate::app::fonts::map_font(painter.ctx(), 13.0 * ms),
                    Color32::WHITE,
                );
            }
        }
    }
}
