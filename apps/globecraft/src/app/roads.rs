//! Keyless worldwide OSM road geometry; roadworks are not congestion reports.
use super::live_layers::{TiledShapes, styled_line};
use super::road_names::{NameMemory, NameView};
use eframe::egui;
use globecraft_data::roads::Road;
use globecraft_data::settings::map_style::{LabelType, LineType, MapStyle};
use globecraft_render::live_layers::ProjectedRoad;
use globecraft_render::overlays::{LabelSpace, LabelSpaceKey, PlacedLabels};
use std::sync::Arc;
use std::time::Instant;

#[derive(Default)]
pub(super) struct RoadOverlay {
    /// GlobeCraft's road tiles.
    pub tiles: globecraft_data::roads::RoadTiles,
    /// The roads drawn.
    pub current: Option<Arc<[Road]>>,
    /// What the shapes were built from: the projection and style.
    projected: Option<(Arc<Vec<ProjectedRoad>>, MapStyle)>,
    /// What the names were placed for: the projection, style and the room the
    /// names of higher priority took.
    named: Option<(Arc<Vec<ProjectedRoad>>, MapStyle, LabelSpaceKey)>,
    /// The lines, grouped by the fade of the tile they came from.
    shapes: TiledShapes,
    /// Street and road names along the lines.
    names: TiledShapes,
    /// The room the names take.
    placed: PlacedLabels,
    memory: NameMemory,
}
impl RoadOverlay {
    pub(super) fn invalidate_text(&mut self) {
        self.projected = None;
        self.named = None;
    }
    /// The room the names drawn last take.
    pub(super) fn placed(&self) -> &PlacedLabels {
        &self.placed
    }
    /// Draws the roads and their names; names go only where `taken` is free.
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        geometry: &globecraft_render::live_layers::LiveGeometry,
        scale: f32,
        style: &MapStyle,
        taken: &LabelSpace,
    ) {
        let Some(roads) = &geometry.input.roads else {
            self.placed.clear();
            return;
        };
        if self
            .projected
            .as_ref()
            .is_none_or(|(old, old_style)| !Arc::ptr_eq(old, &geometry.roads) || old_style != style)
        {
            let view = geometry.input.view;
            let opacity = globecraft_render::live_layers::altitude_fade(
                globecraft_data::roads::MAX_ALTITUDE_KM,
                view.altitude_km,
            );
            let mut lines = Vec::new();
            for projected in geometry.roads.iter() {
                let road = &roads[projected.source_index];
                let line_style = style.line(line_type(road));
                let mut shapes = Vec::new();
                for line in &projected.segments {
                    styled_line(&mut shapes, line, line_style, scale, opacity);
                }
                lines.extend(shapes.into_iter().map(|shape| (road.fade, shape)));
            }
            self.shapes = TiledShapes::build(painter, lines);
            self.projected = Some((geometry.roads.clone(), *style));
        }
        let space = taken.key();
        if self
            .named
            .as_ref()
            .is_none_or(|(old, old_style, old_space)| {
                !Arc::ptr_eq(old, &geometry.roads) || old_style != style || *old_space != space
            })
        {
            let view = geometry.input.view;
            let (names, rects) = super::road_names::names(
                painter,
                roads,
                &geometry.roads,
                &NameView {
                    basis: &view.basis,
                    viewport: view.rect,
                    altitude_km: view.altitude_km,
                    size_points: style.labels.get(LabelType::Road),
                    scale,
                },
                taken,
                &mut self.memory,
            );
            self.names = TiledShapes::build(painter, names);
            self.placed.replace(rects);
            self.named = Some((geometry.roads.clone(), *style, space));
        }
        let now = Instant::now();
        self.shapes.draw(painter, now);
        self.names.draw(painter, now);
    }
}
/// The user-styled line type a road draws with. Neutral gray means unknown, not
/// free-flowing; the roadworks colour means a documented restriction or works.
fn line_type(road: &Road) -> LineType {
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
