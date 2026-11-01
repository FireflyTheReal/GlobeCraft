//! Keyless worldwide OSM road geometry; roadworks are not congestion reports.
use eframe::egui::{self, Color32, Pos2, Rect};
use globecraft_data::settings::map_style::{LineType, MapStyle};
use std::sync::Arc;
use std::time::Instant;

#[derive(Default)]
pub(super) struct RoadOverlay {
    /// GlobeCraft's road tiles.
    pub tiles: globecraft_data::roads::RoadTiles,
    /// The roads drawn.
    pub current: Option<Arc<[globecraft_data::roads::Road]>>,
    /// What the shapes were built from: the projection and style.
    projected: Option<(
        Arc<Vec<globecraft_render::live_layers::ProjectedRoad>>,
        MapStyle,
    )>,
    /// The view and road list of the last build, to tell new tiles from a move.
    source: Option<(globecraft_render::live_layers::LiveView, usize)>,
    shapes: super::live_layers::FadingShapes,
}
impl RoadOverlay {
    pub(super) fn invalidate_text(&mut self) {
        self.projected = None;
    }
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        geometry: &globecraft_render::live_layers::LiveGeometry,
        lang: globecraft_core::language::Lang,
        scale: f32,
        style: &MapStyle,
    ) {
        let rect = geometry.input.view.rect;
        let Some(roads) = &geometry.input.roads else {
            return;
        };
        let hover = painter
            .ctx()
            .pointer_hover_pos()
            .filter(|p| rect.contains(*p));
        if self
            .projected
            .as_ref()
            .is_none_or(|(old, old_style)| !Arc::ptr_eq(old, &geometry.roads) || old_style != style)
        {
            let view = geometry.input.view;
            let data = Arc::as_ptr(roads) as *const () as usize;
            let new_data = self
                .source
                .is_some_and(|(old_view, old_data)| old_view.same_frame(&view) && old_data != data);
            self.source = Some((view, data));
            let opacity = globecraft_render::live_layers::altitude_fade(
                globecraft_data::roads::MAX_ALTITUDE_KM,
                view.altitude_km,
            );
            let mut shapes = Vec::new();
            for projected in geometry.roads.iter() {
                let road = &roads[projected.source_index];
                let line_style = style.line(line_type(road));
                for line in &projected.segments {
                    super::live_layers::styled_line(&mut shapes, line, line_style, scale, opacity);
                }
            }
            self.shapes.replace(
                super::live_geometry::mesh_shapes(painter, shapes),
                new_data,
                Instant::now(),
            );
            self.projected = Some((geometry.roads.clone(), *style));
        }
        self.shapes.draw(painter, Instant::now());
        let nearest = hover
            .and_then(|mouse| geometry.nearest_road(mouse, 12.0 * scale))
            .map(|(index, point)| (&roads[index], point));
        if let Some((road, screen)) = nearest {
            let state = lang.text(if road.restricted {
                "roads-restricted-help"
            } else {
                "roads-flow-unknown-help"
            });
            let text = lang.format(
                "roads-hover-tooltip",
                &[
                    (
                        "name",
                        if road.name.is_empty() {
                            lang.text("roads-unnamed").into()
                        } else {
                            road.name.as_str().into()
                        },
                    ),
                    ("kind", road.kind.as_str().into()),
                    ("state", state.into()),
                ],
            );
            let galley = painter.layout_no_wrap(
                text,
                crate::app::fonts::map_font(painter.ctx(), 12.0 * scale),
                Color32::WHITE,
            );
            let pos = Pos2::new(
                (screen.x + 16.0)
                    .min(rect.right() - galley.size().x - 10.0)
                    .max(rect.left() + 5.0),
                (screen.y + 12.0)
                    .min(rect.bottom() - galley.size().y - 10.0)
                    .max(rect.top() + 5.0),
            );
            painter.rect_filled(
                Rect::from_min_size(pos, galley.size()).expand(5.0),
                5.0,
                Color32::from_rgba_unmultiplied(9, 19, 30, 230),
            );
            painter.galley(pos, galley, Color32::WHITE);
        }
    }
}
/// The user-styled line type a road draws with. Neutral gray means unknown, not
/// free-flowing; the roadworks colour means a documented restriction or works.
fn line_type(road: &globecraft_data::roads::Road) -> LineType {
    if road.restricted {
        LineType::Roadworks
    } else if matches!(
        road.kind.as_str(),
        "motorway" | "trunk" | "primary" | "secondary"
    ) {
        LineType::MajorRoad
    } else {
        LineType::MinorRoad
    }
}
