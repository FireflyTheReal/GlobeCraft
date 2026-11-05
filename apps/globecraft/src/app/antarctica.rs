//! The Antarctic territorial claims, their own layer (on and off apart from the
//! country borders), in the borders' disputed style: the claims' sides from 60° S to
//! the South Pole, and each claim's name, outlined to stand out on the ice.
use super::App;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Shape};
use globecraft_core::{camera::Basis, layers::MapLayer};
use globecraft_data::{
    antarctic_claims::{CLAIMS, NORTH_LIMIT_DEG, boundaries},
    settings::map_style::{LineStyle, LineType, Rgba},
};
use globecraft_render::overlays::project_unhidden;

/// The claim lines get a point every this many degrees of latitude.
const STEP_DEG: f64 = 0.5;
const LABEL_POINTS: f32 = 11.0;
const LABEL: Color32 = Color32::from_rgb(232, 236, 242);
/// A thin dark outline, one pixel in each of eight directions: it keeps the white
/// names legible on the white ice (the earlier halo, two copies offset by a point and
/// a half, read as doubled letters, and a country name's shadow vanished on ice).
const LABEL_OUTLINE: Color32 = Color32::from_rgba_premultiplied(12, 16, 24, 230);
const OUTLINE_DIRECTIONS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (0.7, 0.7),
    (-0.7, 0.7),
    (0.7, -0.7),
    (-0.7, -0.7),
];
/// Names are left out where they would come this close to one already placed.
const LABEL_GAP_POINTS: f32 = 4.0;
/// The casing under the claim lines: this much wider, in this colour.
const CASING_POINTS: f32 = 2.0;
const CASING_RGBA: [u8; 4] = [16, 22, 32, 190];

/// A name with a one-pixel outline all round, in `outline`'s colour; the time zone
/// labels use it too.
pub(super) fn outlined(
    shapes: &mut Vec<Shape>,
    painter: &egui::Painter,
    corner: Pos2,
    galley: std::sync::Arc<egui::Galley>,
    outline: Color32,
) {
    let pixel = 1.0 / painter.ctx().pixels_per_point();
    for (x, y) in OUTLINE_DIRECTIONS {
        shapes.push(Shape::galley_with_override_text_color(
            corner + egui::vec2(x, y) * pixel,
            galley.clone(),
            outline,
        ));
    }
    let colour = galley
        .job
        .sections
        .first()
        .map_or(Color32::WHITE, |section| section.format.color);
    shapes.push(Shape::galley(corner, galley, colour));
}

impl App {
    /// The claim lines and names, under the borders and names; only while their
    /// layer is on and the view reaches south of 60° S.
    pub(super) fn paint_antarctic_claims(
        &self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
    ) {
        if !self.layer(MapLayer::AntarcticClaims) {
            return;
        }
        let scale = self.settings.map_scale;
        let style = *self.settings.map_style.line(LineType::Disputed);
        // A dark casing under the user's style: the ice is as white as the default.
        let casing = LineStyle {
            color: Rgba(CASING_RGBA),
            width_points: style.width_points + CASING_POINTS,
            ..style
        };
        let mut shapes = Vec::new();
        let steps = ((90.0 + NORTH_LIMIT_DEG) / STEP_DEG).round() as usize;
        for lon in boundaries() {
            let line = (0..=steps).map(|i| (NORTH_LIMIT_DEG - i as f64 * STEP_DEG, lon));
            for run in super::selected_flight::screen_runs(basis, rect, line) {
                super::live_layers::styled_line(&mut shapes, &run, &casing, scale, 1.0);
                super::live_layers::styled_line(&mut shapes, &run, &style, scale, 1.0);
            }
        }
        if shapes.is_empty() {
            return;
        }
        let font = super::fonts::map_font(painter.ctx(), LABEL_POINTS * scale);
        let mut taken: Vec<Rect> = Vec::new();
        for claim in &CLAIMS {
            let Some(at) = project_unhidden(
                basis,
                rect,
                None,
                None,
                claim.label_lat_deg,
                claim.middle_lon_deg(),
            )
            .map(|p| Pos2::new(p.x as f32, p.y as f32))
            .filter(|at| rect.contains(*at)) else {
                continue;
            };
            let text = self.lang.text(claim.name_key).to_owned();
            let galley = painter.layout_no_wrap(text, font.clone(), LABEL);
            let corner = Align2::CENTER_CENTER.anchor_size(at, galley.size()).min;
            let bounds =
                Rect::from_min_size(corner, galley.size()).expand(LABEL_GAP_POINTS * scale);
            if taken.iter().any(|other| other.intersects(bounds)) {
                continue;
            }
            taken.push(bounds);
            outlined(&mut shapes, painter, corner, galley, LABEL_OUTLINE);
        }
        painter.extend(shapes);
    }
}
