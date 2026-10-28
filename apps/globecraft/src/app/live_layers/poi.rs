//! Places of interest, peaks, waters and parks: rivers and park outlines under the
//! borders, names over them, styled once per projected geometry.
use eframe::egui::{self, Align2, Color32, FontId, Shape, Stroke, Vec2};
use globecraft_data::poi::{PoiLineKind, PointKind};
use globecraft_render::live_layers::LiveGeometry;
use std::sync::Arc;

const LABEL_FONT_POINTS: f32 = 11.0;
const WATER_FONT_POINTS: f32 = 12.0;
const MARKER_POINTS: f32 = 3.2;
const LABEL_OFFSET_POINTS: f32 = 7.0;
const RIVER: Color32 = Color32::from_rgb(78, 150, 235);
const PARK: Color32 = Color32::from_rgb(92, 180, 96);
const PEAK: Color32 = Color32::from_rgb(150, 105, 70);
const POI: Color32 = Color32::from_rgb(226, 92, 76);
const LABEL: Color32 = Color32::from_rgb(240, 244, 250);
const WATER_LABEL: Color32 = Color32::from_rgb(150, 200, 255);

#[derive(Default)]
pub(in crate::app) struct PoiShapes {
    key: Option<(usize, u32)>,
    lines: Vec<Shape>,
    labels: Vec<Shape>,
}

impl PoiShapes {
    pub(in crate::app) fn invalidate(&mut self) {
        self.key = None;
    }
    fn refresh(&mut self, painter: &egui::Painter, geometry: &LiveGeometry) {
        let key = (
            Arc::as_ptr(&geometry.poi) as usize,
            geometry.input.view.map_scale.to_bits(),
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let scale = geometry.input.view.map_scale;
        self.lines = super::super::live_geometry::mesh_shapes(painter, lines(geometry, scale));
        self.labels =
            super::super::live_geometry::mesh_shapes(painter, labels(painter, geometry, scale));
    }
    /// Rivers, canals and park outlines, drawn under the borders.
    pub(in crate::app) fn lines(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
    ) -> &[Shape] {
        self.refresh(painter, geometry);
        &self.lines
    }
    /// Markers and names, drawn over the borders.
    pub(in crate::app) fn labels(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
    ) -> &[Shape] {
        self.refresh(painter, geometry);
        &self.labels
    }
}

fn lines(geometry: &LiveGeometry, scale: f32) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for line in &geometry.poi.lines {
        let (width, color, dashed) = match line.kind {
            PoiLineKind::River => (1.6, RIVER, false),
            PoiLineKind::Canal => (1.1, RIVER, false),
            PoiLineKind::Park => (1.2, PARK.gamma_multiply(0.8), true),
        };
        let stroke = Stroke::new(width * scale, color);
        for segment in &line.segments {
            if dashed {
                shapes.extend(Shape::dashed_line(
                    segment,
                    stroke,
                    4.0 * scale,
                    3.0 * scale,
                ));
            } else {
                shapes.push(Shape::line(segment.clone(), stroke));
            }
        }
    }
    shapes
}

fn labels(painter: &egui::Painter, geometry: &LiveGeometry, scale: f32) -> Vec<Shape> {
    let Some(snapshot) = &geometry.input.poi else {
        return Vec::new();
    };
    let font = crate::app::fonts::map_font(painter.ctx(), LABEL_FONT_POINTS * scale);
    let water_font = FontId::new(WATER_FONT_POINTS * scale, font.family.clone());
    let mut markers = Vec::new();
    let mut texts = Vec::new();
    for label in &geometry.poi.labels {
        let Some(point) = snapshot
            .tiles
            .get(label.tile)
            .and_then(|tile| tile.points.get(label.index))
        else {
            continue;
        };
        let at = label.position;
        let r = MARKER_POINTS * scale;
        let (text, color, font, anchor) = match point.kind {
            PointKind::Water => (
                point.name.clone(),
                WATER_LABEL,
                water_font.clone(),
                Align2::CENTER_CENTER,
            ),
            PointKind::Peak => {
                markers.push(Shape::convex_polygon(
                    vec![
                        at + Vec2::new(0.0, -r * 1.4),
                        at + Vec2::new(r * 1.3, r),
                        at + Vec2::new(-r * 1.3, r),
                    ],
                    PEAK,
                    Stroke::new(1.0 * scale, Color32::WHITE),
                ));
                let text = match point.elevation_m {
                    Some(metres) => format!("{} {metres} m", point.name),
                    None => point.name.clone(),
                };
                (text, LABEL, font.clone(), Align2::LEFT_CENTER)
            }
            PointKind::Park => {
                markers.push(Shape::circle_filled(at, r, PARK));
                (
                    point.name.clone(),
                    Color32::from_rgb(190, 240, 190),
                    font.clone(),
                    Align2::LEFT_CENTER,
                )
            }
            PointKind::Poi => {
                markers.push(Shape::circle_filled(at, r, POI));
                markers.push(Shape::circle_stroke(
                    at,
                    r,
                    Stroke::new(1.0 * scale, Color32::WHITE),
                ));
                (point.name.clone(), LABEL, font.clone(), Align2::LEFT_CENTER)
            }
        };
        let offset = if anchor == Align2::CENTER_CENTER {
            Vec2::ZERO
        } else {
            Vec2::new(LABEL_OFFSET_POINTS * scale, 0.0)
        };
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), color);
        let position = anchor.anchor_size(at + offset, galley.size()).min;
        let shadow = painter.layout_no_wrap(text, font, Color32::BLACK);
        texts.push(Shape::galley(
            position + Vec2::splat(1.0),
            shadow,
            Color32::BLACK,
        ));
        texts.push(Shape::galley(position, galley, color));
    }
    markers.extend(texts);
    markers
}
