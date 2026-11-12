//! The globe picture: the renderer that draws it, the frame on screen and the one it
//! fades from, when to draw again, and the imagery loaded ahead of where the view goes.
use super::{render_wake, rendering};
use eframe::egui;
use globecraft_data::tiles::TileWarmup;
use std::{sync::Arc, time::Instant};

/// A drawn frame: RGBA pixels, row by row.
pub(super) struct Frame {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) pixels: Arc<Vec<u8>>,
}

pub(super) struct Imagery {
    pub(super) renderer: rendering::Renderer,
    pub(super) texture: Option<egui::TextureHandle>,
    /// The picture shown before the current one, while the current one fades in over
    /// it (a new frame of the same view: tiles arrived, or the view came to rest).
    pub(super) fade_from: Option<egui::TextureHandle>,
    pub(super) fade_started: Option<Instant>,
    /// The last frame's pixels.
    pub(super) image: Option<Frame>,
    /// The state key of the view last drawn; 0 asks for a new frame.
    pub(super) last_state: u64,
    pub(super) wake: render_wake::RenderWakeup,
    pub(super) last_render: Instant,
    /// Imagery loaded for the startup view, then for where the view rests.
    pub(super) warmup: TileWarmup,
    /// Imagery loaded ahead of a zoom out, for the view (state key) it was made for.
    pub(super) zoom_out_warmup: Option<(u64, TileWarmup)>,
    /// The imagery a running flight lands on, loaded while it flies (roadmap 1.4),
    /// with the destination it was listed for.
    pub(super) arrival_warmup: Option<([u64; 5], TileWarmup)>,
}

impl Imagery {
    pub(super) fn new(renderer: rendering::Renderer) -> Self {
        Self {
            renderer,
            texture: None,
            fade_from: None,
            fade_started: None,
            image: None,
            last_state: 0,
            wake: render_wake::RenderWakeup::default(),
            last_render: Instant::now(),
            warmup: TileWarmup::startup_satellite(),
            zoom_out_warmup: None,
            arrival_warmup: None,
        }
    }
}
