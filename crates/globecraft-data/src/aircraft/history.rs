//! The last hour of the aircraft this session has seen, so the timeline can show
//! them where they were (user, 2026-11-20: "the timeline should affect all things
//! possible"). The feeds keep no past: what is replayed is what came in while the
//! app ran, one sample a plane at most every [`SAMPLE_SECS`], and in between a plane
//! is put on the great circle between its two samples.
use super::{Aircraft, AircraftClass, AircraftDetails};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

/// How far back the history reaches.
pub const KEEP_SECS: f64 = 3600.0;
/// A plane's reports closer than this to its last sample are not kept: the local
/// feed reports every few seconds, and a sample every 15 s is a smooth path.
const SAMPLE_SECS: f64 = 15.0;
/// Two samples of a plane further apart than this are not joined (it left the feeds'
/// view between them). The global feed is fetched every 17 minutes.
const MAX_GAP_SECS: f64 = 20.0 * 60.0;
/// After its last sample a plane is handed on with that sample's time, and the map
/// flies it on as it does a live one, for as long: the global feed's reports are 17
/// minutes apart, and without this its planes would vanish from a replay between
/// them.
const FLY_ON_SECS: f64 = 20.0 * 60.0;
/// All the samples kept, at most (about 30 MB): the oldest go first beyond it.
const MAX_SAMPLES: usize = 1_000_000;

/// One report of a plane, kept small.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    unix: f64,
    lat: f32,
    lon: f32,
    heading: f32,
    /// NaN when not reported.
    altitude_feet: f32,
    speed_knots: f32,
}

struct Track {
    callsign: Arc<str>,
    class: AircraftClass,
    details: Option<Arc<AircraftDetails>>,
    samples: VecDeque<Sample>,
}

#[derive(Default)]
pub struct AircraftHistory {
    tracks: HashMap<Arc<str>, Track>,
    samples: usize,
    /// Since when the planes are known without a gap: recording began, or began
    /// again after the layer was off (no plane is known in such a gap).
    since: Option<f64>,
    /// When the last snapshot was recorded.
    last_record: Option<f64>,
}

impl AircraftHistory {
    /// Keeps the planes of a new snapshot, at real time `now`.
    pub fn record(&mut self, aircraft: &[Aircraft], now: f64) {
        if self
            .last_record
            .is_none_or(|last| now - last > MAX_GAP_SECS)
        {
            self.since = Some(now);
        }
        self.last_record = Some(now);
        for plane in aircraft {
            let track = self
                .tracks
                .entry(plane.hex.clone())
                .or_insert_with(|| Track {
                    callsign: plane.callsign.clone(),
                    class: plane.class,
                    details: None,
                    samples: VecDeque::new(),
                });
            track.callsign = plane.callsign.clone();
            track.class = plane.class;
            if plane.details.is_some() {
                track.details = plane.details.clone();
            }
            if track
                .samples
                .back()
                .is_some_and(|last| plane.observed_unix < last.unix + SAMPLE_SECS)
            {
                continue;
            }
            let reported = |value: Option<f64>| value.map_or(f32::NAN, |v| v as f32);
            track.samples.push_back(Sample {
                unix: plane.observed_unix,
                lat: plane.latitude as f32,
                lon: plane.longitude as f32,
                heading: plane.heading_deg as f32,
                altitude_feet: reported(plane.altitude_feet),
                speed_knots: reported(plane.speed_knots),
            });
            self.samples += 1;
        }
        self.forget_before(now - KEEP_SECS);
        // Over the limit: the oldest tenth of the hour goes, until it fits.
        let mut cut = now - KEEP_SECS;
        while self.samples > MAX_SAMPLES && cut < now {
            cut += KEEP_SECS / 10.0;
            self.forget_before(cut);
        }
    }

    fn forget_before(&mut self, cut: f64) {
        let mut forgotten = 0;
        self.tracks.retain(|_, track| {
            while track.samples.front().is_some_and(|s| s.unix < cut) {
                track.samples.pop_front();
                forgotten += 1;
            }
            !track.samples.is_empty()
        });
        self.samples -= forgotten;
        if let Some(since) = &mut self.since {
            *since = since.max(cut);
        }
    }

    /// Whether the history reaches back to `unix` (and it is not after `now`).
    pub fn covers(&self, unix: f64, now: f64) -> bool {
        self.since.is_some_and(|since| unix >= since) && unix <= now
    }

    /// The planes where they were at `unix`: each between its samples around it, or
    /// at its last one for a little while after it.
    pub fn at(&self, unix: f64) -> Vec<Aircraft> {
        let mut planes: Vec<Aircraft> = self
            .tracks
            .iter()
            .filter_map(|(hex, track)| {
                let samples = &track.samples;
                let after = samples.partition_point(|s| s.unix <= unix);
                let before = samples.get(after.checked_sub(1)?)?;
                let (sample, observed_unix) = match samples.get(after) {
                    Some(next) if next.unix - before.unix <= MAX_GAP_SECS => {
                        (between(before, next, unix), unix)
                    }
                    // Flown on from its last report, as a live plane is.
                    _ if unix - before.unix <= FLY_ON_SECS => (*before, before.unix),
                    _ => return None,
                };
                let reported = |value: f32| (!value.is_nan()).then_some(f64::from(value));
                Some(Aircraft {
                    hex: hex.clone(),
                    callsign: track.callsign.clone(),
                    latitude: f64::from(sample.lat),
                    longitude: f64::from(sample.lon),
                    heading_deg: f64::from(sample.heading),
                    speed_knots: reported(sample.speed_knots),
                    altitude_feet: reported(sample.altitude_feet),
                    vertical_rate_fpm: None,
                    turn_rate_deg_s: None,
                    // Between reports: as seen at that moment, drawn there.
                    observed_unix,
                    details: track.details.clone(),
                    class: track.class,
                })
            })
            .collect();
        // The same order each time, so labels do not jump between replayed frames.
        planes.sort_unstable_by(|a, b| a.hex.cmp(&b.hex));
        planes
    }
}

/// A plane between two samples at `unix`: along the great circle, its heading and
/// height in proportion.
fn between(a: &Sample, b: &Sample, unix: f64) -> Sample {
    let span = b.unix - a.unix;
    let f = if span > 0.0 {
        ((unix - a.unix) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let unit = |s: &Sample| {
        let (lat, lon) = (f64::from(s.lat).to_radians(), f64::from(s.lon).to_radians());
        [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
    };
    let (p, q) = (unit(a), unit(b));
    let dot = (p[0] * q[0] + p[1] * q[1] + p[2] * q[2]).clamp(-1.0, 1.0);
    let angle = dot.acos();
    let (wp, wq) = if angle < 1e-9 {
        (1.0 - f, f)
    } else {
        (
            ((1.0 - f) * angle).sin() / angle.sin(),
            (f * angle).sin() / angle.sin(),
        )
    };
    let v = [
        wp * p[0] + wq * q[0],
        wp * p[1] + wq * q[1],
        wp * p[2] + wq * q[2],
    ];
    let lat = v[2].atan2(v[0].hypot(v[1])).to_degrees();
    let lon = v[1].atan2(v[0]).to_degrees();
    let turn = (f64::from(b.heading) - f64::from(a.heading) + 540.0).rem_euclid(360.0) - 180.0;
    let mix = |x: f32, y: f32| {
        if x.is_nan() || y.is_nan() {
            if f < 0.5 { x } else { y }
        } else {
            x + (y - x) * f as f32
        }
    };
    Sample {
        unix,
        lat: lat as f32,
        lon: lon as f32,
        heading: (f64::from(a.heading) + turn * f).rem_euclid(360.0) as f32,
        altitude_feet: mix(a.altitude_feet, b.altitude_feet),
        speed_knots: mix(a.speed_knots, b.speed_knots),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_795_150_000.0;

    fn plane(hex: &str, lat: f64, lon: f64, heading: f64, observed_unix: f64) -> Aircraft {
        Aircraft {
            hex: hex.into(),
            callsign: format!("CS{hex}").into(),
            latitude: lat,
            longitude: lon,
            heading_deg: heading,
            speed_knots: Some(450.0),
            altitude_feet: Some(36_000.0),
            vertical_rate_fpm: None,
            turn_rate_deg_s: None,
            observed_unix,
            details: None,
            class: AircraftClass::Airliner,
        }
    }

    #[test]
    fn a_plane_is_replayed_between_its_samples() {
        let mut history = AircraftHistory::default();
        history.record(
            &[plane("abc123", 50.0, 8.0, 350.0, NOW - 600.0)],
            NOW - 600.0,
        );
        // Reports closer than the sample step are not kept.
        history.record(
            &[plane("abc123", 50.01, 8.0, 350.0, NOW - 595.0)],
            NOW - 595.0,
        );
        history.record(
            &[plane("abc123", 51.0, 8.0, 10.0, NOW - 300.0)],
            NOW - 300.0,
        );
        assert_eq!(history.samples, 2);
        // Halfway in time: halfway along, heading through north (0°, not 180°).
        let halfway = history.at(NOW - 450.0);
        assert_eq!(halfway.len(), 1);
        let p = &halfway[0];
        assert!((p.latitude - 50.5).abs() < 0.01, "{}", p.latitude);
        assert!((p.longitude - 8.0).abs() < 0.01);
        assert!(
            p.heading_deg < 1.0 || p.heading_deg > 359.0,
            "{}",
            p.heading_deg
        );
        assert_eq!(p.observed_unix, NOW - 450.0);
        assert_eq!(p.altitude_feet, Some(36_000.0));
        // Before its first sample it was not seen.
        assert!(history.at(NOW - 700.0).is_empty());
        // After its last, handed on with that report's time (the map flies it on),
        // as long as a live plane would be; then gone.
        let after = history.at(NOW - 300.0 + FLY_ON_SECS);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].observed_unix, NOW - 300.0);
        assert_eq!(after[0].latitude, 51.0);
        assert!(history.at(NOW - 300.0 + FLY_ON_SECS + 1.0).is_empty());
    }

    #[test]
    fn samples_too_far_apart_are_not_joined() {
        let mut history = AircraftHistory::default();
        history.record(
            &[plane("abc123", 50.0, 8.0, 0.0, NOW - 3000.0)],
            NOW - 3000.0,
        );
        history.record(&[plane("abc123", 40.0, 8.0, 0.0, NOW - 100.0)], NOW - 100.0);
        assert!(history.at(NOW - 1500.0).is_empty());
    }

    #[test]
    fn the_history_covers_the_last_hour_since_recording_began() {
        let mut history = AircraftHistory::default();
        assert!(!history.covers(NOW - 60.0, NOW));
        history.record(
            &[plane("abc123", 50.0, 8.0, 0.0, NOW - 4000.0)],
            NOW - 4000.0,
        );
        assert!(history.covers(NOW - 3000.0, NOW - 2000.0));
        assert!(!history.covers(NOW - 4100.0, NOW - 2000.0));
        assert!(!history.covers(NOW, NOW - 2000.0));
        // Recorded on every ten minutes: an hour later the old samples are forgotten
        // and the history starts an hour back.
        let mut t = NOW - 3600.0;
        while t <= NOW {
            history.record(&[plane("def456", 10.0, 10.0, 0.0, t)], t);
            t += 600.0;
        }
        assert!(history.at(NOW - 3700.0).is_empty());
        assert!(!history.covers(NOW - KEEP_SECS - 1.0, NOW));
        assert!(history.covers(NOW - KEEP_SECS + 1.0, NOW));
    }

    #[test]
    fn a_gap_in_recording_is_not_covered() {
        let mut history = AircraftHistory::default();
        history.record(
            &[plane("abc123", 50.0, 8.0, 0.0, NOW - 3000.0)],
            NOW - 3000.0,
        );
        // The layer off for 45 minutes, then on again.
        history.record(&[plane("abc123", 51.0, 8.0, 0.0, NOW - 300.0)], NOW - 300.0);
        assert!(!history.covers(NOW - 1500.0, NOW));
        assert!(history.covers(NOW - 200.0, NOW));
    }

    #[test]
    fn the_great_circle_is_followed_across_the_date_line() {
        let a = Sample {
            unix: 0.0,
            lat: 0.0,
            lon: 179.0,
            heading: 90.0,
            altitude_feet: f32::NAN,
            speed_knots: 400.0,
        };
        let b = Sample {
            lon: -179.0,
            unix: 100.0,
            ..a
        };
        let mid = between(&a, &b, 50.0);
        assert!(mid.lon.abs() > 179.9, "{}", mid.lon);
        assert!(mid.altitude_feet.is_nan());
    }
}
