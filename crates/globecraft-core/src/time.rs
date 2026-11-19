//! The app's time (roadmap 4.H): live, a fixed moment, or a moment that moves at a
//! chosen speed, and an optional window of past events. The sun, the stars, the
//! night side, daily imagery and timed points all read it; live data (aircraft,
//! ships, satellites) keeps the real time. Also the calendar arithmetic the app and
//! data use (ISO 8601 dates, UTC formatting), without a date library.

pub const DAY_SECS: f64 = 86_400.0;
pub const HOUR_SECS: f64 = 3_600.0;

/// The app's time. `Default` is live.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppTime {
    /// The scene time at `anchor_wall`, when not live.
    fixed: Option<f64>,
    /// The real time `fixed` was set at.
    anchor_wall: f64,
    /// Scene seconds per real second while playing; 0 holds the time.
    speed: f64,
    /// Timed points show when they happened within this many seconds before the
    /// scene time; `None` shows all.
    window_secs: Option<f64>,
    /// While playing, the time goes round between these two moments (Unix seconds,
    /// the first earlier).
    loop_range: Option<(f64, f64)>,
}

impl AppTime {
    /// Whether the scene follows the real time.
    pub fn is_live(&self) -> bool {
        self.fixed.is_none()
    }
    /// Scene seconds per real second (0: held or live).
    pub fn speed(&self) -> f64 {
        self.speed
    }
    pub fn window_secs(&self) -> Option<f64> {
        self.window_secs
    }
    /// The scene time (seconds since 1970) at real time `wall`. Playing with a loop,
    /// it goes round within the loop (starting into it from wherever it was).
    pub fn now(&self, wall: f64) -> f64 {
        match self.fixed {
            None => wall,
            Some(fixed) => {
                let time = fixed + (wall - self.anchor_wall) * self.speed;
                match self.loop_range {
                    Some((from, to)) if self.speed != 0.0 => {
                        from + (time - from).rem_euclid(to - from)
                    }
                    _ => time,
                }
            }
        }
    }
    /// The loop the time plays round, if any.
    pub fn loop_range(&self) -> Option<(f64, f64)> {
        self.loop_range
    }
    /// Plays round between `from` and `to` (either order; `None`, or two equal
    /// moments, ends the loop). The time shown stays where it is.
    pub fn set_loop(&mut self, range: Option<(f64, f64)>, wall: f64) {
        let now = self.now(wall);
        self.loop_range = range
            .filter(|(a, b)| a.is_finite() && b.is_finite() && a != b)
            .map(|(a, b)| (a.min(b), a.max(b)));
        if self.fixed.is_some() {
            self.set(now, wall);
        }
    }
    /// Shows the moment `unix`, keeping the speed.
    pub fn set(&mut self, unix: f64, wall: f64) {
        self.fixed = Some(unix);
        self.anchor_wall = wall;
    }
    /// Moves the scene time by `secs` (from live, from the real time).
    pub fn step(&mut self, secs: f64, wall: f64) {
        self.set(self.now(wall) + secs, wall);
    }
    /// Runs the time at `speed` scene seconds per real second; 0 holds it.
    pub fn play(&mut self, speed: f64, wall: f64) {
        let now = self.now(wall);
        self.speed = if speed.is_finite() { speed } else { 0.0 };
        self.set(now, wall);
    }
    /// Back to the real time.
    pub fn go_live(&mut self) {
        self.fixed = None;
        self.speed = 0.0;
    }
    pub fn set_window(&mut self, secs: Option<f64>) {
        self.window_secs = secs.filter(|s| s.is_finite() && *s > 0.0);
    }
    /// Whether a point that happened at `unix` shows at real time `wall`: within the
    /// window before the scene time (never after it unless live).
    pub fn shows(&self, unix: f64, wall: f64) -> bool {
        let now = self.now(wall);
        if !self.is_live() && unix > now {
            return false;
        }
        self.window_secs.is_none_or(|window| unix >= now - window)
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date (year, month, day) of a day count since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `2026-11-04` for a moment (UTC).
pub fn date_utc(unix: f64) -> String {
    let (year, month, day) = civil_from_days((unix / DAY_SECS).floor() as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `2026-11-04 09:38 UTC` for a moment.
pub fn format_utc(unix: f64) -> String {
    let minutes = (unix.rem_euclid(DAY_SECS) / 60.0).floor() as i64;
    format!(
        "{} {:02}:{:02} UTC",
        date_utc(unix),
        minutes / 60,
        minutes % 60
    )
}

/// Seconds since 1970 of an ISO 8601 date or date and time (`2026-11-04`,
/// `2026-11-04T09:38`, `2026-11-04 09:38:12.5Z`, `…+01:00`); without a zone, UTC.
pub fn parse_iso(text: &str) -> Option<f64> {
    let text = text.trim();
    let bytes = text.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let part = text.get(from..from + len)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let year = digits(0, 4)?;
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let month = digits(5, 2)?;
    let day = digits(8, 2)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut secs = (days_from_civil(year, month, day) as f64) * DAY_SECS;
    if text.len() == 10 {
        return Some(secs);
    }
    if !matches!(bytes.get(10), Some(b'T' | b't' | b' ')) || bytes.get(13) != Some(&b':') {
        return None;
    }
    let hour = digits(11, 2)?;
    let minute = digits(14, 2)?;
    secs += ((hour * 60 + minute) * 60) as f64;
    let mut rest = text.get(16..)?;
    if let Some(seconds) = rest.strip_prefix(':') {
        let end = seconds
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(seconds.len());
        secs += seconds[..end].parse::<f64>().ok()?;
        rest = &seconds[end..];
    }
    // Matched on bytes: text from a user's file may hold any character here, and slicing
    // a string by byte position would panic inside a multi-byte one.
    match rest.as_bytes() {
        b"" | b"Z" | b"z" => Some(secs),
        &[sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]
            if [h1, h2, m1, m2].iter().all(u8::is_ascii_digit) =>
        {
            let two_digits = |tens: u8, ones: u8| f64::from((tens - b'0') * 10 + (ones - b'0'));
            let offset = (two_digits(h1, h2) * 60.0 + two_digits(m1, m2)) * 60.0;
            Some(if sign == b'+' {
                secs - offset
            } else {
                secs + offset
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALL: f64 = 1_793_785_080.0; // 2026-11-04 09:38 UTC

    #[test]
    fn live_follows_the_real_time_and_a_fixed_time_holds_or_plays() {
        let mut time = AppTime::default();
        assert!(time.is_live());
        assert_eq!(time.now(WALL), WALL);
        time.step(-DAY_SECS, WALL);
        assert!(!time.is_live());
        assert_eq!(time.now(WALL + 60.0), WALL - DAY_SECS);
        // An hour per second: ten real seconds later, ten hours on.
        time.play(HOUR_SECS, WALL + 60.0);
        assert_eq!(time.now(WALL + 70.0), WALL - DAY_SECS + 10.0 * HOUR_SECS);
        // Held where it was.
        time.play(0.0, WALL + 70.0);
        assert_eq!(time.now(WALL + 1_000.0), WALL - DAY_SECS + 10.0 * HOUR_SECS);
        time.go_live();
        assert_eq!((time.now(WALL), time.speed()), (WALL, 0.0));
    }

    #[test]
    fn a_loop_plays_round_and_holds_still_when_paused() {
        let wall = 1_000.0;
        let mut time = AppTime::default();
        time.set(500.0, wall);
        time.set_loop(Some((600.0, 400.0)), wall);
        assert_eq!(time.loop_range(), Some((400.0, 600.0)));
        // Held, the time stays where it was set.
        assert_eq!(time.now(wall + 50.0), 500.0);
        time.play(10.0, wall);
        assert_eq!(time.now(wall + 5.0), 550.0);
        // Past the end it goes on from the start.
        assert_eq!(time.now(wall + 15.0), 450.0);
        // Backwards past the start it comes in at the end.
        time.play(-10.0, wall + 15.0);
        assert_eq!(time.now(wall + 21.0), 590.0);
        // Ended, the time runs on.
        time.set_loop(None, wall + 21.0);
        time.play(10.0, wall + 21.0);
        assert_eq!(time.now(wall + 31.0), 690.0);
        time.set_loop(Some((5.0, 5.0)), wall);
        assert_eq!(time.loop_range(), None);
    }

    #[test]
    fn the_window_shows_points_up_to_the_scene_time() {
        let mut time = AppTime::default();
        assert!(time.shows(WALL - 30.0 * DAY_SECS, WALL));
        time.set_window(Some(DAY_SECS));
        assert!(time.shows(WALL - HOUR_SECS, WALL));
        assert!(!time.shows(WALL - 2.0 * DAY_SECS, WALL));
        // A day back: what happened after it is not shown yet.
        time.step(-DAY_SECS, WALL);
        assert!(!time.shows(WALL - HOUR_SECS, WALL));
        assert!(time.shows(WALL - DAY_SECS - HOUR_SECS, WALL));
        time.set_window(Some(0.0));
        assert_eq!(time.window_secs(), None);
    }

    #[test]
    fn dates_parse_and_format_in_utc() {
        assert_eq!(parse_iso("1970-01-02"), Some(DAY_SECS));
        assert_eq!(parse_iso("2026-11-04T09:38:00Z"), Some(WALL));
        assert_eq!(parse_iso("2026-11-04 10:38+01:00"), Some(WALL));
        assert_eq!(parse_iso("2026-11-04T09:38:00.5"), Some(WALL + 0.5));
        assert_eq!(parse_iso("yesterday"), None);
        assert_eq!(parse_iso("2026-13-01"), None);
        assert_eq!(
            parse_iso("2026-11-04T09:38-02:30"),
            Some(WALL + 2.5 * HOUR_SECS)
        );
        // Text from a user's file: no panic on characters of several bytes, wherever they are.
        for odd in [
            "2026-11-04T09:38é1234",
            "2026-11-04T09:38+1:é0",
            "2026-11-04T09:38:é",
            "2026-11-04T09é38",
            "2026-11-04Té",
            "2026-11-0é",
            "2026-11-04T09:38+01:0x",
        ] {
            assert_eq!(parse_iso(odd), None, "{odd}");
        }
        // Text after the time that is not a zone, in any script, is no date (a
        // six-byte tail of two-byte letters used to be cut inside a letter).
        assert_eq!(parse_iso("2026-11-04T09:38ééé"), None);
        assert_eq!(parse_iso("2026-11-04T09:38:00€uro"), None);
        assert_eq!(parse_iso("2026-11-04é09:38"), None);
        assert_eq!(format_utc(WALL), "2026-11-04 09:38 UTC");
        assert_eq!(format_utc(0.0), "1970-01-01 00:00 UTC");
        for days in [-1000_i64, 11_016, 47_482] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), days);
        }
    }

    #[test]
    fn any_text_is_parsed_or_refused_without_panicking() {
        let mut inputs = crate::test_inputs::Inputs::new(1);
        let pieces = [
            "0",
            "1",
            "2",
            "9",
            "-",
            ":",
            "T",
            "t",
            " ",
            "+",
            "Z",
            ".",
            "é",
            "日",
            "2026-11-04",
            "T09:38",
            ":12.5",
            "+01:00",
            "\u{0}",
        ];
        for _ in 0..50_000 {
            let text = inputs.text(&pieces, 12);
            let _ = parse_iso(&text);
        }
    }
}
