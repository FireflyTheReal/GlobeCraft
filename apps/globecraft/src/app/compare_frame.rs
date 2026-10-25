//! The Wayback swipe compare's second release, rendered as its own frame.
//!
//! The main frame shows the chosen release everywhere; this worker renders the same
//! view with the compare release, and the globe draws it right of the split (a clip of
//! the finished texture). Moving the split therefore never renders anything: it was
//! the per-column split inside one frame that made every drag re-render the globe.
use eframe::egui;
use globecraft_render::worker::{RenderRequest, RenderWorker};
use std::sync::Arc;

pub(super) struct CompareRenderer {
    worker: RenderWorker<egui::ColorImage>,
    pub(super) texture: Option<egui::TextureHandle>,
    /// The release the texture shows.
    pub(super) release: Option<u16>,
}

impl CompareRenderer {
    pub(super) fn new(cache: Arc<globecraft_data::tiles::TileCache>, ctx: egui::Context) -> Self {
        let admission_cache = cache.clone();
        let mut backend = globecraft_render::backend::FrameRenderer::new(
            globecraft_render::backend::BackendPreference::from_environment(),
        );
        Self {
            worker: RenderWorker::with_prepare(
                move |request, buffer| backend.render(request, &cache, buffer),
                move || ctx.request_repaint(),
                move |frame| admission_cache.prioritize_view(&frame.required_tiles()),
                |request, pixels| {
                    egui::ColorImage::from_rgba_unmultiplied(
                        [request.width, request.height],
                        pixels,
                    )
                },
            )
            .expect("start compare frame renderer"),
            texture: None,
            release: None,
        }
    }

    pub(super) fn submit(&self, request: RenderRequest) {
        self.worker.submit(request);
    }

    /// Takes a finished frame of this presentation into the texture.
    pub(super) fn poll(&mut self, presentation: u64, ctx: &egui::Context) {
        let Some(frame) = self.worker.take_completed(presentation) else {
            return;
        };
        let size = [frame.request.width, frame.request.height];
        let image = frame.prepared;
        match &mut self.texture {
            Some(texture) if texture.size() == size => {
                texture.set_partial([0, 0], image, egui::TextureOptions::LINEAR)
            }
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.texture =
                    Some(ctx.load_texture("globe-compare", image, egui::TextureOptions::LINEAR))
            }
        }
        self.worker.recycle(frame.pixels);
        self.release = Some(frame.request.cfg.rel);
    }
}
