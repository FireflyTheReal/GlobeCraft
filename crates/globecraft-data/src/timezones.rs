//! Time zones (roadmap 5, `REF-05`): the zones in use now, ocean zones included, from
//! timezone-boundary-builder (OpenStreetMap, ODbL), simplified and published into the
//! `map-data` release by `.github/workflows/timezones.yml`. The zone under a point,
//! its outlines for the map, and the local time there at any moment (the IANA rules
//! of `chrono-tz`, so a date on the timeline gets that day's offset).
use chrono::{Offset, TimeZone, Timelike};
use chrono_tz::OffsetName;
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// The file is about 1 MB compressed.
const MAX_BYTES: u64 = 40_000_000;
const RETRY: Duration = Duration::from_secs(10 * 60);

/// One zone: its IANA name and rules, its polygons (rings of latitude, longitude:
/// the outline, then any holes) and their bounds.
#[derive(Debug)]
pub struct Zone {
    pub tzid: Box<str>,
    tz: Option<chrono_tz::Tz>,
    pub polygons: Vec<Vec<Vec<(f64, f64)>>>,
    /// South, west, north, east.
    bounds: [f64; 4],
}

/// The local time in a zone at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTime {
    /// `14:05`.
    pub time: String,
    /// `UTC+5:30`.
    pub offset: String,
    /// `CET`, when the zone has a lettered name for it.
    pub abbreviation: Option<String>,
}

impl Zone {
    fn contains(&self, lat: f64, lon: f64) -> bool {
        let [south, west, north, east] = self.bounds;
        if lat < south || lat > north || lon < west || lon > east {
            return false;
        }
        self.polygons.iter().any(|rings| {
            // Even-odd over the outline and its holes.
            rings
                .iter()
                .filter(|ring| crossings(ring, lat, lon) % 2 == 1)
                .count()
                % 2
                == 1
        })
    }

    /// South, west, north and east edges, degrees.
    pub fn bounds(&self) -> [f64; 4] {
        self.bounds
    }

    /// The local time here at `unix` (seconds since 1970).
    pub fn local_time(&self, unix: f64) -> Option<LocalTime> {
        let tz = self.tz?;
        let at = tz.timestamp_opt(unix.floor() as i64, 0).single()?;
        let seconds = at.offset().fix().local_minus_utc();
        let (sign, seconds) = if seconds < 0 {
            ('−', -seconds)
        } else {
            ('+', seconds)
        };
        let (hours, minutes) = (seconds / 3600, seconds % 3600 / 60);
        let offset = if seconds == 0 {
            "UTC".to_owned()
        } else if minutes == 0 {
            format!("UTC{sign}{hours}")
        } else {
            format!("UTC{sign}{hours}:{minutes:02}")
        };
        let abbreviation = at
            .offset()
            .abbreviation()
            .filter(|name| name.chars().all(|c| c.is_ascii_alphabetic()))
            .map(str::to_owned);
        Some(LocalTime {
            time: format!("{:02}:{:02}", at.hour(), at.minute()),
            offset,
            abbreviation,
        })
    }
}

/// How often a ray from the point eastward crosses `ring`.
fn crossings(ring: &[(f64, f64)], lat: f64, lon: f64) -> usize {
    let mut count = 0;
    for pair in ring.windows(2) {
        let ((lat_a, lon_a), (lat_b, lon_b)) = (pair[0], pair[1]);
        if (lat_a > lat) != (lat_b > lat) {
            let lon_at = lon_a + (lat - lat_a) / (lat_b - lat_a) * (lon_b - lon_a);
            if lon < lon_at {
                count += 1;
            }
        }
    }
    count
}

/// Every zone.
#[derive(Debug, Default)]
pub struct TimeZones {
    pub zones: Vec<Zone>,
    /// Where the clock changes (latitude, longitude): the borders between zones on
    /// different clocks, without coasts.
    pub lines: Vec<Vec<(f64, f64)>>,
    /// One label per group of zones on the same clock: where, and its offsets
    /// (`UTC+1/+2`).
    pub labels: Vec<(f64, f64, Box<str>)>,
}

impl TimeZones {
    /// Reads GeoJSON features with a `tzid` property and (multi)polygon geometry.
    pub fn parse(json: &serde_json::Value) -> Result<Self, crate::Error> {
        let features = json
            .get("features")
            .and_then(serde_json::Value::as_array)
            .ok_or("time zones without features")?;
        let ring = |value: &serde_json::Value| -> Option<Vec<(f64, f64)>> {
            value
                .as_array()?
                .iter()
                .map(|point| {
                    let point = point.as_array()?;
                    Some((point.get(1)?.as_f64()?, point.first()?.as_f64()?))
                })
                .collect()
        };
        let polygon = |value: &serde_json::Value| -> Option<Vec<Vec<(f64, f64)>>> {
            value.as_array()?.iter().map(ring).collect()
        };
        let mut zones = Vec::new();
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        for feature in features {
            let geometry = &feature["geometry"];
            match feature["properties"]["kind"].as_str() {
                Some("line") => {
                    match geometry["type"].as_str() {
                        Some("LineString") => lines.extend(ring(&geometry["coordinates"])),
                        Some("MultiLineString") => lines.extend(
                            geometry["coordinates"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(ring),
                        ),
                        _ => {}
                    }
                    continue;
                }
                Some("label") => {
                    if let (Some(text), Some(point)) = (
                        feature["properties"]["label"].as_str(),
                        ring(&serde_json::json!([geometry["coordinates"]]))
                            .and_then(|points| points.first().copied()),
                    ) {
                        labels.push((point.0, point.1, text.into()));
                    }
                    continue;
                }
                _ => {}
            }
            let Some(tzid) = feature["properties"]["tzid"].as_str() else {
                continue;
            };
            let coordinates = &geometry["coordinates"];
            let polygons: Vec<Vec<Vec<(f64, f64)>>> = match geometry["type"].as_str() {
                Some("Polygon") => polygon(coordinates).into_iter().collect(),
                Some("MultiPolygon") => coordinates
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(polygon)
                    .collect(),
                _ => continue,
            };
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for &(lat, lon) in polygons.iter().flatten().flatten() {
                bounds = [
                    bounds[0].min(lat),
                    bounds[1].min(lon),
                    bounds[2].max(lat),
                    bounds[3].max(lon),
                ];
            }
            if !bounds[0].is_finite() {
                continue;
            }
            zones.push(Zone {
                tzid: tzid.into(),
                tz: tzid.parse().ok(),
                polygons,
                bounds,
            });
        }
        if zones.is_empty() {
            return Err("the time zone file holds no zones".into());
        }
        Ok(Self {
            zones,
            lines,
            labels,
        })
    }

    /// Reads the published file: gzip-compressed GeoJSON.
    pub fn from_gzip(bytes: &[u8]) -> Result<Self, crate::Error> {
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut text)?;
        Self::parse(&serde_json::from_slice(&text)?)
    }

    /// The zone at a point.
    pub fn zone_at(&self, lat: f64, lon: f64) -> Option<&Zone> {
        let lon = (lon + 180.0).rem_euclid(360.0) - 180.0;
        self.zones.iter().find(|zone| zone.contains(lat, lon))
    }
}

/// The zones, loaded once when the layer is on.
#[derive(Default)]
pub struct LiveTimeZones {
    zones: Option<Arc<TimeZones>>,
    attempted: Option<Instant>,
    error: Option<crate::Error>,
    rx: Option<mpsc::Receiver<Result<TimeZones, crate::Error>>>,
}

impl LiveTimeZones {
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        wanted: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(zones)) => {
                    self.zones = Some(Arc::new(zones));
                    self.error = None;
                    self.rx = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if wanted
            && self.zones.is_none()
            && self.rx.is_none()
            && self.attempted.is_none_or(|t| t.elapsed() >= RETRY)
        {
            let (tx, rx) = mpsc::channel();
            self.rx = Some(rx);
            self.attempted = Some(Instant::now());
            runtime.spawn(async move {
                let _ = tx.send(download().await);
                repaint();
            });
        }
    }

    pub fn zones(&self) -> Option<Arc<TimeZones>> {
        self.zones.clone()
    }

    pub fn status(&self, lang: globecraft_core::language::Lang) -> String {
        if let Some(error) = &self.error {
            return error.to_string();
        }
        match &self.zones {
            None => lang.text("timezones-loading-status").into(),
            Some(zones) => lang.format(
                "timezones-loaded-status",
                &[("count", zones.zones.len().into())],
            ),
        }
    }

    /// Replaces the zones, as a finished download does (tests and screenshots).
    pub fn set(&mut self, zones: TimeZones) {
        self.zones = Some(Arc::new(zones));
        self.error = None;
    }
}

async fn download() -> Result<TimeZones, crate::Error> {
    let url = crate::map_data::file("timezones").await?;
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(client.get(&url), MAX_BYTES).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || {
        TimeZones::from_gzip(&bytes)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TimeZones {
        TimeZones::parse(&serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "properties": {"tzid": "Europe/Berlin"},
                 "geometry": {"type": "Polygon", "coordinates": [
                    [[5.0, 47.0], [15.0, 47.0], [15.0, 55.0], [5.0, 55.0], [5.0, 47.0]],
                    // A hole: another zone inside (made up).
                    [[9.0, 50.0], [10.0, 50.0], [10.0, 51.0], [9.0, 51.0], [9.0, 50.0]]
                 ]}},
                {"type": "Feature", "properties": {"tzid": "Asia/Kolkata"},
                 "geometry": {"type": "MultiPolygon", "coordinates": [
                    [[[68.0, 6.0], [97.0, 6.0], [97.0, 36.0], [68.0, 36.0], [68.0, 6.0]]]
                 ]}},
                {"type": "Feature", "properties": {"kind": "line"},
                 "geometry": {"type": "LineString", "coordinates": [[15.0, 47.0], [15.0, 55.0]]}},
                {"type": "Feature", "properties": {"kind": "label", "label": "UTC+1/+2"},
                 "geometry": {"type": "Point", "coordinates": [10.0, 51.0]}},
                {"type": "Feature", "properties": {"tzid": "Etc/GMT+10"},
                 "geometry": {"type": "Polygon", "coordinates": [
                    [[-157.5, -30.0], [-142.5, -30.0], [-142.5, 30.0], [-157.5, 30.0], [-157.5, -30.0]]
                 ]}}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn a_point_finds_its_zone_and_holes_are_left_out() {
        let zones = sample();
        assert_eq!(zones.zones.len(), 3);
        assert_eq!(zones.lines, vec![vec![(47.0, 15.0), (55.0, 15.0)]]);
        assert_eq!(zones.labels, vec![(51.0, 10.0, "UTC+1/+2".into())]);
        assert_eq!(&*zones.zone_at(52.52, 13.40).unwrap().tzid, "Europe/Berlin");
        assert!(zones.zone_at(50.5, 9.5).is_none(), "inside the hole");
        assert_eq!(&*zones.zone_at(19.07, 72.88).unwrap().tzid, "Asia/Kolkata");
        // Longitudes past 180° wrap.
        assert_eq!(&*zones.zone_at(0.0, 210.0).unwrap().tzid, "Etc/GMT+10");
        assert!(zones.zone_at(-40.0, 0.0).is_none());
    }

    #[test]
    fn local_time_follows_the_date() {
        let zones = sample();
        let berlin = zones.zone_at(52.52, 13.40).unwrap();
        // 2026-11-04 14:15 UTC: winter time, UTC+1.
        let winter = berlin.local_time(1_793_801_700.0).unwrap();
        assert_eq!(winter.time, "15:15");
        assert_eq!(winter.offset, "UTC+1");
        assert_eq!(winter.abbreviation.as_deref(), Some("CET"));
        // 2026-07-01 12:00 UTC: summer time, UTC+2.
        let summer = berlin.local_time(1_782_907_200.0).unwrap();
        assert_eq!(
            (summer.time.as_str(), summer.offset.as_str()),
            ("14:00", "UTC+2")
        );
        let kolkata = zones
            .zone_at(19.07, 72.88)
            .unwrap()
            .local_time(1_793_801_700.0)
            .unwrap();
        assert_eq!(
            (kolkata.time.as_str(), kolkata.offset.as_str()),
            ("19:45", "UTC+5:30")
        );
        let ocean = zones
            .zone_at(0.0, -150.0)
            .unwrap()
            .local_time(1_793_801_700.0)
            .unwrap();
        assert_eq!(ocean.offset, "UTC−10");
    }

    /// The published zones (`GLOBECRAFT_TIMEZONES_FILE`, the `.geojson.gz` from the
    /// map-data release) read whole, and real places find their zone and time.
    #[test]
    #[ignore = "needs GLOBECRAFT_TIMEZONES_FILE"]
    fn the_published_zones_read() {
        let path = std::env::var("GLOBECRAFT_TIMEZONES_FILE").expect("GLOBECRAFT_TIMEZONES_FILE");
        let bytes = std::fs::read(path).unwrap();
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .read_to_end(&mut text)
            .unwrap();
        let zones = TimeZones::parse(&serde_json::from_slice(&text).unwrap()).unwrap();
        println!("ZONES {}", zones.zones.len());
        assert!(zones.zones.len() > 400);
        assert!(
            zones.zones.iter().all(|zone| zone.tz.is_some()),
            "a zone chrono-tz lacks"
        );
        // 2026-11-04 14:15 UTC.
        let at = 1_793_801_700.0;
        for (lat, lon, zone, time) in [
            (52.52, 13.40, "Europe/Berlin", "15:15"),
            (40.71, -74.01, "America/New_York", "09:15"),
            (19.07, 72.88, "Asia/Kolkata", "19:45"),
            (-23.55, -46.63, "America/Sao_Paulo", "11:15"),
            (0.0, -150.0, "Etc/GMT+10", "04:15"),
        ] {
            let found = zones.zone_at(lat, lon).unwrap_or_else(|| panic!("{zone}"));
            assert_eq!(&*found.tzid, zone);
            assert_eq!(found.local_time(at).unwrap().time, time, "{zone}");
        }
    }
}
