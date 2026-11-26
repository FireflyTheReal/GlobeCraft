//! Cloud/radar fetching, refresh policy and loaded imagery snapshots.
use globecraft_core::language::Lang;
use image::{GrayImage, RgbaImage};
use std::sync::Arc;
mod archive;
mod download;
/// The radar's colours: each band from its lowest reflectivity (dBZ) up, as drawn
/// on the map and in the legend.
pub const RADAR_BANDS: [(f32, [u8; 3]); 5] = [
    (0.0, [42, 205, 60]),
    (18.0, [155, 220, 35]),
    (28.0, [247, 230, 20]),
    (38.0, [255, 145, 0]),
    (47.0, [240, 35, 30]),
];
use download::{download_clouds, download_radar, download_water};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
/// The first moment whose clouds are kept (the archive's first week, 2026-W11,
/// starts on 9 March 2026, Unix seconds): earlier times show none.
pub const CLOUDS_KEPT_SINCE_UNIX: f64 = 1_773_014_400.0;
const CLOUD_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);
const RADAR_INTERVAL: Duration = Duration::from_secs(10 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(60);
/// RainViewer's frames are ten minutes apart.
const RADAR_STEP_SECS: f64 = 600.0;
/// How far back RainViewer's frames reach (two hours) and a step more: an older time
/// is not asked for, its newest frame would come back for every step.
pub const RADAR_PAST_SECS: f64 = 2.0 * 3600.0 + RADAR_STEP_SECS;
/// Radar frames of other moments kept (16 MB each): an hour of them.
const RADAR_KEPT: usize = 6;
/// Kept cloud maps of earlier moments, the last used last: a playing timeline steps
/// through them without loading them again. Bounded by their pixels (bytes, one a
/// pixel): an 8192 map with its levels is about 42 MB, so two of those, or many
/// 2048 ones.
const KEPT_CLOUD_BYTES: usize = 96 * 1024 * 1024;
/// With a loop on the timeline, its moments are loaded ahead at this width (the
/// archive keeps finished weeks at it anyway; only this week's maps are wider) and
/// kept up to this many bytes: about 70 moments, eight and a half days of maps.
const LOOP_CLOUD_PX: u32 = 2048;
const LOOP_CLOUD_BYTES: usize = 192 * 1024 * 1024;
/// A 2048 map with its two smaller levels, bytes.
const LOOP_MAP_BYTES: usize = 2048 * 1024 + 1024 * 512 + 512 * 256;
/// Moments loaded ahead at once.
const MAX_LOADS_AHEAD: usize = 3;
/// Moments loaded ahead of a playing timeline, in the direction it plays.
const CLOUD_SLOTS_AHEAD: i64 = 2;

/// Which clouds the view wants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudWish {
    /// The cloud layer shows.
    pub shown: bool,
    /// The app's time when it is set back (Unix seconds): the clouds kept for it.
    pub time: Option<f64>,
    /// How fast the app's time plays (seconds a second; negative backwards, 0 still):
    /// the moments ahead are loaded meanwhile.
    pub playing: f64,
    /// The widest level kept (pixels).
    pub max_px: u32,
    /// The timeline's loop (Unix seconds), whose moments load ahead.
    pub loop_range: Option<(f64, f64)>,
}

/// Which rain radar the view wants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadarWish {
    /// The radar layer shows.
    pub shown: bool,
    /// The app's time when it is away from now (Unix seconds): RainViewer keeps the
    /// last two hours, in ten-minute frames, and the one for that time is shown.
    pub time: Option<f64>,
}

/// The ten-minute step a radar frame is wanted for; `None` for the newest.
fn radar_step(time: Option<f64>) -> Option<i64> {
    time.map(|t| (t / RADAR_STEP_SECS).floor() as i64 * RADAR_STEP_SECS as i64)
}

/// The clouds' opacity over the whole Earth (equirectangular, north up, from 180° W),
/// finest level first, each half the size of the one before.
pub struct CloudMap {
    pub levels: Vec<GrayImage>,
}

impl CloudMap {
    /// The map without its levels wider than `max_px` (its coarsest kept in any case).
    /// The map's pixels, one byte each, all levels.
    fn bytes(&self) -> usize {
        self.levels.iter().map(|level| level.as_raw().len()).sum()
    }
    fn capped(mut self, max_px: u32) -> Self {
        let keep_from = self
            .levels
            .iter()
            .position(|level| level.width() <= max_px)
            .unwrap_or(self.levels.len().saturating_sub(1));
        self.levels.drain(..keep_from.min(self.levels.len()));
        self
    }
}

struct RadarFrame {
    image: RgbaImage,
    time: i64,
}

/// One image fetched again and again: the last one loaded, the download under way,
/// when it was last asked for and why it last failed.
struct Feed<T> {
    value: Option<T>,
    receiver: Option<mpsc::Receiver<Result<T, crate::Error>>>,
    attempt: Option<Instant>,
    error: Option<String>,
}

impl<T> Default for Feed<T> {
    fn default() -> Self {
        Self {
            value: None,
            receiver: None,
            attempt: None,
            error: None,
        }
    }
}

impl<T: Send + 'static> Feed<T> {
    /// Takes in a finished download; whether a new value came.
    fn receive(&mut self, interrupted: &str) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(crate::Error::msg(interrupted)),
        };
        self.receiver = None;
        match result {
            Ok(value) => {
                self.value = Some(value);
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    /// Starts `download` when it is `wanted` and due: `interval` after the last load,
    /// sooner after a failure.
    fn refresh<F>(
        &mut self,
        wanted: bool,
        interval: Duration,
        runtime: &crate::runtime::ServiceRuntime,
        repaint: &Arc<impl Fn() + Send + Sync + 'static>,
        download: impl FnOnce() -> F,
    ) where
        F: std::future::Future<Output = Result<T, crate::Error>> + Send + 'static,
    {
        let loaded = self.value.is_some() && self.error.is_none();
        if !(crate::live::ENABLED
            && wanted
            && self.receiver.is_none()
            && refresh_due(self.attempt, loaded, interval))
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.attempt = Some(Instant::now());
        let (repaint, download) = (repaint.clone(), download());
        runtime.spawn(async move {
            let _ = sender.send(download.await);
            repaint();
        });
    }

    /// The status line: `failed` with the error (and `cached` while an older image
    /// stays), `loaded` or `loading`.
    fn status(&self, lang: Lang, failed: &str, loaded: String, loading: &str) -> String {
        if let Some(error) = &self.error {
            let cached = if self.value.is_some() {
                lang.text("live-cached-image")
            } else {
                ""
            };
            format!("{}{cached}: {error}", lang.text(failed))
        } else if self.value.is_some() {
            loaded
        } else {
            lang.text(loading).to_owned()
        }
    }
}

#[derive(Default)]
pub struct LiveMaps {
    cloud: Feed<Arc<CloudMap>>,
    /// The three-hour slot the cloud map shown is of, and of the one loading: `None`
    /// for the live map.
    cloud_slot: Option<i64>,
    cloud_loading: Option<i64>,
    /// Maps of earlier moments already loaded, by slot (see `KEPT_CLOUD_BYTES`).
    kept_clouds: Vec<(i64, Arc<CloudMap>)>,
    /// Moments ahead of a playing timeline (or in its loop) being loaded.
    cloud_ahead: Vec<(i64, mpsc::Receiver<Result<CloudMap, crate::Error>>)>,
    /// How many bytes of maps are kept (`KEPT_CLOUD_BYTES`, more with a loop).
    kept_limit: usize,
    /// The loop's moments being loaded ahead, for the status line.
    loop_slots: Vec<i64>,
    /// The widest level the maps were kept at.
    cloud_max_px: u32,
    radar: Feed<RadarFrame>,
    /// The ten-minute step the radar shown was wanted for, and the one loading
    /// (`None`: the newest).
    radar_for: Option<i64>,
    radar_loading_for: Option<i64>,
    /// Radar frames of earlier steps shown, the newest last (at most
    /// [`RADAR_KEPT`]): a timeline playing or looping over them shows them again
    /// without downloading them again.
    radar_kept: std::collections::VecDeque<(i64, RadarFrame)>,
    water: Feed<Arc<GrayImage>>,
    generation: u64,
}
impl LiveMaps {
    /// Takes in finished downloads and starts the ones due: the clouds and the radar
    /// while their layers show, the water's shine while `water` (the sun's glint). The
    /// clouds are those of the wish's time when it is set back before the current three
    /// hours, and the live map otherwise.
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        wish: CloudWish,
        radar: RadarWish,
        water: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        let repaint = Arc::new(repaint);
        let clouds = wish.shown;
        let looping = wish.loop_range.filter(|_| clouds);
        let max_px = match looping {
            Some(_) => wish.max_px.min(LOOP_CLOUD_PX),
            None => wish.max_px,
        };
        self.kept_limit = if looping.is_some() {
            LOOP_CLOUD_BYTES
        } else {
            KEPT_CLOUD_BYTES
        };
        // Another width: the maps are loaded again at it.
        if max_px != self.cloud_max_px {
            self.cloud_max_px = max_px;
            self.kept_clouds.clear();
            self.cloud_ahead.clear();
            // A map loading arrives at the old width: loaded again instead.
            self.cloud.receiver = None;
            (self.cloud.attempt, self.cloud.error) = (None, None);
        }
        self.receive_ahead();
        let cloud_loading = self.cloud.receiver.is_some();
        let cloud_arrived = self.cloud.receive("Cloud download interrupted");
        if cloud_arrived {
            self.cloud_slot = self.cloud_loading;
            if let (Some(slot), Some(map)) = (self.cloud_slot, &self.cloud.value) {
                self.keep_cloud(slot, map.clone());
            }
        } else if cloud_loading && self.cloud.receiver.is_none() && self.cloud_loading.is_some() {
            // No clouds kept for that moment: none shown, rather than another moment's.
            (self.cloud.value, self.cloud_slot) = (None, None);
            self.generation += 1;
        }
        let wanted_radar = radar_step(radar.time);
        let mut radar_arrived = self.receive_radar(wanted_radar);
        // A step kept: shown at once.
        if radar.shown && self.show_kept_radar(wanted_radar) {
            radar_arrived = true;
        }
        let arrived = [
            cloud_arrived,
            radar_arrived,
            self.water.receive("Water map download interrupted"),
        ];
        if arrived.contains(&true) {
            self.generation += 1;
            repaint();
        }
        // Another moment: loaded at once (the picture shown stays until it is in). While
        // the time moves, the next moment waits for the one loading.
        let now = unix_now();
        let wanted = archive::earlier_slot(wish.time, now);
        // A moment loaded already (ahead of the playing time, or shown before): at once.
        if let Some(slot) = wanted.filter(|&slot| Some(slot) != self.cloud_slot)
            && self.cloud.receiver.is_none()
            && let Some(map) = self.kept_cloud(slot)
        {
            self.cloud.value = Some(map);
            (self.cloud_slot, self.cloud_loading) = (Some(slot), Some(slot));
            (self.cloud.attempt, self.cloud.error) = (Some(Instant::now()), None);
            self.generation += 1;
            repaint();
        }
        if wanted != self.cloud_loading && self.cloud.receiver.is_none() {
            (self.cloud.attempt, self.cloud.error) = (None, None);
        }
        let idle = self.cloud.receiver.is_none();
        self.cloud
            .refresh(clouds, CLOUD_INTERVAL, runtime, &repaint, || async move {
                match wanted {
                    Some(slot) => archive::download(slot, now).await,
                    None => download_clouds().await,
                }
                .map(|map| Arc::new(map.capped(max_px)))
            });
        if idle && self.cloud.receiver.is_some() {
            self.cloud_loading = wanted;
        }
        match looping {
            Some(range) => {
                self.loop_slots = loop_slots(
                    range,
                    wanted,
                    wish.playing,
                    now,
                    LOOP_CLOUD_BYTES / LOOP_MAP_BYTES,
                );
                for slot in self.loop_slots.clone() {
                    if self.cloud_ahead.len() >= MAX_LOADS_AHEAD {
                        break;
                    }
                    if !self.has_cloud(slot) {
                        self.load_cloud(slot, now, runtime, &repaint);
                    }
                }
            }
            None => {
                self.loop_slots.clear();
                if clouds
                    && let Some(slot) = wanted
                    && self.cloud_ahead.is_empty()
                    && let Some(next) =
                        slot_ahead(slot, wish.playing, now, |next| self.has_cloud(next))
                {
                    self.load_cloud(next, now, runtime, &repaint);
                }
            }
        }
        // Another moment wanted (the timeline moved, or back to now): its frame at once.
        let wanted = wanted_radar;
        if radar.shown
            && wanted != self.radar_for
            && wanted != self.radar_loading_for
            && self.radar.receiver.is_none()
        {
            // At once, unless the last download failed: then after its pause.
            if self.radar.error.is_none() {
                self.radar.attempt = None;
            }
            self.radar_loading_for = wanted;
        }
        let for_step = self.radar_loading_for;
        self.radar
            .refresh(radar.shown, RADAR_INTERVAL, runtime, &repaint, move || {
                download_radar(for_step)
            });
        // The water's shine is the clouds' companion: as often, from the same source.
        self.water
            .refresh(water, CLOUD_INTERVAL, runtime, &repaint, || async {
                download_water().await.map(Arc::new)
            });
    }

    /// Takes in a moment loaded ahead, if it is in (a failure is passed over: the
    /// moment is loaded again when it is shown).
    fn receive_ahead(&mut self) {
        let mut arrived = Vec::new();
        self.cloud_ahead
            .retain(|(slot, receiver)| match receiver.try_recv() {
                Err(mpsc::TryRecvError::Empty) => true,
                Ok(Ok(map)) => {
                    arrived.push((*slot, Arc::new(map)));
                    false
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => false,
            });
        for (slot, map) in arrived {
            self.keep_cloud(slot, map);
        }
    }

    /// Whether the map of `slot` is kept, shown or loading.
    fn has_cloud(&self, slot: i64) -> bool {
        Some(slot) == self.cloud_loading
            || self.kept_clouds.iter().any(|(kept, _)| *kept == slot)
            || self.cloud_ahead.iter().any(|(loading, _)| *loading == slot)
    }

    /// Keeps `map` of `slot`, the least recently used going past `KEPT_CLOUD_BYTES`
    /// (the newest stays in any case).
    fn keep_cloud(&mut self, slot: i64, map: Arc<CloudMap>) {
        self.kept_clouds.retain(|(kept, _)| *kept != slot);
        self.kept_clouds.push((slot, map));
        let bytes = |kept: &[(i64, Arc<CloudMap>)]| -> usize {
            kept.iter().map(|(_, map)| map.bytes()).sum()
        };
        let limit = self.kept_limit.max(KEPT_CLOUD_BYTES);
        while self.kept_clouds.len() > 1 && bytes(&self.kept_clouds) > limit {
            self.kept_clouds.remove(0);
        }
    }

    /// The kept map of `slot`, now the last used.
    fn kept_cloud(&mut self, slot: i64) -> Option<Arc<CloudMap>> {
        let index = self
            .kept_clouds
            .iter()
            .position(|(kept, _)| *kept == slot)?;
        let entry = self.kept_clouds.remove(index);
        let map = entry.1.clone();
        self.kept_clouds.push(entry);
        Some(map)
    }

    /// Loads the map of `slot` ahead, kept when it is in.
    fn load_cloud(
        &mut self,
        slot: i64,
        now: f64,
        runtime: &crate::runtime::ServiceRuntime,
        repaint: &Arc<impl Fn() + Send + Sync + 'static>,
    ) {
        if !crate::live::ENABLED {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.cloud_ahead.push((slot, receiver));
        let (repaint, max_px) = (repaint.clone(), self.cloud_max_px);
        runtime.spawn(async move {
            let map = archive::download(slot, now).await;
            let _ = sender.send(map.map(|map| map.capped(max_px)));
            repaint();
        });
    }

    /// The cloud map; a new download is a new `Arc`.
    pub fn cloud(&self) -> Option<&Arc<CloudMap>> {
        self.cloud.value.as_ref()
    }
    /// Whether the map shown is the one for the app's `time` (`CloudWish::time`) and
    /// nothing is loading: scripted tours wait for it before drawing a frame.
    pub fn clouds_settled(&self, time: Option<f64>) -> bool {
        self.cloud.receiver.is_none()
            && self.cloud.value.is_some()
            && self.cloud_slot == archive::earlier_slot(time, unix_now())
    }
    pub fn radar(&self) -> Option<&RgbaImage> {
        self.radar.value.as_ref().map(|frame| &frame.image)
    }
    /// Takes in a finished radar download; whether the frame shown changed. The
    /// frame it replaces is kept; one for a step no longer wanted (a kept frame came
    /// on while it loaded) is kept instead of shown.
    fn receive_radar(&mut self, wanted: Option<i64>) -> bool {
        let shown = self.radar.value.take();
        if !self.radar.receive("Radar download interrupted") {
            self.radar.value = shown;
            return false;
        }
        let came_for = self.radar_loading_for;
        if came_for != wanted && shown.is_some() && self.radar_for == wanted {
            let came = self.radar.value.take();
            self.radar.value = shown;
            self.keep_radar(came_for, came);
            return false;
        }
        self.keep_radar(self.radar_for, shown);
        self.radar_for = came_for;
        true
    }

    /// Shows the kept frame of step `wanted`, if there is one: whether it did.
    fn show_kept_radar(&mut self, wanted: Option<i64>) -> bool {
        if wanted.is_none() || wanted == self.radar_for {
            return false;
        }
        let Some(index) = self
            .radar_kept
            .iter()
            .position(|(step, _)| Some(*step) == wanted)
        else {
            return false;
        };
        let Some((_, frame)) = self.radar_kept.remove(index) else {
            return false;
        };
        let replaced = self.radar.value.replace(frame);
        self.keep_radar(self.radar_for, replaced);
        self.radar_for = wanted;
        if self.radar.receiver.is_none() {
            self.radar_loading_for = wanted;
        }
        true
    }

    /// Keeps a radar frame of a past step (not the newest, which is fetched again).
    fn keep_radar(&mut self, step: Option<i64>, frame: Option<RadarFrame>) {
        let (Some(step), Some(frame)) = (step, frame) else {
            return;
        };
        self.radar_kept.retain(|(kept, _)| *kept != step);
        self.radar_kept.push_back((step, frame));
        while self.radar_kept.len() > RADAR_KEPT {
            self.radar_kept.pop_front();
        }
    }

    /// The moment the radar shown was measured (Unix seconds).
    pub fn radar_time(&self) -> Option<i64> {
        self.radar.value.as_ref().map(|frame| frame.time)
    }
    /// How much the water shines (0 to 255; land and cloud dark), equirectangular from
    /// 180° W, north up.
    pub fn water(&self) -> Option<&Arc<GrayImage>> {
        self.water.value.as_ref()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn status(&self, clouds: bool, radar: bool, lang: Lang) -> String {
        let mut lines = Vec::new();
        if clouds {
            let none_kept = self
                .cloud
                .value
                .as_ref()
                .is_some_and(|map| map.levels.is_empty());
            let loaded = match self.cloud_slot {
                Some(slot) if none_kept => lang.format(
                    "live-clouds-none-kept",
                    &[("time", archive::shown(slot).into())],
                ),
                Some(slot) => lang.format(
                    "live-clouds-archived",
                    &[("time", archive::shown(slot).into())],
                ),
                None => lang.text("live-clouds-ok").to_owned(),
            };
            lines.push(self.cloud.status(
                lang,
                "live-clouds-failed",
                loaded,
                "live-clouds-loading",
            ));
        }
        if clouds && !self.loop_slots.is_empty() {
            let loaded = self
                .loop_slots
                .iter()
                .filter(|slot| self.kept_clouds.iter().any(|(kept, _)| kept == *slot))
                .count();
            lines.push(lang.format(
                "live-clouds-loop-loaded",
                &[
                    ("loaded", loaded.into()),
                    ("total", self.loop_slots.len().into()),
                ],
            ));
        }
        if radar {
            let now = unix_now() as i64;
            let age = self
                .radar
                .value
                .as_ref()
                .map_or(0, |frame| (now - frame.time).max(0) / 60);
            lines.push(self.radar.status(
                lang,
                "live-radar-failed",
                lang.format("live-radar-age", &[("minutes", age.to_string().into())]),
                "live-radar-loading",
            ));
        }
        lines.join("\n")
    }
}
/// The moments of the loop `range` with clouds kept (before the current three hours),
/// from `wanted` on in the direction the time plays and round the loop, at most `max`.
fn loop_slots(
    range: (f64, f64),
    wanted: Option<i64>,
    playing: f64,
    now: f64,
    max: usize,
) -> Vec<i64> {
    let slot = |time: f64| (time / archive::SLOT_SECS as f64).floor() as i64;
    let (first, last) = (slot(range.0), slot(range.1).min(slot(now) - 1));
    if last < first {
        return Vec::new();
    }
    let count = last - first + 1;
    let start = wanted
        .filter(|s| (first..=last).contains(s))
        .unwrap_or(first);
    let step = if playing < 0.0 { -1 } else { 1 };
    (0..count.min(max as i64))
        .map(|k| first + (start - first + k * step).rem_euclid(count))
        .collect()
}

/// The moment to load ahead of a time playing (`playing` seconds a second) from
/// `slot`: the first of the next `CLOUD_SLOTS_AHEAD` in the direction it plays that is
/// not `had` already, and never the current three hours or later (the live map).
fn slot_ahead(slot: i64, playing: f64, now: f64, had: impl Fn(i64) -> bool) -> Option<i64> {
    if playing == 0.0 || !playing.is_finite() {
        return None;
    }
    let step = if playing > 0.0 { 1 } else { -1 };
    (1..=CLOUD_SLOTS_AHEAD)
        .map(|k| slot + k * step)
        .take_while(|&next| {
            archive::earlier_slot(Some((next * archive::SLOT_SECS) as f64), now) == Some(next)
        })
        .find(|&next| !had(next))
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn refresh_due(last: Option<Instant>, loaded: bool, interval: Duration) -> bool {
    last.is_none_or(|t| t.elapsed() >= if loaded { interval } else { RETRY_INTERVAL })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radar_frame(time: i64) -> RadarFrame {
        RadarFrame {
            image: RgbaImage::new(1, 1),
            time,
        }
    }

    #[test]
    fn radar_frames_shown_before_come_back_without_a_download() {
        let mut maps = LiveMaps::default();
        // Steps 0..8 shown one after the other, as a playing timeline does.
        for step in 0..8 {
            let shown = maps.radar.value.take();
            maps.keep_radar(maps.radar_for, shown);
            maps.radar.value = Some(radar_frame(step * 600));
            maps.radar_for = Some(step * 600);
        }
        // The last six before the one shown are kept, the oldest gone.
        let kept: Vec<i64> = maps.radar_kept.iter().map(|(step, _)| *step).collect();
        assert_eq!(kept, [600, 1200, 1800, 2400, 3000, 3600]);
        // Back to a kept step: shown at once, the one shown kept instead.
        assert!(maps.show_kept_radar(Some(1200)));
        assert_eq!(maps.radar_time(), Some(1200));
        assert_eq!(maps.radar_for, Some(1200));
        assert!(maps.radar_kept.iter().any(|(step, _)| *step == 4200));
        assert!(!maps.radar_kept.iter().any(|(step, _)| *step == 1200));
        // Not kept, the one shown, or the newest: nothing to show.
        assert!(!maps.show_kept_radar(Some(0)));
        assert!(!maps.show_kept_radar(Some(1200)));
        assert!(!maps.show_kept_radar(None));
        // The newest (live) frame is not kept: it is fetched again anyway.
        maps.keep_radar(None, Some(radar_frame(9_999)));
        assert!(!maps.radar_kept.iter().any(|(_, frame)| frame.time == 9_999));
    }

    #[test]
    fn a_radar_download_for_a_moment_left_is_kept_not_shown() {
        let mut maps = LiveMaps::default();
        maps.radar.value = Some(radar_frame(600));
        maps.radar_for = Some(600);
        // Step 1800 was loading when the timeline went back to the kept step 600.
        let (send, receive) = mpsc::channel();
        maps.radar.receiver = Some(receive);
        maps.radar_loading_for = Some(1800);
        send.send(Ok(radar_frame(1800))).unwrap();
        assert!(!maps.receive_radar(Some(600)));
        assert_eq!(maps.radar_time(), Some(600));
        assert!(maps.radar_kept.iter().any(|(step, _)| *step == 1800));
        // Wanted when it comes: shown, and the one before kept.
        let (send, receive) = mpsc::channel();
        maps.radar.receiver = Some(receive);
        maps.radar_loading_for = Some(2400);
        send.send(Ok(radar_frame(2400))).unwrap();
        assert!(maps.receive_radar(Some(2400)));
        assert_eq!(
            (maps.radar_time(), maps.radar_for),
            (Some(2400), Some(2400))
        );
        assert!(maps.radar_kept.iter().any(|(step, _)| *step == 600));
    }

    #[test]
    fn maps_keep_their_levels_up_to_the_width_chosen() {
        let map = || CloudMap {
            levels: [8192, 4096, 2048].map(|w| GrayImage::new(w, 1)).to_vec(),
        };
        let widths = |map: CloudMap| map.levels.iter().map(|l| l.width()).collect::<Vec<_>>();
        assert_eq!(widths(map().capped(8192)), [8192, 4096, 2048]);
        assert_eq!(widths(map().capped(4096)), [4096, 2048]);
        assert_eq!(widths(map().capped(2048)), [2048]);
        // Narrower than any: the coarsest stays.
        assert_eq!(widths(map().capped(1000)), [2048]);
        assert!(
            CloudMap { levels: Vec::new() }
                .capped(2048)
                .levels
                .is_empty()
        );
    }

    #[test]
    fn kept_maps_are_bounded_by_size_and_the_last_used_stays() {
        let mut maps = LiveMaps::default();
        // A quarter of the room each: four fit.
        let map = || {
            Arc::new(CloudMap {
                levels: vec![GrayImage::new((KEPT_CLOUD_BYTES / 4) as u32, 1)],
            })
        };
        for slot in 0..4 {
            maps.keep_cloud(slot, map());
        }
        // Using the oldest makes it the last to go.
        assert!(maps.kept_cloud(0).is_some());
        maps.keep_cloud(99, map());
        assert_eq!(maps.kept_clouds.len(), 4);
        assert!(maps.kept_cloud(0).is_some());
        assert!(maps.kept_cloud(1).is_none());
        // One larger than the room is still kept, alone.
        maps.keep_cloud(
            7,
            Arc::new(CloudMap {
                levels: vec![GrayImage::new(KEPT_CLOUD_BYTES as u32 + 1, 1)],
            }),
        );
        assert_eq!(maps.kept_clouds.len(), 1);
    }

    #[test]
    fn a_loop_loads_its_moments_from_the_time_on_and_round() {
        let slot_secs = archive::SLOT_SECS as f64;
        let now = 100.5 * slot_secs;
        let range = (90.0 * slot_secs, 94.5 * slot_secs);
        // From the time shown, forward and round to the loop's start.
        assert_eq!(
            loop_slots(range, Some(92), 1.0, now, 10),
            [92, 93, 94, 90, 91]
        );
        // Backwards, the other way round.
        assert_eq!(
            loop_slots(range, Some(92), -1.0, now, 10),
            [92, 91, 90, 94, 93]
        );
        // At most `max`; from the loop's start when the time is outside it.
        assert_eq!(loop_slots(range, Some(50), 1.0, now, 2), [90, 91]);
        // Not the current three hours or later (the live map).
        let into_now = (98.0 * slot_secs, 103.0 * slot_secs);
        assert_eq!(loop_slots(into_now, None, 1.0, now, 10), [98, 99]);
        assert!(loop_slots((101.0 * slot_secs, 102.0 * slot_secs), None, 1.0, now, 10).is_empty());
    }

    #[test]
    fn the_moments_ahead_follow_the_play_and_stop_before_the_live_map() {
        let slot_secs = archive::SLOT_SECS as f64;
        // Now is inside slot 100: slot 99 is the last kept one.
        let now = 100.5 * slot_secs;
        let none = |_| false;
        assert_eq!(slot_ahead(90, 3600.0, now, none), Some(91));
        assert_eq!(slot_ahead(90, -3600.0, now, none), Some(89));
        assert_eq!(slot_ahead(90, 0.0, now, none), None);
        // Past what is had already, at most two ahead.
        assert_eq!(slot_ahead(90, 1.0, now, |s| s == 91), Some(92));
        assert_eq!(slot_ahead(90, 1.0, now, |s| s == 91 || s == 92), None);
        // Never into the current three hours.
        assert_eq!(slot_ahead(98, 1.0, now, none), Some(99));
        assert_eq!(slot_ahead(99, 1.0, now, none), None);
    }
}
