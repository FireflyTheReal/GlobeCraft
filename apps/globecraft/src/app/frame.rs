//! The displayed frame (roadmap 10.2: one of the feature states split out of `App`).
use super::{render_wake, rendering, still_shot, ui};
use eframe::egui;
use std::{sync::Arc, time::Instant};

/// The globe's picture: the renderer, the frame on screen and the one it fades
/// from, and what decides when the next one is asked for.
pub(super) struct FrameState {
    pub(super) renderer: rendering::Renderer,
    pub(super) texture: Option<egui::TextureHandle>,
    /// The picture shown before the current one, while the current one fades in over
    /// it (a new frame of the same view: tiles arrived, or the view came to rest).
    pub(super) fade_from: Option<egui::TextureHandle>,
    pub(super) fade_started: Option<Instant>,
    /// Space behind the frame, at the screen's full resolution.
    pub(super) sky_layer: super::sky_layer::SkyLayer,
    /// The frame on screen as pixels.
    pub(super) image: Option<Frame>,
    /// The state key of the view the last frame was asked for; 0 asks for a new one.
    pub(super) last_state: u64,
    pub(super) wake: render_wake::RenderWakeup,
    pub(super) last_render: Instant,
    pub(super) last_interact: Instant,
    /// The globe's size last frame: a pane dragged wider or narrower renders like a
    /// moving camera, so the view follows the edge at once.
    pub(super) last_viewport: egui::Vec2,
    /// The side pane is sliding open or shut this frame (`ui::panes`).
    pub(super) pane_sliding: bool,
    pub(super) scale_bar: globecraft_render::scale_bar::ScaleBarCache,
    /// A high-quality screenshot rendering in the background.
    pub(super) still_shot: Option<still_shot::StillShot>,
    /// The streaming diagnostics overlay shows (`ui.tileDiagnostics`; not saved).
    pub(super) diagnostics: bool,
    pub(super) download_meter: ui::tile_diagnostics::DownloadMeter,
    pub(super) download_log: DownloadLog,
}

/// A drawn frame: RGBA pixels, row by row.
pub(super) struct Frame {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) pixels: Arc<Vec<u8>>,
}

/// When the download totals were last written to the run log, and what they were.
pub(super) struct DownloadLog {
    pub(super) at: Instant,
    pub(super) bytes: u64,
}

impl FrameState {
    pub(super) fn new(renderer: rendering::Renderer) -> Self {
        Self {
            renderer,
            texture: None,
            fade_from: None,
            fade_started: None,
            sky_layer: Default::default(),
            image: None,
            last_state: 0,
            wake: render_wake::RenderWakeup::default(),
            last_render: Instant::now(),
            last_interact: Instant::now(),
            last_viewport: egui::Vec2::ZERO,
            pane_sliding: false,
            scale_bar: Default::default(),
            still_shot: None,
            diagnostics: false,
            download_meter: Default::default(),
            download_log: DownloadLog {
                at: Instant::now(),
                bytes: 0,
            },
        }
    }
}
