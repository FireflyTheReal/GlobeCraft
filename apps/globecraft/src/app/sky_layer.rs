//! Space behind the globe's frame, drawn at the screen's full resolution: the Milky
//! Way and the stars (the sun is in the frame, through the air). Frames render at a fraction of the size while the view moves,
//! which blurred them and made the stars blink; they and the Milky Way must always
//! show at full resolution (user, 2026-11-17). The frame leaves its sky transparent
//! but for the air's glow (`RenderCfg::sky_behind`).
use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect, TextureHandle, TextureOptions};
use glam::DVec2;
use globecraft_core::{camera::Basis, celestial::CelestialFrame};
use globecraft_data::sky::SkyImage;
use globecraft_render::celestial::SkyBackdrop;
use std::sync::Arc;

/// Cell size of the Milky Way's mesh, in points. The picture is mapped straight
/// across each cell, which stays well under a pixel off at this size.
const MESH_STEP_POINTS: f32 = 12.0;

/// The sky the displayed frame leaves to this layer, seen with that frame's camera.
#[derive(Clone)]
pub(super) struct SkyBehind {
    pub(super) basis: Basis,
    pub(super) backdrop: Option<SkyBackdrop>,
    /// The stars at this moment of the sky, when shown.
    pub(super) stars: Option<CelestialFrame>,
}

#[derive(Default)]
pub(super) struct SkyLayer {
    texture: Option<(Arc<SkyImage>, TextureHandle)>,
}

impl SkyLayer {
    /// Paints space into `rect`, where the frame goes over it.
    pub(super) fn draw(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        rect: Rect,
        sky: &SkyBehind,
    ) {
        match &sky.backdrop {
            Some(backdrop) => painter.add(self.milky_way(ctx, rect, &sky.basis, backdrop)),
            None => {
                let [r, g, b] = globecraft_render::space_color().map(|c| (c * 255.0) as u8);
                painter.rect_filled(rect, 0.0, Color32::from_rgb(r, g, b))
            }
        };
        if let Some(frame) = sky.stars {
            painter.add(stars(rect, &sky.basis, frame, ctx.pixels_per_point()));
        }
    }

    fn milky_way(
        &mut self,
        ctx: &egui::Context,
        rect: Rect,
        basis: &Basis,
        backdrop: &SkyBackdrop,
    ) -> Mesh {
        let image = backdrop.image();
        if !self
            .texture
            .as_ref()
            .is_some_and(|(shown, _)| Arc::ptr_eq(shown, image))
        {
            let pixels = image.pixels();
            let picture = egui::ColorImage::from_rgb(
                [pixels.width() as usize, pixels.height() as usize],
                pixels.as_raw(),
            );
            let texture = ctx.load_texture("milky-way", picture, TextureOptions::LINEAR_REPEAT);
            self.texture = Some((image.clone(), texture));
        }
        let texture = &self.texture.as_ref().expect("loaded above").1;
        let (columns, rows) = (
            (rect.width() / MESH_STEP_POINTS).ceil().max(1.0) as usize,
            (rect.height() / MESH_STEP_POINTS).ceil().max(1.0) as usize,
        );
        let at = |column: usize, row: usize| {
            Pos2::new(
                rect.min.x + rect.width() * column as f32 / columns as f32,
                rect.min.y + rect.height() * row as f32 / rows as f32,
            )
        };
        let uvs: Vec<DVec2> = (0..=rows)
            .flat_map(|row| (0..=columns).map(move |column| (column, row)))
            .map(|(column, row)| backdrop.uv(basis.ray(rect, at(column, row))))
            .collect();
        let shade = Color32::from_gray((backdrop.visibility().clamp(0.0, 1.0) * 255.0) as u8);
        let mut mesh = Mesh::with_texture(texture.id());
        for row in 0..rows {
            for column in 0..columns {
                let corners = [
                    (column, row),
                    (column + 1, row),
                    (column + 1, row + 1),
                    (column, row + 1),
                ];
                let first = uvs[row * (columns + 1) + column];
                let base = mesh.vertices.len() as u32;
                for (column, row) in corners {
                    let uv = uvs[row * (columns + 1) + column];
                    // Across the map's edge the picture goes on (it repeats) rather
                    // than running back across the whole sky.
                    let u = uv.x + (first.x - uv.x).round();
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: at(column, row),
                        uv: Pos2::new(u as f32, uv.y as f32),
                        color: shade,
                    });
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
        mesh
    }
}

/// The stars as whole-pixel squares: a star smaller than a pixel lights one pixel
/// with its light spread over it, so none blinks as the view moves.
fn stars(rect: Rect, basis: &Basis, frame: CelestialFrame, pixels_per_point: f32) -> Mesh {
    let scale = f64::from(pixels_per_point);
    let (width, height) = (
        f64::from(rect.width()) * scale,
        f64::from(rect.height()) * scale,
    );
    let mut mesh = Mesh::default();
    for dot in globecraft_render::stars::seen(basis, frame, width, height, scale) {
        let side = dot.size.round().max(1.0);
        let spread = (dot.size / side).powi(2).min(1.0);
        let corner = (rect.min.to_vec2() * pixels_per_point + dot.centre.to_vec2()
            - egui::Vec2::splat(side * 0.5))
        .round();
        let square = Rect::from_min_size(
            (corner / pixels_per_point).to_pos2(),
            egui::Vec2::splat(side / pixels_per_point),
        );
        let [r, g, b] = dot
            .light
            .map(|c| ((c * spread).clamp(0.0, 1.0) * 255.0) as u8);
        // Alpha 0: the star's light adds to the Milky Way behind it.
        mesh.add_colored_rect(square, Color32::from_rgba_premultiplied(r, g, b, 0));
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;

    #[test]
    fn stars_land_on_whole_pixels_where_the_catalogue_puts_them() {
        let cam = Cam {
            lat: 10.0,
            lon: 20.0,
            alt: 20_000.0,
            heading: 30.0,
            tilt: 0.0,
        };
        let basis = cam.basis(1.6);
        let frame = CelestialFrame::at_unix(1_763_000_000.0).unwrap();
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(800.0, 500.0));
        let scale = 2.0;
        let mesh = stars(rect, &basis, frame, scale);
        let dots = globecraft_render::stars::seen(&basis, frame, 1600.0, 1000.0, 2.0);
        assert!(!dots.is_empty(), "some stars are in view");
        assert_eq!(mesh.vertices.len(), dots.len() * 4);
        for (square, dot) in mesh.vertices.chunks(4).zip(&dots) {
            // Corners on whole pixels, the middle within half a pixel of the star.
            let corner = square[0].pos.to_vec2() * scale;
            assert_eq!(corner, corner.round());
            let middle =
                Rect::from_points(&square.iter().map(|v| v.pos).collect::<Vec<_>>()).center();
            let star = rect.min + dot.centre.to_vec2() / scale;
            assert!((middle - star).abs().max_elem() * scale <= 0.5 + 1e-3);
        }
    }
}
