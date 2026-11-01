//! Places of interest, peaks, waters and parks: rivers and park outlines under the
//! borders, names over them, styled once per projected geometry.
use super::fade::{FadingShapes, styled_line};
use eframe::egui::{self, Align2, Color32, FontId, Shape, Stroke, Vec2};
use globecraft_core::language::Lang;
use globecraft_data::poi::{PoiLineKind, PoiPoint, PointKind};
use globecraft_data::settings::map_style::{LineType, MapStyle};
use globecraft_render::live_layers::{LiveGeometry, LiveView};
use std::sync::Arc;
use std::time::Instant;

const HOVER_RADIUS_POINTS: f32 = 12.0;

const LABEL_FONT_POINTS: f32 = 11.0;
const WATER_FONT_POINTS: f32 = 12.0;
const MARKER_POINTS: f32 = 3.2;
const LABEL_OFFSET_POINTS: f32 = 7.0;
const PARK: Color32 = Color32::from_rgb(92, 180, 96);
const PARK_LABEL: Color32 = Color32::from_rgb(190, 240, 190);
const PEAK: Color32 = Color32::from_rgb(150, 105, 70);
const POI: Color32 = Color32::from_rgb(226, 92, 76);
const LABEL: Color32 = Color32::from_rgb(240, 244, 250);
const WATER_LABEL: Color32 = Color32::from_rgb(150, 200, 255);

#[derive(Default)]
pub(in crate::app) struct PoiShapes {
    /// What the shapes were built from: the projected geometry, scale and style.
    key: Option<(usize, u32, MapStyle)>,
    /// The view and data snapshot of the last build, to tell new tiles from a move.
    source: Option<(LiveView, usize)>,
    lines: FadingShapes,
    labels: FadingShapes,
}

impl PoiShapes {
    pub(in crate::app) fn invalidate(&mut self) {
        self.key = None;
    }
    fn refresh(&mut self, painter: &egui::Painter, geometry: &LiveGeometry, style: &MapStyle) {
        let key = (
            Arc::as_ptr(&geometry.poi) as usize,
            geometry.input.view.map_scale.to_bits(),
            *style,
        );
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        let view = geometry.input.view;
        let data = geometry
            .input
            .poi
            .as_ref()
            .map_or(0, |snapshot| Arc::as_ptr(snapshot) as usize);
        let new_data = self
            .source
            .is_some_and(|(old_view, old_data)| old_view.same_frame(&view) && old_data != data);
        self.source = Some((view, data));
        let now = Instant::now();
        let scale = view.map_scale;
        self.lines.replace(
            super::super::live_geometry::mesh_shapes(painter, lines(geometry, style, scale)),
            new_data,
            now,
        );
        self.labels.replace(
            super::super::live_geometry::mesh_shapes(painter, labels(painter, geometry, scale)),
            new_data,
            now,
        );
    }
    /// Rivers, canals and park outlines, drawn under the borders.
    pub(in crate::app) fn draw_lines(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        style: &MapStyle,
    ) {
        self.refresh(painter, geometry, style);
        self.lines.draw(painter, Instant::now());
    }
    /// Markers and names, drawn over the borders.
    pub(in crate::app) fn draw_labels(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        style: &MapStyle,
    ) {
        self.refresh(painter, geometry, style);
        self.labels.draw(painter, Instant::now());
    }
    /// The nearest name under the pointer, with its class or elevation.
    pub(in crate::app) fn hover(
        &self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        lang: Lang,
    ) {
        let Some(pointer) = painter.ctx().pointer_hover_pos() else {
            return;
        };
        let Some(snapshot) = &geometry.input.poi else {
            return;
        };
        let scale = geometry.input.view.map_scale;
        let mut nearest = None;
        let mut minimum = (HOVER_RADIUS_POINTS * scale).powi(2);
        for label in &geometry.poi.labels {
            let distance = (pointer - label.position).length_sq();
            if distance < minimum {
                minimum = distance;
                nearest = Some((label.position, label.tile, label.index));
            }
        }
        let Some((at, tile, index)) = nearest else {
            return;
        };
        let Some(point) = snapshot
            .tiles
            .get(tile)
            .and_then(|tile| tile.points.get(index))
        else {
            return;
        };
        let text = tooltip(lang, point);
        super::hover_label(painter, geometry.input.view.rect, at, &text, scale);
    }
}

fn tooltip(lang: Lang, point: &PoiPoint) -> String {
    let name = if point.name.is_empty() {
        "—"
    } else {
        &point.name
    };
    let class = if point.class.is_empty() {
        "—"
    } else {
        &point.class
    };
    match point.kind {
        PointKind::Poi => lang.format(
            "poi-hover-place",
            &[("name", name.into()), ("class", class.into())],
        ),
        PointKind::Peak => lang.format(
            "poi-hover-peak",
            &[
                ("name", name.into()),
                (
                    "elevation",
                    point
                        .elevation_m
                        .map_or_else(|| "—".to_owned(), |metres| format!("{metres} m"))
                        .into(),
                ),
            ],
        ),
        PointKind::Water => lang.format(
            "poi-hover-water",
            &[("name", name.into()), ("class", class.into())],
        ),
        PointKind::Park => lang.format(
            "poi-hover-park",
            &[("name", name.into()), ("class", class.into())],
        ),
    }
}

fn lines(geometry: &LiveGeometry, style: &MapStyle, scale: f32) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for line in &geometry.poi.lines {
        let line_style = style.line(match line.kind {
            PoiLineKind::River => LineType::River,
            PoiLineKind::Canal => LineType::Canal,
            PoiLineKind::Park => LineType::Park,
        });
        for segment in &line.segments {
            styled_line(&mut shapes, segment, line_style, scale, line.opacity);
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
        let opacity = label.opacity;
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
                    PEAK.gamma_multiply(opacity),
                    Stroke::new(1.0 * scale, Color32::WHITE.gamma_multiply(opacity)),
                ));
                let text = match point.elevation_m {
                    Some(metres) => format!("{} {metres} m", point.name),
                    None => point.name.clone(),
                };
                (text, LABEL, font.clone(), Align2::LEFT_CENTER)
            }
            PointKind::Park => {
                markers.push(Shape::circle_filled(at, r, PARK.gamma_multiply(opacity)));
                (
                    point.name.clone(),
                    PARK_LABEL,
                    font.clone(),
                    Align2::LEFT_CENTER,
                )
            }
            PointKind::Poi => {
                markers.push(Shape::circle_filled(at, r, POI.gamma_multiply(opacity)));
                markers.push(Shape::circle_stroke(
                    at,
                    r,
                    Stroke::new(1.0 * scale, Color32::WHITE.gamma_multiply(opacity)),
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
        texts.push(super::transport::faded_text(
            position + Vec2::splat(1.0),
            shadow,
            opacity,
        ));
        texts.push(super::transport::faded_text(position, galley, opacity));
    }
    markers.extend(texts);
    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(kind: PointKind) -> PoiPoint {
        PoiPoint {
            kind,
            lat: 0.0,
            lon: 0.0,
            name: "Feature".into(),
            class: "museum".into(),
            rank: 1,
            elevation_m: None,
        }
    }

    #[test]
    fn hover_names_the_feature_class_and_peak_elevation() {
        let place = tooltip(Lang::En, &point(PointKind::Poi));
        assert!(place.contains("Feature"), "{place}");
        assert!(place.contains("Place of interest • museum"), "{place}");
        let peak = tooltip(
            Lang::En,
            &PoiPoint {
                kind: PointKind::Peak,
                elevation_m: Some(1_234),
                class: "peak".into(),
                ..point(PointKind::Poi)
            },
        );
        assert!(peak.contains("Peak • 1234 m"), "{peak}");
        let park = tooltip(Lang::PtBr, &point(PointKind::Park));
        assert!(park.contains("Parque ou área protegida • museum"), "{park}");
    }
}
