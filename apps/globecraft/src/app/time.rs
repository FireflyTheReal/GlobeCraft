//! The app's time (roadmap 4.H): `time.*` commands and what the scene reads.
use super::App;
use globecraft_core::math::now_unix;
use globecraft_core::time::{HOUR_SECS, parse_iso};
use serde_json::{Value, json};

/// A playing time moves dated imagery on at most this often.
const IMAGERY_STEP: std::time::Duration = std::time::Duration::from_millis(1500);

/// The play speeds the timeline offers, scene seconds per second.
pub(super) const SPEEDS: [(f64, &str); 4] = [
    (60.0, "timeline-speed-minute"),
    (HOUR_SECS, "timeline-speed-hour"),
    (6.0 * HOUR_SECS, "timeline-speed-six-hours"),
    (24.0 * HOUR_SECS, "timeline-speed-day"),
];
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
        let (shown, moved) = self.imagery_time;
        if self.time.speed() == 0.0 || moved.elapsed() >= IMAGERY_STEP || shown == 0.0 {
            self.imagery_time = (scene, std::time::Instant::now());
        }
        self.imagery_time.0
    }

    /// Runs a `time.*` command.
    pub(super) fn run_time(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let wall = now_unix();
        match id {
            "time.set" => match p.get("time") {
                Some(Value::String(text)) if text.trim().eq_ignore_ascii_case("now") => {
                    self.time.go_live();
                }
                Some(Value::String(text)) => {
                    let unix = parse_iso(text)
                        .ok_or("time must be now, an ISO 8601 date or seconds since 1970")?;
                    self.time.set(unix, wall);
                }
                Some(Value::Number(n)) => {
                    self.time
                        .set(n.as_f64().ok_or("time must be a number")?, wall);
                }
                _ => return Err("give a time: now, an ISO 8601 date or seconds since 1970".into()),
            },
            "time.step" => {
                let hours = p
                    .get("hours")
                    .and_then(Value::as_f64)
                    .filter(|h| h.is_finite())
                    .ok_or("hours must be a number")?;
                self.time.step(hours * HOUR_SECS, wall);
            }
            "time.play" => {
                let speed = p
                    .get("hours_per_second")
                    .and_then(Value::as_f64)
                    .filter(|s| s.is_finite())
                    .map_or(self.timeline_speed, |hours| hours * HOUR_SECS);
                if speed != 0.0 {
                    self.timeline_speed = speed;
                }
                self.time.play(speed, wall);
            }
            "time.pause" => self.time.play(0.0, wall),
            "time.speed" => {
                let hours = p
                    .get("hours_per_second")
                    .and_then(Value::as_f64)
                    .filter(|h| h.is_finite() && *h != 0.0)
                    .ok_or("hours_per_second must be a number other than 0")?;
                self.timeline_speed = hours * HOUR_SECS;
                if self.time.speed() != 0.0 {
                    self.time.play(self.timeline_speed, wall);
                }
            }
            "time.window" => {
                let hours = p
                    .get("hours")
                    .and_then(Value::as_f64)
                    .ok_or("hours must be a number (0: everything)")?;
                self.time.set_window(Some(hours * HOUR_SECS));
            }
            _ => return Err(format!("unknown time command {id}")),
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
        })
    }
}
