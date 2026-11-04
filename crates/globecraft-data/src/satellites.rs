//! CelesTrak OMM element sets propagated with SGP4 (the model the elements are made
//! for) into predicted positions, with their altitude. Element sets older than 14
//! days are not shown; a set SGP4 rejects falls back to a two-body estimate.
mod decode;
use decode::parse_omm;
use globecraft_core::math::now_unix;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

/// The CelesTrak groups shown, by their GP query names. A satellite in several keeps
/// the first.
const GROUPS: [(SatelliteGroup, &str); 4] = [
    (SatelliteGroup::Station, "STATIONS"),
    (SatelliteGroup::Navigation, "GNSS"),
    (SatelliteGroup::Weather, "WEATHER"),
    (SatelliteGroup::Bright, "VISUAL"),
];
const REFRESH: Duration = Duration::from_secs(6 * 3600);
const RETRY: Duration = Duration::from_secs(10 * 60);
const SECONDS_PER_DAY: f64 = 86_400.0;
/// Element sets are shown up to this long from their epoch.
const MAX_EPOCH_AGE_SECS: f64 = 14.0 * SECONDS_PER_DAY;
/// 2000-01-01 12:00 UTC (J2000), in Unix seconds.
const J2000_UNIX_SECS: f64 = 946_728_000.0;
const SECONDS_PER_JULIAN_YEAR: f64 = 365.25 * SECONDS_PER_DAY;
const MEAN_EARTH_RADIUS_KM: f64 = 6_371.0;

/// What a satellite is for, by the CelesTrak group it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatelliteGroup {
    /// Space stations (ISS, Tiangong) and the craft visiting them.
    Station,
    /// GPS, GLONASS, Galileo, BeiDou.
    Navigation,
    Weather,
    /// The brightest satellites, easy to see by eye.
    Bright,
}

impl SatelliteGroup {
    pub const ALL: [Self; 4] = [Self::Station, Self::Navigation, Self::Weather, Self::Bright];
    /// Its name in the interface.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Station => "satellite-group-station",
            Self::Navigation => "satellite-group-navigation",
            Self::Weather => "satellite-group-weather",
            Self::Bright => "satellite-group-bright",
        }
    }
}

/// An element set ready for SGP4.
#[derive(Clone, Debug)]
pub struct Sgp4Orbit {
    constants: sgp4::Constants,
    epoch_unix_secs: f64,
}

impl Sgp4Orbit {
    /// From one CelesTrak OMM JSON record; `None` when SGP4 cannot use it.
    pub fn from_omm(record: &serde_json::Value, epoch_unix_secs: f64) -> Option<Self> {
        let elements: sgp4::Elements = serde_json::from_value(record.clone()).ok()?;
        let constants = sgp4::Constants::from_elements(&elements).ok()?;
        Some(Self {
            constants,
            epoch_unix_secs,
        })
    }

    /// Where it is at `unix`: geocentric latitude and longitude, and the height above
    /// the mean Earth radius.
    fn position(&self, unix: f64) -> Option<SatellitePosition> {
        let minutes = (unix - self.epoch_unix_secs) / 60.0;
        let prediction = self
            .constants
            .propagate(sgp4::MinutesSinceEpoch(minutes))
            .ok()?;
        // TEME (true equator, mean equinox) to Earth-fixed: turn by sidereal time.
        let theta =
            sgp4::iau_epoch_to_sidereal_time((unix - J2000_UNIX_SECS) / SECONDS_PER_JULIAN_YEAR);
        let [x, y, z] = prediction.position;
        let (sin, cos) = theta.sin_cos();
        let (east_x, east_y) = (cos * x + sin * y, -sin * x + cos * y);
        let radius = (east_x * east_x + east_y * east_y + z * z).sqrt();
        let position = SatellitePosition {
            lat: z.atan2(east_x.hypot(east_y)).to_degrees(),
            lon: east_y.atan2(east_x).to_degrees(),
            altitude_km: radius - MEAN_EARTH_RADIUS_KM,
        };
        (position.lat.is_finite() && position.lon.is_finite() && position.altitude_km > 0.0)
            .then_some(position)
    }
}

/// A satellite's place: under it on the ground, and how high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SatellitePosition {
    pub lat: f64,
    pub lon: f64,
    pub altitude_km: f64,
}

#[derive(Clone, Debug)]
pub struct Satellite {
    pub name: String,
    pub norad: String,
    pub group: SatelliteGroup,
    /// The mean elements (for the period, mean altitude and inclination shown).
    pub orbit: globecraft_core::orbit::OrbitalElements,
    /// SGP4, when the element set allows it.
    pub sgp4: Option<Arc<Sgp4Orbit>>,
}
impl Satellite {
    /// Where it is at `unix`, by SGP4 (else a two-body estimate at its mean
    /// altitude); `None` for element sets more than 14 days from `unix`.
    pub fn position(&self, unix: f64) -> Option<SatellitePosition> {
        if (unix - self.orbit.epoch_unix_secs).abs() > MAX_EPOCH_AGE_SECS {
            return None;
        }
        match &self.sgp4 {
            Some(sgp4) => sgp4.position(unix),
            None => {
                let (lat, lon) = self.orbit.ground_position(unix)?;
                Some(SatellitePosition {
                    lat,
                    lon,
                    altitude_km: self.orbit.altitude_km()?,
                })
            }
        }
    }
    pub fn ground_position(&self, unix: f64) -> Option<(f64, f64)> {
        self.position(unix).map(|at| (at.lat, at.lon))
    }
}
#[derive(Default)]
pub struct LiveSatellites {
    satellites: Arc<[Satellite]>,
    loaded: Option<Instant>,
    attempted: Option<Instant>,
    error: Option<crate::Error>,
    rx: Option<mpsc::Receiver<Result<Arc<[Satellite]>, crate::Error>>>,
}
impl LiveSatellites {
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        enabled: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(s)) => {
                    self.satellites = s;
                    self.loaded = Some(Instant::now());
                    self.error = None;
                    self.rx = None;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("CelesTrak disconnected".into());
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if enabled
            && self.rx.is_none()
            && self
                .attempted
                .is_none_or(|t| t.elapsed() >= if self.error.is_some() { RETRY } else { REFRESH })
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
    pub fn snapshot(&self) -> &[Satellite] {
        &self.satellites
    }
    pub fn shared_snapshot(&self) -> Option<Arc<[Satellite]>> {
        (!self.satellites.is_empty()).then(|| self.satellites.clone())
    }
    pub fn status(&self, lang: globecraft_core::language::Lang) -> String {
        if let Some(error) = &self.error {
            return format!("CelesTrak: {error}");
        }
        if self.loaded.is_none() {
            return lang.text("satellites-loading-status").into();
        }
        lang.format(
            "satellites-loaded-status",
            &[("count", self.satellites.len().into())],
        )
    }
}
async fn download() -> Result<Arc<[Satellite]>, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    // All groups at once: one slow answer does not hold up the others.
    let mut jobs = tokio::task::JoinSet::new();
    for (order, (group, name)) in GROUPS.into_iter().enumerate() {
        let client = client.clone();
        jobs.spawn(async move {
            let url =
                format!("https://celestrak.org/NORAD/elements/gp.php?GROUP={name}&FORMAT=JSON");
            let bytes = crate::http::body(client.get(url), 4_000_000).await?;
            let v = crate::http::parse_json(bytes).await?;
            let satellites = crate::cpu::run(crate::cpu::Service::Features, move || {
                Ok(parse_omm(&v, now_unix(), group))
            })
            .await?;
            Ok::<_, crate::Error>((order, satellites))
        });
    }
    let mut groups = Vec::new();
    let mut errors = Vec::new();
    while let Some(done) = jobs.join_next().await {
        match done {
            Ok(Ok(group)) => groups.push(group),
            Ok(Err(e)) => errors.push(e.to_string()),
            Err(e) => errors.push(e.to_string()),
        }
    }
    // In the groups' order, so the dedup below keeps a satellite's first group.
    groups.sort_by_key(|(order, _)| *order);
    let mut all: Vec<Satellite> = groups.into_iter().flat_map(|(_, s)| s).collect();
    crate::cpu::run(crate::cpu::Service::Features, move || {
        // Stable: of a satellite listed in several groups, the first group's stays.
        all.sort_by(|a, b: &Satellite| a.norad.cmp(&b.norad));
        all.dedup_by(|a, b| a.norad == b.norad);
        if all.is_empty() {
            return Err(format!("CelesTrak unavailable: {}", errors.join("; ")).into());
        }
        Ok(all.into())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ISS element set (CelesTrak OMM JSON) propagated by Vallado's SGP4 in Python
    /// (`sgp4` 2.x, WGS 72 constants) and turned Earth-fixed by its sidereal time:
    /// latitude, longitude and height above 6,371 km at 0 s, 30 min, 2 h and 2 days.
    /// The crate uses WGS 84 constants, hence the tolerance.
    #[test]
    fn sgp4_positions_match_the_reference_implementation() {
        let record = serde_json::json!({
            "OBJECT_NAME": "ISS (ZARYA)", "OBJECT_ID": "1998-067A",
            "EPOCH": "2024-01-01T12:00:00.000000", "MEAN_MOTION": 15.50103472,
            "ECCENTRICITY": 0.0003, "INCLINATION": 51.64, "RA_OF_ASC_NODE": 100.0,
            "ARG_OF_PERICENTER": 90.0, "MEAN_ANOMALY": 270.0, "EPHEMERIS_TYPE": 0,
            "CLASSIFICATION_TYPE": "U", "NORAD_CAT_ID": 25544, "ELEMENT_SET_NO": 999,
            "REV_AT_EPOCH": 43000, "BSTAR": 0.0002, "MEAN_MOTION_DOT": 0.0001,
            "MEAN_MOTION_DDOT": 0
        });
        let epoch = 1_704_110_400.0;
        let satellites =
            decode::parse_omm(&serde_json::json!([record]), epoch, SatelliteGroup::Station);
        let iss = &satellites[0];
        assert!(iss.sgp4.is_some(), "SGP4 takes the element set");
        for (seconds, lat, lon, altitude) in [
            (0.0, -0.1045, 179.2718, 424.316),
            (1800.0, 44.6051, -59.6378, 415.627),
            (7200.0, 49.1012, -97.2576, 414.827),
            (172_800.0, 6.3861, 172.5544, 423.264),
        ] {
            let at = iss.position(epoch + seconds).unwrap();
            let dlon = (at.lon - lon + 540.0).rem_euclid(360.0) - 180.0;
            assert!(
                (at.lat - lat).abs() < 0.05
                    && dlon.abs() < 0.05
                    && (at.altitude_km - altitude).abs() < 2.0,
                "{seconds} s: {at:?} instead of ({lat}, {lon}, {altitude})"
            );
        }
        // More than 14 days away, nothing is predicted.
        assert!(iss.position(epoch + 15.0 * SECONDS_PER_DAY).is_none());
    }
}
