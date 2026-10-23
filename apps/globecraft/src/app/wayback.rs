use super::App;
use eframe::egui;
use globecraft_core::math::now_unix;
use globecraft_data::{
    tiles::Provider,
    wayback::{load_wayback_releases, set_wayback_releases},
};
use std::sync::mpsc;
/// A swipe compare between the current release and another (see `RenderCfg::compare`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Compare {
    /// The release index shown right of the split.
    pub(super) index: usize,
    /// Where the split runs, as a fraction of the view's width.
    pub(super) split: f32,
}
/// The split stays this far from the view's edges, so its handle can be grabbed.
pub(super) const MIN_SPLIT: f32 = 0.02;

impl App {
    /// The swipe compare, when the Wayback basemap is showing and a compare is set.
    pub(super) fn active_compare(&self) -> Option<Compare> {
        self.wayback_compare
            .filter(|_| self.effective_provider() == Provider::Wayback)
            .filter(|compare| compare.index < self.wayback_list.len())
    }

    /// Starts a compare with the newest release (or the next one, when the newest is
    /// already showing), or ends it.
    pub(super) fn set_wayback_compare(&mut self, enabled: bool) {
        self.wayback_compare = (enabled && self.wayback_list.len() > 1).then(|| Compare {
            index: if self.wayback_idx == 0 { 1 } else { 0 },
            split: self.wayback_compare.map_or(0.5, |old| old.split),
        });
        self.last_state = 0;
    }

    pub(super) fn ensure_wayback(&mut self, ctx: &egui::Context) {
        if self.settings.layers.basemap != Provider::Wayback || self.wayback_started {
            return;
        }
        self.wayback_started = true;
        self.wayback_err = None;
        let (tx, rx) = mpsc::channel();
        self.wayback_rx = Some(rx);
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(load_wayback_releases().await);
            ctx.request_repaint();
        });
    }

    pub(super) fn poll_wayback(&mut self) {
        let Some(rx) = &self.wayback_rx else { return };
        match rx.try_recv() {
            Ok(Ok(list)) => {
                self.wayback_rx = None;
                set_wayback_releases(list.clone());
                // default: the latest release that is at least ~5 years old, so the difference is visible
                let year = 1970 + (now_unix() / 31_556_952.0) as i32;
                let limit = format!("{}-12-31", year - 5);
                self.wayback_idx = list.iter().position(|r| r.date <= limit).unwrap_or(0);
                self.wayback_list = list;
                self.last_state = 0;
                self.wake.dependencies_may_have_changed();
            }
            Ok(Err(e)) => {
                self.wayback_rx = None;
                self.wayback_started = false;
                self.wayback_err = Some(e.to_string());
                self.settings.layers.basemap = Provider::Satellite;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.wayback_rx = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(super) fn set_wayback(&mut self, idx: usize) {
        if idx < self.wayback_list.len() && idx != self.wayback_idx {
            self.wayback_idx = idx;
            self.last_state = 0;
        }
    }

    pub(super) fn effective_provider(&self) -> Provider {
        if self.settings.layers.basemap == Provider::Wayback && self.wayback_list.is_empty() {
            Provider::Satellite // until the list of dates is loaded
        } else {
            self.settings.layers.basemap
        }
    }
}
