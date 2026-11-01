//! Railways, ferries and airports: styled once per projected geometry and kept as
//! tessellated meshes, so a still view costs nothing per frame.
use super::fade::{FadingShapes, styled_line};
use eframe::egui::{self, Align2, Color32, Shape, Stroke, Vec2};
use globecraft_core::language::Lang;
use globecraft_data::settings::map_style::{DashStyle, LineType, MapStyle};
use globecraft_data::transport::{Airport, LineKind};
use globecraft_render::live_layers::{LiveGeometry, LiveView, ProjectedTransport};
use std::sync::Arc;
use std::time::Instant;

const LABEL_FONT_POINTS: f32 = 11.0;
const LABEL_OFFSET_POINTS: f32 = 8.0;
const AIRPORT_MARKER_POINTS: f32 = 4.0;
const HOVER_RADIUS_POINTS: f32 = 12.0;
const AIRPORT_COLOR: Color32 = Color32::from_rgb(52, 120, 220);
const LABEL_COLOR: Color32 = Color32::from_rgb(235, 242, 255);
/// Opacity of lines in tunnels.
const TUNNEL_OPACITY: f32 = 0.45;
/// Below this the main railways get their white dashes (the classic map style), when
/// their style is a solid line.
const RAIL_DASH_MAX_ALTITUDE_KM: f64 = 250.0;
const RAIL_DASH_COLOR: Color32 = Color32::from_rgb(238, 238, 240);

/// The user-styled line type a kind of line draws with.
fn line_type(kind: LineKind) -> LineType {
    match kind {
        LineKind::Rail => LineType::Railway,
        LineKind::RailService => LineType::ServiceRail,
        LineKind::Subway => LineType::Subway,
        LineKind::Tram => LineType::Tram,
        LineKind::Ferry => LineType::Ferry,
        LineKind::Runway => LineType::Runway,
        LineKind::Taxiway => LineType::Taxiway,
    }
}

#[derive(Default)]
pub(in crate::app) struct TransportShapes {
    /// What the shapes were built from: the projected geometry, scale and style.
    key: Option<(usize, u32, MapStyle)>,
    /// The view and data snapshot of the last build, to tell new tiles from a move.
    source: Option<(LiveView, usize)>,
    lines: FadingShapes,
    airports: FadingShapes,
}

impl TransportShapes {
    pub(in crate::app) fn invalidate(&mut self) {
        self.key = None;
    }
    fn refresh(&mut self, painter: &egui::Painter, geometry: &LiveGeometry, style: &MapStyle) {
        let key = (
            Arc::as_ptr(&geometry.transport) as usize,
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
            .transport
            .as_ref()
            .map_or(0, |snapshot| Arc::as_ptr(snapshot) as usize);
        let new_data = self
            .source
            .is_some_and(|(old_view, old_data)| old_view.same_frame(&view) && old_data != data);
        self.source = Some((view, data));
        let now = Instant::now();
        let scale = view.map_scale;
        self.lines.replace(
            super::super::live_geometry::mesh_shapes(
                painter,
                lines(&geometry.transport, style, scale, view.altitude_km),
            ),
            new_data,
            now,
        );
        self.airports.replace(
            super::super::live_geometry::mesh_shapes(painter, airports(painter, geometry, scale)),
            new_data,
            now,
        );
    }
    /// The lines, drawn under the borders.
    pub(in crate::app) fn draw_lines(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        style: &MapStyle,
    ) {
        self.refresh(painter, geometry, style);
        self.lines.draw(painter, Instant::now());
    }
    /// Airport markers and names, drawn over the borders.
    pub(in crate::app) fn draw_airports(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        style: &MapStyle,
    ) {
        self.refresh(painter, geometry, style);
        self.airports.draw(painter, Instant::now());
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

fn lines(
    transport: &ProjectedTransport,
    style: &MapStyle,
    scale: f32,
    altitude_km: f64,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for line in &transport.lines {
        let line_style = style.line(line_type(line.kind));
        let opacity = line.opacity * if line.tunnel { TUNNEL_OPACITY } else { 1.0 };
        for segment in &line.segments {
            styled_line(&mut shapes, segment, line_style, scale, opacity);
            if line.kind == LineKind::Rail
                && line_style.dash == DashStyle::Solid
                && altitude_km <= RAIL_DASH_MAX_ALTITUDE_KM
            {
                let white = Stroke::new(
                    line_style.width_points * 0.45 * scale,
                    RAIL_DASH_COLOR.gamma_multiply(opacity),
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
        let opacity = projected.opacity;
        let radius = AIRPORT_MARKER_POINTS * scale;
        markers.push(Shape::circle_filled(
            at,
            radius,
            AIRPORT_COLOR.gamma_multiply(opacity),
        ));
        markers.push(Shape::circle_stroke(
            at,
            radius,
            Stroke::new(1.2 * scale, Color32::WHITE.gamma_multiply(opacity)),
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
        labels.push(faded_text(position + Vec2::splat(1.0), shadow, opacity));
        labels.push(faded_text(position, galley, opacity));
    }
    markers.extend(labels);
    markers
}

/// A name at `opacity` (the galley's colours are fixed when laid out).
pub(super) fn faded_text(position: egui::Pos2, galley: Arc<egui::Galley>, opacity: f32) -> Shape {
    Shape::Text(
        egui::epaint::TextShape::new(position, galley, Color32::BLACK).with_opacity_factor(opacity),
    )
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
