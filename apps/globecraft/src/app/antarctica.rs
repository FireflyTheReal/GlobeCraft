//! The Antarctic territorial claims, drawn with the country borders in the disputed
//! style: the claims' sides from 60° S to the South Pole, and each claim's name.
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
const LABEL_HALO: Color32 = Color32::from_rgba_premultiplied(10, 12, 16, 220);
/// Names are left out where they would come this close to one already placed.
const LABEL_GAP_POINTS: f32 = 4.0;
/// The casing under the claim lines: this much wider, in this colour.
const CASING_POINTS: f32 = 2.0;
const CASING_RGBA: [u8; 4] = [16, 22, 32, 190];

impl App {
    /// The claim lines and names, under the borders and names; only while country
    /// borders show and the view reaches south of 55° S.
    pub(super) fn paint_antarctic_claims(
        &self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
    ) {
        if !self.layer(MapLayer::CountryBorders) {
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
            let galley = painter.layout_no_wrap(text.clone(), font.clone(), LABEL);
            let corner = Align2::CENTER_CENTER.anchor_size(at, galley.size()).min;
            let bounds =
                Rect::from_min_size(corner, galley.size()).expand(LABEL_GAP_POINTS * scale);
            if taken.iter().any(|other| other.intersects(bounds)) {
                continue;
            }
            taken.push(bounds);
            let halo = painter.layout_no_wrap(text, font.clone(), LABEL_HALO);
            for offset in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, -1.0)] {
                shapes.push(Shape::galley(
                    corner + offset * scale,
                    halo.clone(),
                    LABEL_HALO,
                ));
            }
            shapes.push(Shape::galley(corner, galley, LABEL));
        }
        painter.extend(shapes);
    }
}
