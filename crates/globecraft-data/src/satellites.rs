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
/// The Earth's gravitational parameter (WGS 84), km³/s².
const EARTH_GM_KM3_S2: f64 = 398_600.441_8;

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

/// Craft closer than this fly together: docked to a station (a Soyuz, a Progress or a
/// Dragon at the ISS share its orbit to within metres) or in close formation. They
/// are drawn as one (user, 2026-11-08: the ISS and a docked Soyuz overlapped).
pub const TOGETHER_KM: f64 = 5.0;

impl SatellitePosition {
    /// The straight distance to another, km (on a round Earth: plenty for "together").
    pub fn distance_km(&self, other: &Self) -> f64 {
        let point = |at: &Self| {
            let (lat, lon) = (at.lat.to_radians(), at.lon.to_radians());
            let radius = MEAN_EARTH_RADIUS_KM + at.altitude_km;
            [
                radius * lat.cos() * lon.cos(),
                radius * lat.cos() * lon.sin(),
                radius * lat.sin(),
            ]
        };
        let (a, b) = (point(self), point(other));
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }
}

/// Which of `positions` fly together (within [`TOGETHER_KM`]): for each, the index
/// of the one its group is drawn as (the oldest, lowest catalogue number: the station
/// rather than the craft docked to it), itself when alone. `norads` gives each
/// catalogue number.
pub fn together(positions: &[SatellitePosition], norads: &[&str]) -> Vec<usize> {
    let mut leader: Vec<usize> = (0..positions.len()).collect();
    fn root(leader: &mut [usize], mut i: usize) -> usize {
        while leader[i] != i {
            leader[i] = leader[leader[i]];
            i = leader[i];
        }
        i
    }
    // Only neighbours in height can be together: sorted by it, each is compared with
    // those less than TOGETHER_KM higher.
    let mut order: Vec<usize> = (0..positions.len()).collect();
    order.sort_by(|a, b| {
        positions[*a]
            .altitude_km
            .total_cmp(&positions[*b].altitude_km)
    });
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if positions[j].altitude_km - positions[i].altitude_km > TOGETHER_KM {
                break;
            }
            if positions[i].distance_km(&positions[j]) < TOGETHER_KM {
                let (a, b) = (root(&mut leader, i), root(&mut leader, j));
                if a != b {
                    leader[b] = a;
                }
            }
        }
    }
    // Each group drawn as its member with the lowest catalogue number.
    let number = |i: usize| norads[i].parse::<u64>().unwrap_or(u64::MAX);
    let mut drawn_as: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..positions.len() {
        let group = root(&mut leader, i);
        let best = drawn_as.entry(group).or_insert(i);
        if (number(i), i) < (number(*best), *best) {
            *best = i;
        }
    }
    (0..positions.len())
        .map(|i| drawn_as[&root(&mut leader, i)])
        .collect()
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

    /// One revolution around `unix`: `samples + 1` times and places, from half a
    /// period before to half a period after. Earth-fixed, so the path does not close:
    /// the Earth turns under it. Empty without a period.
    pub fn orbit_path(&self, unix: f64, samples: usize) -> Vec<(f64, SatellitePosition)> {
        let Some(period_secs) = self.orbit.period_minutes().map(|minutes| minutes * 60.0) else {
            return Vec::new();
        };
        let samples = samples.max(1);
        (0..=samples)
            .filter_map(|i| {
                let at = unix + period_secs * (i as f64 / samples as f64 - 0.5);
                self.position(at).map(|position| (at, position))
            })
            .collect()
    }

    /// Its speed at a height, km/s (about the Earth's centre, by the vis-viva
    /// equation with the orbit's mean size).
    pub fn speed_km_s(&self, altitude_km: f64) -> Option<f64> {
        let period_secs = self.orbit.period_minutes()? * 60.0;
        let motion_rad_s = std::f64::consts::TAU / period_secs;
        let semi_major_km = (EARTH_GM_KM3_S2 / (motion_rad_s * motion_rad_s)).cbrt();
        let radius_km = MEAN_EARTH_RADIUS_KM + altitude_km;
        let squared = EARTH_GM_KM3_S2 * (2.0 / radius_km - 1.0 / semi_major_km);
        (squared > 0.0).then(|| squared.sqrt())
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
    /// Shows recorded CelesTrak answers (OMM JSON per group, in `GROUPS`' order) as if
    /// just downloaded, for screenshots and offline demonstrations; the next download
    /// is due six hours later. Groups missing from `answers` stay empty.
    pub fn show_recorded(&mut self, answers: &[(SatelliteGroup, serde_json::Value)]) {
        let now = now_unix();
        self.satellites = merged(
            GROUPS
                .iter()
                .filter_map(|(group, _)| answers.iter().find(|(g, _)| g == group))
                .map(|(group, json)| parse_omm(json, now, *group)),
        );
        self.loaded = Some(Instant::now());
        self.attempted = Some(Instant::now());
        self.error = None;
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
    let client = crate::http::client(crate::http::Policy::Satellites)?;
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
    // In the groups' order, so the dedup keeps a satellite's first group.
    groups.sort_by_key(|(order, _)| *order);
    crate::cpu::run(crate::cpu::Service::Features, move || {
        let all = merged(groups.into_iter().map(|(_, s)| s));
        if all.is_empty() {
            return Err(format!("CelesTrak unavailable: {}", errors.join("; ")).into());
        }
        Ok(all)
    })
    .await
}

/// The groups' satellites, each once: of one listed in several groups, the first
/// group's stays (the sort is stable).
fn merged(groups: impl IntoIterator<Item = Vec<Satellite>>) -> Arc<[Satellite]> {
    let mut all: Vec<Satellite> = groups.into_iter().flatten().collect();
    all.sort_by(|a, b: &Satellite| a.norad.cmp(&b.norad));
    all.dedup_by(|a, b| a.norad == b.norad);
    all.into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn docked_craft_are_drawn_as_the_station() {
        let at = |lat: f64, lon: f64, altitude_km: f64| SatellitePosition {
            lat,
            lon,
            altitude_km,
        };
        // The ISS, a Soyuz 300 m away, a Progress 2 km away, and a satellite 50 km off.
        let positions = [
            at(10.0, 20.0, 420.3),
            at(10.0, 20.0, 420.0),
            at(10.0, 20.018, 420.1),
            at(10.45, 20.0, 420.0),
        ];
        let norads = ["63204", "25544", "62030", "40000"];
        assert_eq!(together(&positions, &norads), [1, 1, 1, 3]);
        assert!((positions[0].distance_km(&positions[1]) - 0.3).abs() < 1e-9);
    }

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
        // One revolution (about 93 minutes), centred on now, at its height.
        let path = iss.orbit_path(epoch + 3600.0, 120);
        assert_eq!(path.len(), 121);
        let period = path[120].0 - path[0].0;
        assert!((period / 60.0 - 92.9).abs() < 0.2, "{period}");
        assert!((path[60].0 - (epoch + 3600.0)).abs() < 1e-6);
        assert!(
            path.iter()
                .all(|(_, at)| (400.0..440.0).contains(&at.altitude_km))
        );
        let reached = path.iter().map(|(_, at)| at.lat.abs()).fold(0.0, f64::max);
        assert!(
            (reached - 51.6).abs() < 0.6,
            "up to its inclination: {reached}"
        );
        // About 7.66 km/s at 420 km.
        let speed = iss.speed_km_s(420.0).unwrap();
        assert!((speed - 7.66).abs() < 0.02, "{speed}");
    }
}
