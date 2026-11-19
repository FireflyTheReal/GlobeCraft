//! The app's time (roadmap 4.H): `time.*` commands and what the scene reads.
use super::App;
use globecraft_core::commands::{TimeCommand, params::TimeValue};
use globecraft_core::math::now_unix;
use globecraft_core::time::{HOUR_SECS, parse_iso};
use globecraft_data::tiles::Provider;
use serde_json::{Value, json};

/// The timeline bar (roadmap 10.2: one of the feature states split out of `App`).
pub(super) struct TimelineState {
    /// The timeline bar is open, and the speed its play button runs at (scene
    /// seconds per second).
    pub(super) open: bool,
    pub(super) speed: f64,
    /// The moment dated imagery shows, and when it last moved (it trails a playing
    /// time, so tiles of each day can arrive).
    pub(super) imagery: (f64, std::time::Instant),
    /// How many days the track spans (`time.range`), and where it ends while the time
    /// is away from it (Unix seconds; `None`: at the real time).
    pub(super) range_days: f64,
    pub(super) track_end: Option<f64>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            open: false,
            speed: HOUR_SECS,
            imagery: (0.0, std::time::Instant::now()),
            range_days: 7.0,
            track_end: None,
        }
    }
}

/// The night lights' credit for an edition, with its year.
pub(super) fn night_lights_credit(edition: u16, lang: globecraft_core::language::Lang) -> String {
    let provider = Provider::NightLights;
    match provider.source().edition_year(edition) {
        Some(year) => format!("{} {year}", provider.attribution(lang)),
        None => provider.attribution(lang).to_owned(),
    }
}

/// A playing time moves dated imagery on at most this often.
const IMAGERY_STEP: std::time::Duration = std::time::Duration::from_millis(1500);

/// The play speeds the timeline offers, scene seconds per second.
pub(super) const SPEEDS: [(f64, &str); 4] = [
    (60.0, "timeline-speed-minute"),
    (HOUR_SECS, "timeline-speed-hour"),
    (6.0 * HOUR_SECS, "timeline-speed-six-hours"),
    (24.0 * HOUR_SECS, "timeline-speed-day"),
];
/// The spans the timeline's track offers, days.
pub(super) const RANGES: [(f64, &str); 5] = [
    (1.0, "timeline-range-day"),
    (7.0, "timeline-range-week"),
    (30.0, "timeline-range-month"),
    (365.0, "timeline-range-year"),
    (3650.0, "timeline-range-decade"),
];
/// The track spans at least a day and at most ten years.
const MIN_RANGE_DAYS: f64 = 1.0;
const MAX_RANGE_DAYS: f64 = 3650.0;
/// The event windows the timeline offers, hours (0: everything).
pub(super) const WINDOWS: [(f64, &str); 5] = [
    (0.0, "timeline-window-all"),
    (1.0, "timeline-window-hour"),
    (24.0, "timeline-window-day"),
    (7.0 * 24.0, "timeline-window-week"),
    (30.0 * 24.0, "timeline-window-month"),
];

impl App {
    /// The moment the globe shows (seconds since 1970).
    pub(super) fn scene_unix(&self) -> f64 {
        self.time.now(now_unix())
    }

    /// The moment dated imagery (`{date}` tiles) shows: the app's time, moved at most
    /// every `IMAGERY_STEP` while the time plays.
    pub(super) fn imagery_time(&mut self) -> f64 {
        let scene = self.scene_unix();
        let (shown, moved) = self.timeline.imagery;
        if self.time.speed() == 0.0 || moved.elapsed() >= IMAGERY_STEP || shown == 0.0 {
            self.timeline.imagery = (scene, std::time::Instant::now());
        }
        self.timeline.imagery.0
    }

    /// The night-lights edition for the app's time: the newest composite on or
    /// before its day (NASA's 2016 one, the 2012 one before 2016).
    pub(super) fn night_lights_edition(&self) -> u16 {
        let date = globecraft_core::time::date_utc(self.scene_unix());
        Provider::NightLights.source().edition_for(&date)
    }

    /// The night lights' credit, with the year shown.
    pub(super) fn night_lights_credit(&self, lang: globecraft_core::language::Lang) -> String {
        night_lights_credit(self.night_lights_edition(), lang)
    }

    /// Runs a `time.*` command.
    pub(super) fn run_time(&mut self, command: TimeCommand) -> Result<Value, String> {
        let wall = now_unix();
        match command {
            TimeCommand::Set(p) => match p.time {
                TimeValue::Text(text) if text.eq_ignore_ascii_case("now") => {
                    self.time.go_live();
                }
                TimeValue::Text(text) => {
                    let unix = parse_iso(&text)
                        .ok_or("time must be now, an ISO 8601 date or seconds since 1970")?;
                    self.time.set(unix, wall);
                }
                TimeValue::Number(unix) => self.time.set(unix, wall),
            },
            TimeCommand::Step(p) => self.time.step(p.hours * HOUR_SECS, wall),
            TimeCommand::Play(p) => {
                let speed = p
                    .hours_per_second
                    .map_or(self.timeline.speed, |hours| hours * HOUR_SECS);
                if speed != 0.0 {
                    self.timeline.speed = speed;
                }
                self.time.play(speed, wall);
            }
            TimeCommand::Pause => self.time.play(0.0, wall),
            TimeCommand::Speed(p) => {
                let hours = Some(p.hours_per_second)
                    .filter(|h| *h != 0.0)
                    .ok_or("hours_per_second must be a number other than 0")?;
                self.timeline.speed = hours * HOUR_SECS;
                if self.time.speed() != 0.0 {
                    self.time.play(self.timeline.speed, wall);
                }
            }
            TimeCommand::Window(p) => self.time.set_window(Some(p.hours * HOUR_SECS)),
            TimeCommand::Loop(p) => {
                let moment = |value: TimeValue| match value {
                    TimeValue::Number(unix) => Some(unix),
                    TimeValue::Text(text) if text.eq_ignore_ascii_case("now") => Some(wall),
                    TimeValue::Text(text) => parse_iso(&text),
                };
                let range = match (p.from, p.to) {
                    (None, None) => None,
                    (Some(from), Some(to)) => Some(
                        moment(from)
                            .zip(moment(to))
                            .ok_or("from and to must be ISO 8601 dates or seconds since 1970")?,
                    ),
                    _ => return Err("give both from and to, or neither to end the loop".into()),
                };
                self.time.set_loop(range, wall);
            }
            TimeCommand::Range(p) => {
                if !p.days.is_finite() {
                    return Err("days must be a number".into());
                }
                self.timeline.range_days = p.days.clamp(MIN_RANGE_DAYS, MAX_RANGE_DAYS);
                self.timeline.track_end = None;
            }
        }
        Ok(self.time_json())
    }

    pub(super) fn time_json(&self) -> Value {
        let now = self.scene_unix();
        json!({
            "time": globecraft_core::time::format_utc(now),
            "unix": now,
            "live": self.time.is_live(),
            "hours_per_second": self.time.speed() / HOUR_SECS,
            "window_hours": self.time.window_secs().map(|s| s / HOUR_SECS),
            "range_days": self.timeline.range_days,
            "loop": self.time.loop_range().map(|(from, to)| json!({
                "from": globecraft_core::time::format_utc(from),
                "to": globecraft_core::time::format_utc(to),
            })),
        })
    }
}
