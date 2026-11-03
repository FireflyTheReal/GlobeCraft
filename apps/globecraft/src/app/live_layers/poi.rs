//! Places of interest, peaks, waters and parks: rivers and park outlines under the
//! borders, names over them, styled once per projected geometry.
use super::fade::{TiledShapes, styled_line};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Shape, Stroke, Vec2};
use globecraft_core::language::Lang;
use globecraft_data::poi::{PoiLineKind, PoiPoint, PointKind};
use globecraft_data::settings::map_style::{LineType, MapStyle};
use globecraft_data::vector_tiles::TileFade;
use globecraft_render::live_layers::LiveGeometry;
use globecraft_render::overlays::{LabelSpace, LabelSpaceKey};
use std::sync::Arc;
use std::time::Instant;

const HOVER_RADIUS_POINTS: f32 = 12.0;

const LABEL_FONT_POINTS: f32 = 11.0;
const WATER_FONT_POINTS: f32 = 12.0;
const MARKER_POINTS: f32 = 3.2;
const LABEL_OFFSET_POINTS: f32 = 7.0;
/// Between a place's badge and its name.
const BADGE_GAP_POINTS: f32 = 3.0;
/// Free room kept around each marker and name, map points.
const LABEL_PADDING_POINTS: f32 = 2.0;
const PARK: Color32 = Color32::from_rgb(92, 180, 96);
const PARK_LABEL: Color32 = Color32::from_rgb(190, 240, 190);
const PEAK: Color32 = Color32::from_rgb(150, 105, 70);
const LABEL: Color32 = Color32::from_rgb(240, 244, 250);
const WATER_LABEL: Color32 = Color32::from_rgb(150, 200, 255);

#[derive(Default)]
pub(in crate::app) struct PoiShapes {
    /// What the lines were built from: the projected geometry, scale and style.
    key: Option<(usize, u32, MapStyle)>,
    /// What the names were placed for: the projected geometry, scale and the room
    /// names of higher priority took.
    labels_key: Option<(usize, u32, LabelSpaceKey)>,
    lines: TiledShapes,
    labels: TiledShapes,
}

impl PoiShapes {
    pub(in crate::app) fn invalidate(&mut self) {
        self.key = None;
        self.labels_key = None;
    }
    /// Brings the shapes up to date; markers and names go only where `taken` is
    /// free (the names of every other map layer come first).
    pub(in crate::app) fn prepare(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        style: &MapStyle,
        taken: &LabelSpace,
    ) {
        let source = Arc::as_ptr(&geometry.poi) as usize;
        let scale_bits = geometry.input.view.map_scale.to_bits();
        let scale = geometry.input.view.map_scale;
        let labels_key = (source, scale_bits, taken.key());
        if self.labels_key.as_ref() != Some(&labels_key) {
            self.labels = TiledShapes::build(painter, labels(painter, geometry, scale, taken));
            self.labels_key = Some(labels_key);
        }
        let key = (source, scale_bits, *style);
        if self.key.as_ref() != Some(&key) {
            self.lines = TiledShapes::build(painter, lines(geometry, style, scale));
            self.key = Some(key);
        }
    }
    /// Rivers, canals and park outlines, drawn under the borders.
    pub(in crate::app) fn draw_lines(&self, painter: &egui::Painter) {
        self.lines.draw(painter, Instant::now());
    }
    /// Markers and names, drawn over the borders.
    pub(in crate::app) fn draw_labels(&self, painter: &egui::Painter) {
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

fn lines(geometry: &LiveGeometry, style: &MapStyle, scale: f32) -> Vec<(TileFade, Shape)> {
    let mut shapes = Vec::new();
    for line in &geometry.poi.lines {
        let fade = geometry
            .input
            .poi
            .as_ref()
            .map_or_else(TileFade::default, |snapshot| snapshot.fade(line.tile));
        let line_style = style.line(match line.kind {
            PoiLineKind::River => LineType::River,
            PoiLineKind::Canal => LineType::Canal,
            PoiLineKind::Park => LineType::Park,
        });
        let mut drawn = Vec::new();
        for segment in &line.segments {
            styled_line(&mut drawn, segment, line_style, scale, line.opacity);
        }
        shapes.extend(drawn.into_iter().map(|shape| (fade, shape)));
    }
    shapes
}

/// Markers and names. Labels come in priority order; each takes the room it
/// covers on screen, so a later one whose name would overlap an earlier marker or
/// name (or a name of another layer, in `taken`) tries the other side of its
/// marker, and else shows its marker alone (or nothing when even that is covered)
/// — as map apps thin out busy streets.
fn labels(
    painter: &egui::Painter,
    geometry: &LiveGeometry,
    scale: f32,
    taken: &LabelSpace,
) -> Vec<(TileFade, Shape)> {
    let Some(snapshot) = &geometry.input.poi else {
        return Vec::new();
    };
    let font = crate::app::fonts::map_font(painter.ctx(), LABEL_FONT_POINTS * scale);
    let water_font = FontId::new(WATER_FONT_POINTS * scale, font.family.clone());
    let padding = LABEL_PADDING_POINTS * scale;
    let mut placed: Vec<Rect> = Vec::new();
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
        let fade = snapshot.fade(label.tile);
        let at = label.position;
        let opacity = label.opacity;
        let r = MARKER_POINTS * scale;
        let mut marker = Vec::new();
        let (text, color, font, beside) = match point.kind {
            // Seas and oceans in the larger type; lakes, ponds and fountains (also
            // OpenMapTiles water names) as small as the other names.
            PointKind::Water => (
                point.name.clone(),
                WATER_LABEL,
                if matches!(point.class.as_str(), "ocean" | "sea" | "bay") {
                    water_font.clone()
                } else {
                    font.clone()
                },
                None,
            ),
            PointKind::Peak => {
                marker.push(Shape::convex_polygon(
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
                (text, LABEL, font.clone(), Some(LABEL_OFFSET_POINTS * scale))
            }
            PointKind::Park => {
                marker.push(Shape::circle_filled(at, r, PARK.gamma_multiply(opacity)));
                (
                    point.name.clone(),
                    PARK_LABEL,
                    font.clone(),
                    Some(LABEL_OFFSET_POINTS * scale),
                )
            }
            PointKind::Poi => {
                super::poi_icons::badge(&mut marker, at, &point.class, scale, opacity);
                (
                    point.name.clone(),
                    LABEL,
                    font.clone(),
                    Some((super::poi_icons::BADGE_RADIUS_POINTS + BADGE_GAP_POINTS) * scale),
                )
            }
        };
        let marker_rect = Shape::Vec(marker.clone())
            .visual_bounding_rect()
            .expand(padding);
        if marker_rect.is_positive()
            && (placed.iter().any(|other| other.intersects(marker_rect))
                || !taken.is_free(marker_rect))
        {
            continue;
        }
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), color);
        let size = galley.size();
        // Right of the marker, else left of it; centred names have one place.
        let candidates: Vec<egui::Pos2> = match beside {
            None => vec![Align2::CENTER_CENTER.anchor_size(at, size).min],
            Some(offset) => vec![
                Align2::LEFT_CENTER
                    .anchor_size(at + Vec2::new(offset, 0.0), size)
                    .min,
                Align2::RIGHT_CENTER
                    .anchor_size(at - Vec2::new(offset, 0.0), size)
                    .min,
            ],
        };
        let position = candidates.into_iter().find(|min| {
            let rect = Rect::from_min_size(*min, size).expand(padding);
            !placed.iter().any(|other| other.intersects(rect)) && taken.is_free(rect)
        });
        if marker_rect.is_positive() {
            placed.push(marker_rect);
            markers.extend(marker.into_iter().map(|shape| (fade, shape)));
        }
        let Some(position) = position else {
            continue;
        };
        placed.push(Rect::from_min_size(position, size).expand(padding));
        let shadow = painter.layout_no_wrap(text, font, Color32::BLACK);
        texts.push((
            fade,
            super::transport::faded_text(position + Vec2::splat(1.0), shadow, opacity),
        ));
        texts.push((
            fade,
            super::transport::faded_text(position, galley, opacity),
        ));
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
