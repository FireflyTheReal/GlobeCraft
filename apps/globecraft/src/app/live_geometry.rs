//! Match background live-layer geometry to the displayed globe before drawing.
use super::App;
use eframe::egui;
use globecraft_core::{
    camera::{Basis, Cam},
    math::now_unix,
};
use globecraft_render::live_layers::{LiveGeometry, LiveInput, LiveView, LiveWorker};
use std::sync::Arc;

const MIN_TRAFFIC_WIDTH_PX: usize = 160;
const MAX_TRAFFIC_DIMENSION_PX: usize = 800;

pub(super) struct LiveOverlay {
    worker: LiveWorker,
    requested: Option<LiveInput>,
    latest: Option<Arc<LiveGeometry>>,
}
impl LiveOverlay {
    pub fn new(ctx: egui::Context, cache: Option<Arc<globecraft_data::tiles::TileCache>>) -> Self {
        Self {
            worker: LiveWorker::with_elevation(cache, move || ctx.request_repaint())
                .expect("start live-layer projection"),
            requested: None,
            latest: None,
        }
    }
    pub fn accept(&mut self, geometry: Arc<LiveGeometry>) {
        self.latest = Some(geometry);
        self.requested = None;
    }
    /// Geometry for the displayed view and data, and a request for the current motion
    /// step when it is older: the last step stays on screen until the next is ready.
    pub fn update(&mut self, input: LiveInput) -> Option<Arc<LiveGeometry>> {
        if let Some(geometry) = self.worker.take_completed()
            && geometry.input.same_view(&input)
            && self.latest.as_ref().is_none_or(|old| {
                !old.input.same_view(&input) || old.input.unix_secs <= geometry.input.unix_secs
            })
        {
            self.latest = Some(geometry);
        }
        let current = self
            .latest
            .as_ref()
            .filter(|geometry| geometry.input.same_view(&input))
            .cloned();
        let stale = current
            .as_ref()
            .is_none_or(|geometry| !geometry.input.same_request(&input));
        if stale
            && self
                .requested
                .as_ref()
                .is_none_or(|old| !old.same_request(&input))
        {
            self.worker.submit(input.clone());
            self.requested = Some(input);
        }
        current
    }
}
impl App {
    pub(super) fn transport_layers(&self) -> globecraft_render::live_layers::TransportLayers {
        use globecraft_core::layers::MapLayer;
        globecraft_render::live_layers::TransportLayers {
            railways: self.layer(MapLayer::Railways),
            ferries: self.layer(MapLayer::Ferries),
            airports: self.layer(MapLayer::Airports),
        }
    }
    pub(super) fn poi_layers(&self) -> globecraft_render::live_layers::PoiLayers {
        use globecraft_core::layers::MapLayer;
        globecraft_render::live_layers::PoiLayers {
            places: self.layer(MapLayer::PointsOfInterest),
            peaks: self.layer(MapLayer::Peaks),
            waters: self.layer(MapLayer::Waters),
            parks: self.layer(MapLayer::Parks),
        }
    }
    /// Whether any layer drawn from the transport or places tiles is on.
    pub(super) fn map_data_shown(&self) -> bool {
        self.transport_shown() || self.poi_layers().any()
    }
    pub(super) fn transport_shown(&self) -> bool {
        let layers = self.transport_layers();
        layers.railways || layers.ferries || layers.airports
    }
    pub(super) fn live_layer_input(
        &self,
        basis: Basis,
        rect: egui::Rect,
        camera: Cam,
        pixels_per_point: f32,
    ) -> LiveInput {
        let width = ((rect.width() * pixels_per_point) as usize)
            .clamp(MIN_TRAFFIC_WIDTH_PX, MAX_TRAFFIC_DIMENSION_PX);
        let height = ((width as f32 * rect.height() / rect.width().max(1.0)) as usize)
            .clamp(1, MAX_TRAFFIC_DIMENSION_PX);
        LiveInput {
            view: LiveView {
                basis,
                rect,
                altitude_km: camera.alt,
                map_scale: self.settings.map_scale,
                pixels_per_point,
                traffic_width_px: width,
                traffic_height_px: height,
                ground: self.renderer.displayed_ground(self.overlay_ground()),
            },
            roads: self.roads.current.clone(),
            vessels: self
                .layer(globecraft_core::layers::MapLayer::Vessels)
                .then(|| self.live_vessels.shared_snapshot())
                .flatten(),
            satellites: self
                .layer(globecraft_core::layers::MapLayer::Satellites)
                .then(|| self.live_satellites.shared_snapshot())
                .flatten(),
            traffic: self
                .layer(globecraft_core::layers::MapLayer::Traffic)
                .then(|| self.traffic.service.shared_tiles())
                .flatten(),
            transport: self.transport_tiles.shared_snapshot(),
            transport_layers: self.transport_layers(),
            poi: self.poi_tiles.shared_snapshot(),
            poi_layers: self.poi_layers(),
            unix_secs: now_unix(),
        }
    }
}

pub(super) fn mesh_shapes(painter: &egui::Painter, shapes: Vec<egui::Shape>) -> Vec<egui::Shape> {
    let clipped = shapes
        .into_iter()
        .map(|shape| egui::epaint::ClippedShape {
            clip_rect: painter.clip_rect(),
            shape,
        })
        .collect();
    painter
        .ctx()
        .tessellate(clipped, painter.ctx().pixels_per_point())
        .into_iter()
        .map(|primitive| match primitive.primitive {
            egui::epaint::Primitive::Mesh(mesh) => egui::Shape::mesh(mesh),
            egui::epaint::Primitive::Callback(_) => {
                unreachable!("live overlays contain only styled geometry and labels")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_geometry_publication_for_an_old_view_is_never_drawn_on_a_new_view() {
        let ctx = egui::Context::default();
        let mut overlay = LiveOverlay::new(ctx, None);
        let cam = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 1000.0,
            tilt: 0.0,
        };
        let input = LiveInput {
            view: LiveView {
                basis: cam.basis(1.0),
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 300.0)),
                altitude_km: cam.alt,
                map_scale: 1.0,
                pixels_per_point: 1.0,
                traffic_width_px: 16,
                traffic_height_px: 16,
                ground: None,
            },
            roads: None,
            vessels: None,
            satellites: None,
            traffic: None,
            transport: None,
            transport_layers: Default::default(),
            poi: None,
            poi_layers: Default::default(),
            unix_secs: 1_700_000_000.0,
        };
        let geometry =
            globecraft_render::live_layers::LivePreparer::default().prepare(input.clone());
        overlay.accept(geometry.clone());
        assert!(Arc::ptr_eq(
            &geometry,
            &overlay.update(input.clone()).unwrap()
        ));
        // A later motion step keeps the last geometry on screen while it is prepared
        // (vessels and satellites blinked on every step before).
        let mut later = input.clone();
        later.satellites = Some(Vec::new().into());
        overlay
            .accept(globecraft_render::live_layers::LivePreparer::default().prepare(later.clone()));
        let shown = overlay.update(later.clone()).unwrap();
        later.unix_secs += 5.0;
        assert!(Arc::ptr_eq(&shown, &overlay.update(later.clone()).unwrap()));
        // Turning the layer off hides its last geometry at once; a new tile snapshot
        // (above) keeps it only because the layer stays present.
        let mut hidden = later;
        hidden.satellites = None;
        assert!(overlay.update(hidden).is_none());
        let mut moved = input;
        moved.view.rect = moved.view.rect.translate(egui::vec2(10.0, 0.0));
        assert!(overlay.update(moved).is_none());
    }

    #[test]
    fn a_new_tile_snapshot_keeps_the_last_geometry_until_it_is_projected() {
        let ctx = egui::Context::default();
        let mut overlay = LiveOverlay::new(ctx, None);
        let cam = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 1000.0,
            tilt: 0.0,
        };
        let mut input = LiveInput {
            view: LiveView {
                basis: cam.basis(1.0),
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 300.0)),
                altitude_km: cam.alt,
                map_scale: 1.0,
                pixels_per_point: 1.0,
                traffic_width_px: 16,
                traffic_height_px: 16,
                ground: None,
            },
            roads: None,
            vessels: None,
            satellites: None,
            traffic: None,
            transport: Some(Arc::new(globecraft_data::transport::TransportSnapshot {
                tiles: Vec::new(),
                zoom: 10,
            })),
            transport_layers: Default::default(),
            poi: None,
            poi_layers: Default::default(),
            unix_secs: 1_700_000_000.0,
        };
        let geometry =
            globecraft_render::live_layers::LivePreparer::default().prepare(input.clone());
        overlay.accept(geometry.clone());
        assert!(Arc::ptr_eq(
            &geometry,
            &overlay.update(input.clone()).unwrap()
        ));
        // A poll lands with a new snapshot pointer: the last geometry still matches
        // this view and stays on screen while the new snapshot is projected.
        input.transport = Some(Arc::new(globecraft_data::transport::TransportSnapshot {
            tiles: Vec::new(),
            zoom: 10,
        }));
        input.unix_secs += 1.0;
        assert!(Arc::ptr_eq(
            &geometry,
            &overlay.update(input.clone()).unwrap()
        ));
        // Turning on a layer group whose geometry is not projected yet does blank it
        // rather than showing a set that lacks the newly enabled layer.
        input.transport_layers.airports = true;
        assert!(overlay.update(input).is_none());
    }
}
