//! The legend images of web-map layers served by a WMS (GetLegendGraphic), fetched
//! once per layer in the background and kept as textures for the Layers pane.
use eframe::egui::{self, ColorImage, TextureHandle};
use globecraft_data::user_data::{UserLayer, UserSource, legend_image, wms_legend_url};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Answer = Arc<Mutex<Option<Result<ColorImage, String>>>>;

enum Legend {
    Loading(Answer),
    Ready(TextureHandle),
    /// No legend to show (the reason went to the log).
    Failed,
}

#[derive(Default)]
pub(in crate::app) struct Legends {
    by_layer: HashMap<(u64, String), Legend>,
}

impl Legends {
    /// Drops the legends of layers no longer there (or whose address changed).
    pub(super) fn retain(&mut self, layers: &[UserLayer]) {
        self.by_layer.retain(|(id, url), _| {
            layers
                .iter()
                .any(|layer| layer.id == *id && legend_url(layer).as_deref() == Some(url))
        });
    }

    /// The legend of `layer`, when it is a WMS layer whose legend has loaded; asks
    /// for it the first time.
    pub(in crate::app) fn get(
        &mut self,
        ctx: &egui::Context,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        layer: &UserLayer,
    ) -> Option<TextureHandle> {
        let url = legend_url(layer)?;
        let key = (layer.id, url.clone());
        let legend = self.by_layer.entry(key).or_insert_with(|| {
            let answer: Answer = Arc::default();
            let slot = answer.clone();
            let wake = ctx.clone();
            runtime.spawn(async move {
                let result = legend_image(url)
                    .await
                    .map_err(|e| e.to_string())
                    .map(|image| {
                        let size = [image.width() as usize, image.height() as usize];
                        ColorImage::from_rgba_unmultiplied(size, image.as_raw())
                    });
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(result);
                }
                wake.request_repaint();
            });
            Legend::Loading(answer)
        });
        if let Legend::Loading(answer) = legend {
            let done = answer.lock().ok().and_then(|mut slot| slot.take());
            match done {
                Some(Ok(image)) => {
                    let name = format!("user-layer-legend-{}", layer.id);
                    *legend = Legend::Ready(ctx.load_texture(name, image, Default::default()));
                }
                Some(Err(error)) => {
                    eprintln!("Legend of layer {} not loaded: {error}", layer.name);
                    *legend = Legend::Failed;
                }
                None => {}
            }
        }
        match legend {
            Legend::Ready(texture) => Some(texture.clone()),
            Legend::Loading(_) | Legend::Failed => None,
        }
    }
}

fn legend_url(layer: &UserLayer) -> Option<String> {
    match &layer.source {
        UserSource::Tiles { template, .. } => wms_legend_url(template),
        UserSource::Features { .. } => None,
    }
}
