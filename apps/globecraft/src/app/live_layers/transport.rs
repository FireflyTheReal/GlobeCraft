//! Railways, ferries and airports: styled once per projected geometry and kept as
//! tessellated meshes, so a still view costs nothing per frame.
use eframe::egui::{self, Align2, Color32, Shape, Stroke, Vec2};
use globecraft_core::language::Lang;
use globecraft_data::transport::{Airport, LineKind};
use globecraft_render::live_layers::{LiveGeometry, ProjectedTransport};
use std::sync::Arc;

const LABEL_FONT_POINTS: f32 = 11.0;
const LABEL_OFFSET_POINTS: f32 = 8.0;
const AIRPORT_MARKER_POINTS: f32 = 4.0;
const HOVER_RADIUS_POINTS: f32 = 12.0;
const AIRPORT_COLOR: Color32 = Color32::from_rgb(52, 120, 220);
const LABEL_COLOR: Color32 = Color32::from_rgb(235, 242, 255);
/// Opacity of lines in tunnels.
const TUNNEL_OPACITY: f32 = 0.45;
/// Below this the main railways get their white dashes (the classic map style).
const RAIL_DASH_MAX_ALTITUDE_KM: f64 = 250.0;

/// Base stroke width (points), colour and, for dashed lines, dash and gap lengths.
struct Style {
    width: f32,
    color: Color32,
    dash: Option<(f32, f32)>,
}

fn style(kind: LineKind) -> Style {
    let style = |width, (r, g, b), dash| Style {
        width,
        color: Color32::from_rgb(r, g, b),
        dash,
    };
    match kind {
        LineKind::Rail => style(2.4, (58, 58, 66), None),
        LineKind::RailService => style(1.0, (96, 96, 106), None),
        LineKind::Subway => style(1.8, (150, 92, 210), None),
        LineKind::Tram => style(1.3, (214, 112, 56), None),
        LineKind::Ferry => style(1.5, (86, 178, 255), Some((6.0, 4.0))),
        LineKind::Runway => style(3.5, (205, 205, 212), None),
        LineKind::Taxiway => style(1.0, (226, 196, 84), None),
    }
}

#[derive(Default)]
pub(in crate::app) struct TransportShapes {
    key: Option<(usize, usize)>,
    lines: Vec<Shape>,
    airports: Vec<Shape>,
}

impl TransportShapes {
    pub(in crate::app) fn invalidate(&mut self) {
        self.key = None;
    }
    fn refresh(&mut self, painter: &egui::Painter, geometry: &LiveGeometry) {
        let key = (
            Arc::as_ptr(&geometry.transport) as usize,
            geometry.input.view.map_scale.to_bits() as usize,
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let scale = geometry.input.view.map_scale;
        self.lines = super::super::live_geometry::mesh_shapes(
            painter,
            lines(&geometry.transport, scale, geometry.input.view.altitude_km),
        );
        self.airports =
            super::super::live_geometry::mesh_shapes(painter, airports(painter, geometry, scale));
    }
    /// The lines, drawn under the borders.
    pub(in crate::app) fn lines(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
    ) -> &[Shape] {
        self.refresh(painter, geometry);
        &self.lines
    }
    /// Airport markers and names, drawn over the borders.
    pub(in crate::app) fn airports(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
    ) -> &[Shape] {
        self.refresh(painter, geometry);
        &self.airports
    }
    /// The nearest airport under the pointer, with its codes and class.
    pub(in crate::app) fn hover(
        &self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        lang: Lang,
    ) {
        let Some(pointer) = painter.ctx().pointer_hover_pos() else {
            return;
        };
        let Some(snapshot) = &geometry.input.transport else {
            return;
        };
        let scale = geometry.input.view.map_scale;
        let mut nearest = None;
        let mut minimum = (HOVER_RADIUS_POINTS * scale).powi(2);
        for projected in &geometry.transport.airports {
            let distance = (pointer - projected.position).length_sq();
            if distance < minimum {
                minimum = distance;
                nearest = Some((projected.position, projected.tile, projected.index));
            }
        }
        let Some((at, tile, index)) = nearest else {
            return;
        };
        let Some(airport) = snapshot
            .tiles
            .get(tile)
            .and_then(|tile| tile.airports.get(index))
        else {
            return;
        };
        let text = tooltip(lang, airport);
        super::hover_label(painter, geometry.input.view.rect, at, &text, scale);
    }
}

fn tooltip(lang: Lang, airport: &Airport) -> String {
    let class = if airport.class.is_empty() {
        "other"
    } else {
        airport.class.as_str()
    };
    lang.format(
        "transport-airport-hover-tooltip",
        &[
            ("name", airport.name.as_str().into()),
            ("iata", airport.iata.as_deref().unwrap_or("—").into()),
            ("icao", airport.icao.as_deref().unwrap_or("—").into()),
            ("class", class.into()),
        ],
    )
}

fn lines(transport: &ProjectedTransport, scale: f32, altitude_km: f64) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for line in &transport.lines {
        let style = style(line.kind);
        let opacity = if line.tunnel { TUNNEL_OPACITY } else { 1.0 };
        let stroke = Stroke::new(style.width * scale, style.color.gamma_multiply(opacity));
        for segment in &line.segments {
            match style.dash {
                Some((dash, gap)) => shapes.extend(Shape::dashed_line(
                    segment,
                    stroke,
                    dash * scale,
                    gap * scale,
                )),
                None => shapes.push(Shape::line(segment.clone(), stroke)),
            }
            if line.kind == LineKind::Rail && altitude_km <= RAIL_DASH_MAX_ALTITUDE_KM {
                let white = Stroke::new(
                    style.width * 0.45 * scale,
                    Color32::from_rgb(238, 238, 240).gamma_multiply(opacity),
                );
                shapes.extend(Shape::dashed_line(segment, white, 5.0 * scale, 5.0 * scale));
            }
        }
    }
    shapes
}

fn airports(painter: &egui::Painter, geometry: &LiveGeometry, scale: f32) -> Vec<Shape> {
    let Some(snapshot) = &geometry.input.transport else {
        return Vec::new();
    };
    let font = crate::app::fonts::map_font(painter.ctx(), LABEL_FONT_POINTS * scale);
    let mut markers = Vec::new();
    let mut labels = Vec::new();
    for projected in &geometry.transport.airports {
        let Some(airport) = snapshot
            .tiles
            .get(projected.tile)
            .and_then(|tile| tile.airports.get(projected.index))
        else {
            continue;
        };
        let at = projected.position;
        let radius = AIRPORT_MARKER_POINTS * scale;
        markers.push(Shape::circle_filled(at, radius, AIRPORT_COLOR));
        markers.push(Shape::circle_stroke(
            at,
            radius,
            Stroke::new(1.2 * scale, Color32::WHITE),
        ));
        if !projected.labelled {
            continue;
        }
        let text = match &airport.iata {
            Some(code) if !airport.name.contains(code.as_str()) => {
                format!("{} ({code})", airport.name)
            }
            _ => airport.name.clone(),
        };
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), LABEL_COLOR);
        let position = Align2::LEFT_CENTER
            .anchor_size(
                at + Vec2::new(LABEL_OFFSET_POINTS * scale, 0.0),
                galley.size(),
            )
            .min;
        let shadow = painter.layout_no_wrap(text, font.clone(), Color32::BLACK);
        labels.push(Shape::galley(
            position + Vec2::splat(1.0),
            shadow,
            Color32::BLACK,
        ));
        labels.push(Shape::galley(position, galley, LABEL_COLOR));
    }
    markers.extend(labels);
    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airport_tooltip_names_codes_and_class() {
        let airport = Airport {
            lat: 0.0,
            lon: 0.0,
            name: "Example".into(),
            iata: Some("EXA".into()),
            icao: Some("EXAM".into()),
            class: "international".into(),
        };
        let text = tooltip(Lang::En, &airport);
        assert!(text.contains("Example"), "{text}");
        assert!(text.contains("IATA EXA • ICAO EXAM"), "{text}");
        assert!(text.contains("International airport"), "{text}");
        let without_codes = Airport {
            iata: None,
            icao: None,
            class: String::new(),
            ..airport
        };
        let text = tooltip(Lang::PtBr, &without_codes);
        assert!(text.contains("IATA — • ICAO —"), "{text}");
        assert!(text.contains("Aeródromo"), "{text}");
    }
}
