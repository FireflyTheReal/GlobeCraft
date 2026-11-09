//! Map-label galleys kept across overlay rebuilds.
//!
//! egui drops every galley that was not laid out in the previous frame. Overlay shapes
//! are rebuilt only when new geometry arrives, a few frames apart, so each rebuild laid
//! out all 500–1,000 label candidates from scratch: about 50 µs each, 5–25 ms per
//! rebuild while descending. Kept here, a rebuild costs a map lookup per candidate.
use eframe::egui::{self, Color32, FontFamily, FontId, Galley};
use std::collections::HashMap;
use std::sync::Arc;

/// Rebuilds a galley may go unused before it is dropped, once the cache is large.
const UNUSED_REBUILDS: u32 = 32;
/// Galleys kept before unused ones are dropped (each is a few hundred bytes).
const MAX_GALLEYS: usize = 8_192;
/// New names laid out per rebuild. A layer arriving brings ~1,000 new names at once
/// (about 50 µs each); the rest follow over the next frames, highest priority first,
/// instead of one 50 ms frame.
const NEW_LAYOUTS_PER_REBUILD: usize = 120;

struct Entry {
    galley: Arc<Galley>,
    used: u32,
}

#[derive(Default)]
pub(super) struct LabelGalleys {
    /// Keyed by font family and size bits (`FontId` is not `Eq`).
    by_font: HashMap<(FontFamily, u32), HashMap<String, Entry>>,
    len: usize,
    rebuild: u32,
    new_layouts_left: usize,
    /// What the galleys were laid out for; a change drops them all.
    pixels_per_point: f32,
    atlas_fill: f32,
}

impl LabelGalleys {
    /// Starts one overlay rebuild. Galleys reference glyphs in egui's font atlas, which
    /// egui recreates (emptier) when it fills up or text options change, so a lower
    /// fill ratio or a new scale invalidates every cached galley.
    pub(super) fn begin(&mut self, ctx: &egui::Context) {
        let pixels_per_point = ctx.pixels_per_point();
        let atlas_fill = ctx.fonts(|fonts| fonts.font_atlas_fill_ratio());
        if pixels_per_point != self.pixels_per_point || atlas_fill < self.atlas_fill {
            self.clear();
        }
        self.pixels_per_point = pixels_per_point;
        self.atlas_fill = atlas_fill;
        self.rebuild = self.rebuild.wrapping_add(1);
        self.new_layouts_left = NEW_LAYOUTS_PER_REBUILD;
        if self.len > MAX_GALLEYS {
            let rebuild = self.rebuild;
            let mut len = 0;
            for galleys in self.by_font.values_mut() {
                galleys.retain(|_, entry| rebuild.wrapping_sub(entry.used) <= UNUSED_REBUILDS);
                len += galleys.len();
            }
            self.by_font.retain(|_, galleys| !galleys.is_empty());
            self.len = len;
        }
    }

    pub(super) fn clear(&mut self) {
        self.by_font.clear();
        self.len = 0;
    }

    /// The galley for `text`, laid out in white so shapes can recolour it, or `None`
    /// when this rebuild has used its budget of new layouts.
    pub(super) fn get(
        &mut self,
        painter: &egui::Painter,
        text: &str,
        font: &FontId,
    ) -> Option<Arc<Galley>> {
        let rebuild = self.rebuild;
        if let Some(entry) = self
            .by_font
            .get_mut(&(font.family.clone(), font.size.to_bits()))
            .and_then(|galleys| galleys.get_mut(text))
        {
            entry.used = rebuild;
            return Some(entry.galley.clone());
        }
        if self.new_layouts_left == 0 {
            return None;
        }
        self.new_layouts_left -= 1;
        let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE);
        self.by_font
            .entry((font.family.clone(), font.size.to_bits()))
            .or_default()
            .insert(
                text.to_owned(),
                Entry {
                    galley: galley.clone(),
                    used: rebuild,
                },
            );
        self.len += 1;
        Some(galley)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galleys_are_reused_and_dropped_when_the_scale_changes() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        {
            let ctx = &ctx;
            let painter = ctx.layer_painter(egui::LayerId::background());
            let font = FontId::proportional(10.0);
            let mut galleys = LabelGalleys::default();
            galleys.begin(ctx);
            let first = galleys.get(&painter, "Bavaria", &font).unwrap();
            galleys.begin(ctx);
            assert!(Arc::ptr_eq(
                &first,
                &galleys.get(&painter, "Bavaria", &font).unwrap()
            ));
            assert!(!Arc::ptr_eq(
                &first,
                &galleys
                    .get(&painter, "Bavaria", &FontId::proportional(12.0))
                    .unwrap()
            ));
            assert_eq!(galleys.len, 2);
            // egui itself may still return the same galley this frame; what matters is
            // that ours were dropped and laid out again.
            galleys.pixels_per_point = 3.0;
            galleys.begin(ctx);
            assert_eq!(galleys.len, 0);
            galleys.get(&painter, "Bavaria", &font);
            assert_eq!(galleys.len, 1);
        }
        ctx.end_pass().textures_delta.clear();
    }

    #[test]
    fn unused_galleys_are_dropped_once_the_cache_is_full() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        {
            let ctx = &ctx;
            let painter = ctx.layer_painter(egui::LayerId::background());
            let font = FontId::proportional(10.0);
            let mut galleys = LabelGalleys::default();
            galleys.begin(ctx);
            galleys.new_layouts_left = usize::MAX;
            for i in 0..=MAX_GALLEYS {
                galleys.get(&painter, &i.to_string(), &font);
            }
            for _ in 0..=UNUSED_REBUILDS {
                galleys.begin(ctx);
                galleys.get(&painter, "kept", &font);
            }
            galleys.begin(ctx);
            assert_eq!(galleys.len, 1);
        }
        ctx.end_pass().textures_delta.clear();
    }

    #[test]
    fn new_layouts_per_rebuild_are_bounded_and_cached_names_are_free() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        {
            let ctx = &ctx;
            let painter = ctx.layer_painter(egui::LayerId::background());
            let font = FontId::proportional(10.0);
            let mut galleys = LabelGalleys::default();
            galleys.begin(ctx);
            let names: Vec<String> = (0..NEW_LAYOUTS_PER_REBUILD + 5)
                .map(|i| format!("Name {i}"))
                .collect();
            let laid_out = |galleys: &mut LabelGalleys| {
                names
                    .iter()
                    .filter(|name| galleys.get(&painter, name, &font).is_some())
                    .count()
            };
            assert_eq!(laid_out(&mut galleys), NEW_LAYOUTS_PER_REBUILD);
            // The next rebuild reuses those and lays out the rest.
            galleys.begin(ctx);
            assert_eq!(laid_out(&mut galleys), names.len());
        }
        ctx.end_pass().textures_delta.clear();
    }
}
