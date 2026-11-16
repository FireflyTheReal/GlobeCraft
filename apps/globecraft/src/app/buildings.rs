//! Buildings as blocks close up (roadmap 8.6); the geometry is projected by the
//! live-layer worker (`globecraft_render::live_layers::BuildingMesh`).
use eframe::egui;
use globecraft_data::buildings::Building;
use globecraft_render::live_layers::BuildingMesh;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct BuildingOverlay {
    /// Overture's building tiles.
    pub(super) tiles: globecraft_data::buildings::BuildingTiles,
    /// The blocks drawn.
    pub(super) current: Option<Arc<[Building]>>,
    /// The mesh last drawn, and what it was built from.
    drawn: Option<(Arc<BuildingMesh>, egui::Shape)>,
}

impl BuildingOverlay {
    pub(super) fn draw(&mut self, painter: &egui::Painter, mesh: &Arc<BuildingMesh>) {
        if mesh.indices.is_empty() {
            return;
        }
        if self
            .drawn
            .as_ref()
            .is_none_or(|(old, _)| !Arc::ptr_eq(old, mesh))
        {
            let shape = egui::Mesh {
                vertices: mesh
                    .vertices
                    .iter()
                    .map(|&(pos, [r, g, b, a])| egui::epaint::Vertex {
                        pos,
                        uv: egui::epaint::WHITE_UV,
                        color: egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                    })
                    .collect(),
                indices: mesh.indices.clone(),
                ..Default::default()
            };
            self.drawn = Some((mesh.clone(), egui::Shape::mesh(shape)));
        }
        if let Some((_, shape)) = &self.drawn {
            painter.add(shape.clone());
        }
    }
}
