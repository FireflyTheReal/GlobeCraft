//! One displayed view; tile generation and unrelated panels do not invalidate it.
use super::galleys::LabelGalleys;
use super::meshes::PreparedOverlay;
use super::projected;
use eframe::egui::{self, Rect, Shape};
use globecraft_core::{camera::Basis, language::Lang};
use globecraft_data::boundaries::BoundaryData;
use globecraft_data::settings::map_style::MapStyle;
use globecraft_render::overlays::OverlayWorker;
use std::sync::Arc;

pub(super) use globecraft_render::overlays::OverlayLayers;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct OverlayStyle {
    pub layers: OverlayLayers,
    pub map_scale: f32,
    pub lang: Lang,
    pub arrival: ArrivalFade,
    pub map_style: MapStyle,
}

/// How far each streamed layer has faded in since its data arrived, in
/// `ARRIVAL_FADE_STEPS` steps. Data that lands while the camera is already inside
/// its altitude band fades in over a short time instead of popping. Part of the
/// style, so the cached shapes rebuild once per step during a fade and never after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ArrivalFade {
    pub states: u8,
    pub states_big: u8,
    pub municipal: u8,
}
pub(super) const ARRIVAL_FADE_STEPS: u8 = 16;
impl ArrivalFade {
    pub(super) const SETTLED: Self = Self {
        states: ARRIVAL_FADE_STEPS,
        states_big: ARRIVAL_FADE_STEPS,
        municipal: ARRIVAL_FADE_STEPS,
    };
    pub(super) fn is_settled(self) -> bool {
        self == Self::SETTLED
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct OverlayView {
    pub basis: Basis,
    pub rect: Rect,
    pub altitude_km: f64,
    pub state_max_km: f64,
    pub grid_step_deg: Option<f64>,
    pub pixels_per_point: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct DrawKey {
    view: OverlayView,
    style: OverlayStyle,
}

pub(in crate::app) struct OverlayCache {
    key: Option<DrawKey>,
    shapes: Vec<Shape>,
    /// Key the current `shapes` were drawn for; kept when a boundary patch
    /// replaces the dataset so the previous dataset can keep painting until
    /// the replacement geometry is ready.
    stale: Option<DrawKey>,
    geometry: Option<PreparedOverlay>,
    requested: Option<(globecraft_render::overlays::OverlayView, usize)>,
    worker: OverlayWorker<PreparedOverlay>,
    galleys: LabelGalleys,
    /// The current shapes lack names that had no layout budget left.
    labels_pending: bool,
}

impl OverlayCache {
    pub(in crate::app) fn accept(&mut self, prepared: PreparedOverlay) {
        let geometry = &prepared.geometry;
        if self
            .geometry
            .as_ref()
            .is_none_or(|old| !old.geometry.matches(geometry.view, &geometry.data))
        {
            self.key = None;
            self.geometry = Some(prepared);
        }
    }

    #[cfg(test)]
    pub(super) fn is_ready(&self, view: OverlayView, style: OverlayStyle) -> bool {
        self.key == Some(DrawKey { view, style })
    }
    pub(in crate::app) fn new(ctx: egui::Context) -> Self {
        Self {
            key: None,
            stale: None,
            shapes: Vec::new(),
            geometry: None,
            requested: None,
            galleys: LabelGalleys::default(),
            labels_pending: false,
            worker: OverlayWorker::with_finish(
                {
                    let ctx = ctx.clone();
                    move || ctx.request_repaint()
                },
                move |geometry| PreparedOverlay::new(geometry, &ctx),
            )
            .expect("start overlay preparation"),
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.key = None;
        self.stale = None;
        self.shapes = Vec::new();
        self.geometry = None;
        self.requested = None;
        self.galleys.clear();
    }

    /// The boundary dataset was replaced but the displayed camera, layers and
    /// fonts did not change. Drop nothing the eye can see: keep painting the
    /// previous dataset until the worker publishes geometry for the new one,
    /// so a detail swap never flashes the borders away first.
    pub(super) fn merge_boundaries(&mut self) {
        self.key = None;
        self.requested = None;
    }

    pub(super) fn draw(
        &mut self,
        painter: &egui::Painter,
        view: OverlayView,
        boundaries: &Arc<BoundaryData>,
        style: OverlayStyle,
    ) {
        let current = DrawKey { view, style };
        if std::mem::take(&mut self.labels_pending) && self.key == Some(current) {
            // Same view and geometry: rebuild in place to add the remaining names.
            self.key = None;
        }
        if self.key != Some(current) {
            let geometry_view = globecraft_render::overlays::OverlayView {
                basis: view.basis,
                rect: view.rect,
                altitude_km: view.altitude_km,
                state_visible_max_km: view.state_max_km,
                grid_step_deg: view.grid_step_deg,
                layers: style.layers,
                dashes: globecraft_render::overlays::BorderDashes::of(&style.map_style),
            };
            if let Some(completed) = self.worker.take_completed()
                && completed.geometry.view == geometry_view
            {
                self.geometry = Some(completed);
            }
            let this_view = self
                .geometry
                .as_ref()
                .filter(|prepared| prepared.geometry.view == geometry_view);
            let current_data = this_view
                .is_some_and(|prepared| prepared.geometry.matches(geometry_view, boundaries));
            if let Some(prepared) = this_view {
                // Geometry for exactly this view. When it was projected from the
                // previous dataset (a patch arrived while the camera moved), paint it
                // anyway rather than a blank frame: it is a complete, consistent set of
                // lines, one patch old, and the replacement is requested below.
                #[cfg(feature = "profiling")]
                let _profile = globecraft_data::profiling::Span::new("ui.overlay_shapes");
                let built = projected::build_prepared(painter, prepared, &mut self.galleys, style);
                self.shapes = built.shapes;
                if built.labels_pending {
                    // Rebuild next frame to lay out the names left over (cheap: the
                    // rest of the view is cached).
                    self.labels_pending = true;
                    painter.ctx().request_repaint();
                }
                self.stale = Some(current);
                if current_data {
                    self.key = Some(current);
                    painter.extend(self.shapes.iter().cloned());
                    return;
                }
                self.key = None;
                let request = (geometry_view, Arc::as_ptr(boundaries) as usize);
                if self.requested != Some(request) {
                    self.worker.submit(geometry_view, boundaries.clone());
                    self.requested = Some(request);
                }
                painter.ctx().request_repaint();
                painter.extend(self.shapes.iter().cloned());
                return;
            } else {
                let request = (geometry_view, Arc::as_ptr(boundaries) as usize);
                if self.requested != Some(request) {
                    self.worker.submit(geometry_view, boundaries.clone());
                    self.requested = Some(request);
                }
                // Hold the previous dataset's shapes while the worker prepares
                // the replacement, but only when they were projected for
                // exactly this camera and layer set; a moved camera must never
                // paint geometry from another view.
                if self.stale == Some(current) {
                    painter.extend(self.shapes.iter().cloned());
                }
                return;
            }
        }
        // Never paint shapes projected for a different displayed camera or layer set.
        if self.key == Some(current) {
            painter.extend(self.shapes.iter().cloned());
        }
    }
}

#[cfg(test)]
mod tests;
