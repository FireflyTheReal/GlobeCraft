//! Egui texture ownership and repaint scheduling for data-owned live imagery.
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, TextureHandle};
use globecraft_core::{camera::Basis, language::Lang};
use std::time::Duration;
/// What the live maps were projected for: view, size, each layer's opacity bits
/// (none while hidden) and the imagery's generation.
type ProjectionKey = (Basis, usize, usize, Option<u32>, Option<u32>, u64);
#[derive(Default)]
pub(super) struct LiveMaps {
    service: globecraft_data::live_maps::LiveMaps,
    projected: Option<TextureHandle>,
    projected_for: Option<ProjectionKey>,
}
impl LiveMaps {
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        clouds: bool,
        radar: bool,
    ) {
        let wake = ctx.clone();
        self.service
            .update(runtime, clouds, radar, move || wake.request_repaint());
        if clouds || radar {
            ctx.request_repaint_after(Duration::from_secs(60));
        }
    }
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        clouds: Option<f32>,
        radar: Option<f32>,
    ) {
        // Each shown at its opacity (roadmap 6.1).
        let clouds = clouds.filter(|_| self.service.cloud().is_some());
        let radar = radar.filter(|_| self.service.radar().is_some());
        if clouds.is_none() && radar.is_none() {
            return;
        }
        // Projection is cached and updated only when camera or imagery changes.
        let max_width = if painter.ctx().input(|i| i.pointer.any_down()) {
            480
        } else {
            960
        };
        let width =
            ((rect.width() * painter.ctx().pixels_per_point()) as usize).clamp(160, max_width);
        let height =
            ((width as f32 * rect.height() / rect.width().max(1.0)) as usize).clamp(1, 960);
        let key = (
            *basis,
            width,
            height,
            clouds.map(f32::to_bits),
            radar.map(f32::to_bits),
            self.service.generation(),
        );
        if self.projected_for != Some(key) {
            let rgba = globecraft_render::weather::project_live_maps(
                basis,
                rect,
                width,
                height,
                self.service.cloud().zip(clouds),
                self.service.radar().zip(radar),
            );
            let im = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            if let Some(tex) = self.projected.as_mut() {
                tex.set(im, egui::TextureOptions::LINEAR);
            } else {
                self.projected = Some(painter.ctx().load_texture(
                    "live-weather-cloud-map",
                    im,
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.projected_for = Some(key);
        }
        if let Some(tex) = &self.projected {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    pub fn status(&self, clouds: bool, radar: bool, lang: Lang) -> String {
        self.service.status(clouds, radar, lang)
    }
}
