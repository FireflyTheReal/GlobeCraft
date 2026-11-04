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
                self.wayback_time_date = None;
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

    /// Away from the real time, Wayback shows the newest release on or before the
    /// app's day (roadmap 4.H). Picked again only when that day changes, so a release
    /// chosen by hand stays until the time moves.
    pub(super) fn wayback_follow_time(&mut self) {
        if self.time.is_live() || self.wayback_list.is_empty() {
            self.wayback_time_date = None;
            return;
        }
        let date = globecraft_core::time::date_utc(self.scene_unix());
        if self.wayback_time_date.as_deref() == Some(date.as_str()) {
            return;
        }
        let index = release_on_or_before(self.wayback_list.iter().map(|r| r.date.as_str()), &date);
        self.wayback_time_date = Some(date);
        self.set_wayback(index);
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

/// The index of the newest release dated on or before `date` in a list sorted newest
/// first; the oldest release for days before any.
fn release_on_or_before<'a>(dates: impl ExactSizeIterator<Item = &'a str>, date: &str) -> usize {
    let count = dates.len();
    let mut dates = dates;
    dates
        .position(|release| release <= date)
        .unwrap_or(count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::release_on_or_before;

    #[test]
    fn the_release_shown_is_the_newest_on_or_before_the_day() {
        let dates = ["2026-10-01", "2024-05-02", "2014-02-20"];
        let pick = |day: &str| release_on_or_before(dates.iter().copied(), day);
        assert_eq!(pick("2026-11-04"), 0);
        assert_eq!(pick("2026-10-01"), 0);
        assert_eq!(pick("2026-09-30"), 1);
        assert_eq!(pick("2015-01-01"), 2);
        assert_eq!(pick("2001-01-01"), 2);
    }
}
