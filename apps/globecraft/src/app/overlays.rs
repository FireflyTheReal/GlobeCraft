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
use std::time::{Duration, Instant};

/// How long a streamed boundary layer takes to fade in once its data arrives.
const ARRIVAL_FADE: Duration = Duration::from_millis(600);

/// When each streamed boundary layer last went from empty to loaded.
#[derive(Default)]
pub(super) struct LayerArrivals {
    states: Option<Instant>,
    states_big: Option<Instant>,
    municipal: Option<Instant>,
}
impl LayerArrivals {
    fn observe(&mut self, old: &BoundaryData, new: &BoundaryData, now: Instant) {
        let arrived = |old: bool, new: bool| old && !new;
        if arrived(old.states.is_empty(), new.states.is_empty()) {
            self.states = Some(now);
        }
        if arrived(old.states_big.is_empty(), new.states_big.is_empty()) {
            self.states_big = Some(now);
        }
        let municipal =
            |data: &BoundaryData| data.cities.is_empty() && data.world_cities.is_empty();
        if arrived(municipal(old), municipal(new)) {
            self.municipal = Some(now);
        }
    }
    fn fade(&self, now: Instant) -> ArrivalFade {
        let step = |arrived: Option<Instant>| {
            arrived.map_or(ARRIVAL_FADE_STEPS, |at| {
                let progress = now.duration_since(at).as_secs_f32() / ARRIVAL_FADE.as_secs_f32();
                (progress.min(1.0) * f32::from(ARRIVAL_FADE_STEPS)).round() as u8
            })
        };
        ArrivalFade {
            states: step(self.states),
            states_big: step(self.states_big),
            municipal: step(self.municipal),
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
            },
            &self.boundaries,
            style,
        );
        let ms = self.settings.map_scale;
        // ruler
        let stroke = Stroke::new(2.5_f32, Color32::from_rgb(255, 80, 80));
        for leg in globecraft_render::overlays::ruler::project_ruler(basis, rect, &self.ruler_paths)
        {
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
            if let Some(s) = basis.project(basis.surface_position(p.0, p.1), rect) {
                painter.circle_filled(s, 4.0, Color32::WHITE);
                painter.circle_stroke(s, 4.0, Stroke::new(1.5_f32, Color32::from_rgb(255, 80, 80)));
            }
        }
        // placemarks
        for pm in self.placemarks.markers(true) {
            if let Some(s) = basis.project(basis.surface_position(pm.lat, pm.lon), rect)
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
