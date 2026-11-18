//! Egui texture ownership and repaint scheduling for data-owned live imagery: the
//! cloud layer (a mesh over the frame, `globecraft_render::clouds`) and the radar.
use eframe::egui::{self, Color32, ColorImage, Mesh, Pos2, Rect, TextureHandle, TextureOptions};
use globecraft_core::{camera::Basis, language::Lang};
use globecraft_data::live_maps::CloudMap;
use globecraft_render::clouds::{CLOUD_OPACITY, CloudLight, cloud_level, cloud_shell};
use std::sync::Arc;
use std::time::Duration;
/// What the radar was projected for: view, size, its opacity bits and the imagery's
/// generation.
type ProjectionKey = (Basis, usize, usize, u32, u64);
/// A corner of the clouds' mesh every this many points: the map is laid straight
/// across each cell, which stays within a pixel at this size.
const CLOUD_MESH_STEP_POINTS: f32 = 8.0;
#[derive(Default)]
pub(super) struct LiveMaps {
    service: globecraft_data::live_maps::LiveMaps,
    projected: Option<TextureHandle>,
    projected_for: Option<ProjectionKey>,
    /// The cloud map's levels as textures, made when first drawn, for the map they
    /// were made from.
    cloud_textures: Option<(Arc<CloudMap>, Vec<Option<TextureHandle>>)>,
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
    /// Draws the clouds and the radar at their opacities (roadmap 6.1), `None` while
    /// hidden; the clouds lit as `light` says.
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        clouds: Option<f32>,
        radar: Option<f32>,
        light: CloudLight,
    ) {
        match clouds {
            Some(opacity) => self.draw_clouds(painter, basis, rect, opacity, light),
            // Hidden, the cloud textures (up to 170 MB on the graphics card) go.
            None => self.cloud_textures = None,
        }
        let Some((image, opacity)) = self.service.radar().zip(radar) else {
            return;
        };
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
            opacity.to_bits(),
            self.service.generation(),
        );
        if self.projected_for != Some(key) {
            let rgba = globecraft_render::weather::project_radar(
                basis,
                rect,
                width,
                height,
                (image, opacity),
            );
            let im = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            if let Some(tex) = self.projected.as_mut() {
                tex.set(im, TextureOptions::LINEAR);
            } else {
                self.projected = Some(painter.ctx().load_texture(
                    "live-weather-radar",
                    im,
                    TextureOptions::LINEAR,
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

    /// The cloud layer 8 km up, from the level of the map that suits the view.
    fn draw_clouds(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        opacity: f32,
        light: CloudLight,
    ) {
        let Some(map) = self.service.cloud().cloned() else {
            return;
        };
        let ctx = painter.ctx();
        let Some(shell) = cloud_shell(
            basis,
            rect,
            CLOUD_MESH_STEP_POINTS,
            ctx.pixels_per_point(),
            light,
        ) else {
            return;
        };
        // Levels wider than the graphics card takes are left out.
        let max_side = ctx.input(|i| i.max_texture_side);
        let usable: Vec<usize> = (0..map.levels.len())
            .filter(|&level| map.levels[level].width() as usize <= max_side)
            .collect();
        let widths: Vec<u32> = usable.iter().map(|&l| map.levels[l].width()).collect();
        let Some(&level) = usable.get(cloud_level(&widths, shell.pixel_km)) else {
            return;
        };
        if !self
            .cloud_textures
            .as_ref()
            .is_some_and(|(shown, _)| Arc::ptr_eq(shown, &map))
        {
            self.cloud_textures = Some((map.clone(), vec![None; map.levels.len()]));
        }
        let textures = &mut self.cloud_textures.as_mut().expect("set above").1;
        let texture = textures[level].get_or_insert_with(|| {
            let image = &map.levels[level];
            // White, as opaque as the cloud (premultiplied).
            let pixels = image
                .as_raw()
                .iter()
                .map(|&a| Color32::from_rgba_premultiplied(a, a, a, a))
                .collect();
            ctx.load_texture(
                format!("live-clouds-{}", image.width()),
                ColorImage::new([image.width() as usize, image.height() as usize], pixels),
                TextureOptions::LINEAR_REPEAT,
            )
        });
        let alpha = opacity * CLOUD_OPACITY * shell.fade;
        let mut mesh = Mesh::with_texture(texture.id());
        let row = shell.columns + 1;
        for y in 0..shell.rows {
            for x in 0..shell.columns {
                let corners = [
                    y * row + x,
                    y * row + x + 1,
                    (y + 1) * row + x + 1,
                    (y + 1) * row + x,
                ]
                .map(|index| shell.corners[index]);
                let [Some(first), ..] = corners else {
                    continue;
                };
                if corners.iter().any(Option::is_none) {
                    continue;
                }
                let base = mesh.vertices.len() as u32;
                for corner in corners.into_iter().flatten() {
                    // Across the map's edge the picture goes on (it repeats) rather
                    // than running back across the whole Earth.
                    let u = corner.uv[0] + (first.uv[0] - corner.uv[0]).round();
                    let alpha = alpha * corner.opacity;
                    let [r, g, b] = corner.colour.map(|c| (c * alpha * 255.0) as u8);
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: corner.pos,
                        uv: Pos2::new(u as f32, corner.uv[1] as f32),
                        color: Color32::from_rgba_premultiplied(r, g, b, (alpha * 255.0) as u8),
                    });
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
        painter.add(mesh);
    }

    /// The cloud map, once loaded.
    pub fn cloud_map(&self) -> Option<Arc<CloudMap>> {
        self.service.cloud().cloned()
    }

    pub fn status(&self, clouds: bool, radar: bool, lang: Lang) -> String {
        self.service.status(clouds, radar, lang)
    }
}
