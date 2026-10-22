//! Border meshes tessellated next to the overlay geometry, off the UI thread.
//!
//! A descent through the state and municipal bands projects 30–45 k border segments per
//! view. Tessellating them on the UI thread cost 10–30 ms per rebuilt view, so the
//! overlay workers tessellate each border kind once, in white, right after projecting
//! it. The UI thread only tints the finished vertices with the current fade colours,
//! which keeps arrival and altitude fades on the UI while the expensive part moves out.
use super::projected::{BORDER_KINDS, border_width};
use eframe::egui::{
    self, Color32, Mesh, Stroke,
    epaint::{TessellationOptions, Tessellator},
};
use globecraft_data::settings::map_style::MapStyle;
use globecraft_render::overlays::OverlayGeometry;
use std::sync::Arc;

/// What tessellation depends on besides the geometry. Read from the egui context on
/// the worker thread; the UI re-tessellates when its own settings differ (a display
/// change between preparation and drawing).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct MeshSettings {
    pixels_per_point: f32,
    options: TessellationOptions,
    widths: BorderWidths,
}

impl MeshSettings {
    pub(super) fn of(ctx: &egui::Context) -> Self {
        Self {
            pixels_per_point: ctx.pixels_per_point(),
            options: ctx.tessellation_options(|options| *options),
            widths: BorderWidths::current(ctx),
        }
    }
}

/// Line width in points of each border kind, from the user's map style. The app
/// publishes them to the egui context, where the overlay and frame workers read them
/// along with the other tessellation settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct BorderWidths([f32; BORDER_KINDS.len()]);

impl BorderWidths {
    fn id() -> egui::Id {
        egui::Id::new("globecraft-border-widths")
    }
    pub(super) fn of(style: &MapStyle) -> Self {
        Self(BORDER_KINDS.map(|kind| border_width(kind, style)))
    }
    pub(super) fn publish(self, ctx: &egui::Context) {
        if Self::current(ctx) != self {
            ctx.data_mut(|data| data.insert_temp(Self::id(), self));
        }
    }
    fn current(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp(Self::id()))
            .unwrap_or_else(|| Self::of(&MapStyle::default()))
    }
}

/// One white mesh per `BorderKind`, indexed by `kind as usize`, drawn in that order
/// (the order `project_borders` emits the kinds in).
pub(super) struct BorderMeshes {
    settings: MeshSettings,
    kinds: [Mesh; BORDER_KINDS.len()],
}

impl BorderMeshes {
    pub(super) fn tessellate(geometry: &OverlayGeometry, settings: MeshSettings) -> Self {
        #[cfg(feature = "profiling")]
        let _profile = globecraft_data::profiling::Span::new("overlay.tessellate");
        // Line segments never sample the font atlas, so its size does not matter here.
        let mut tessellator = Tessellator::new(
            settings.pixels_per_point,
            settings.options,
            [1, 1],
            Vec::new(),
        );
        tessellator.set_clip_rect(geometry.view.rect);
        let mut kinds: [Mesh; BORDER_KINDS.len()] = Default::default();
        for &(kind, points) in &geometry.borders {
            tessellator.tessellate_line_segment(
                points,
                Stroke::new(settings.widths.0[kind as usize], Color32::WHITE),
                &mut kinds[kind as usize],
            );
        }
        Self { settings, kinds }
    }

    pub(super) fn settings(&self) -> MeshSettings {
        self.settings
    }

    /// All kinds in one mesh, each tinted with its colour. Fully transparent kinds
    /// are skipped. White times a colour is that colour exactly, so the result
    /// matches tessellating with the coloured strokes directly.
    pub(super) fn tinted(&self, colors: [Color32; BORDER_KINDS.len()]) -> Mesh {
        let mut out = Mesh::default();
        let visible = || {
            self.kinds
                .iter()
                .zip(colors)
                .filter(|(mesh, color)| color.a() > 0 && !mesh.is_empty())
        };
        let (vertices, indices) = visible().fold((0, 0), |(v, i), (mesh, _)| {
            (v + mesh.vertices.len(), i + mesh.indices.len())
        });
        out.vertices.reserve_exact(vertices);
        out.indices.reserve_exact(indices);
        for (mesh, color) in visible() {
            let base = out.vertices.len() as u32;
            out.indices
                .extend(mesh.indices.iter().map(|index| index + base));
            out.vertices
                .extend(mesh.vertices.iter().map(|vertex| egui::epaint::Vertex {
                    color: vertex.color * color,
                    ..*vertex
                }));
        }
        out
    }
}

/// Overlay geometry with its border meshes, as the workers publish it.
#[derive(Clone)]
pub(in crate::app) struct PreparedOverlay {
    pub(super) geometry: Arc<OverlayGeometry>,
    pub(super) borders: Arc<BorderMeshes>,
}

impl PreparedOverlay {
    /// Tessellates on the calling (worker) thread.
    pub(in crate::app) fn new(geometry: Arc<OverlayGeometry>, ctx: &egui::Context) -> Self {
        let borders = Arc::new(BorderMeshes::tessellate(&geometry, MeshSettings::of(ctx)));
        Self { geometry, borders }
    }

    /// Like `new`, but reuses the previous result when the preparer returned the same
    /// geometry (a frame that only changed tiles) under the same display settings.
    pub(in crate::app) fn reuse_or_new(
        previous: &mut Option<Self>,
        geometry: Arc<OverlayGeometry>,
        ctx: &egui::Context,
    ) -> Self {
        if let Some(last) = previous.as_ref().filter(|last| {
            Arc::ptr_eq(&last.geometry, &geometry)
                && last.borders.settings() == MeshSettings::of(ctx)
        }) {
            return last.clone();
        }
        previous.insert(Self::new(geometry, ctx)).clone()
    }
}
