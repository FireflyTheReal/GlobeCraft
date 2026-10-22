use super::App;
use eframe::egui;
use globecraft_core::math::now_unix;
use globecraft_data::{
    tiles::Provider,
    wayback::{load_wayback_releases, set_wayback_releases},
};
use std::sync::mpsc;
impl App {
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
