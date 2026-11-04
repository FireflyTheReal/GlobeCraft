//! Bounded live telemetry; the desktop only swaps immutable snapshots.
#[cfg(test)]
mod benchmarks;
mod decode;
pub mod route;
mod snapshot;
#[cfg(test)]
mod tests;
mod worker;
use std::{sync::Arc, time::Instant};
use tokio::sync::watch;

const MAX_POSITION_AGE_SECS: f64 = 120.0;

#[derive(Debug, Clone)]
pub struct Aircraft {
    pub hex: Arc<str>,
    pub callsign: Arc<str>,
    pub latitude: f64,
    pub longitude: f64,
    pub heading_deg: f64,
    pub speed_knots: Option<f64>,
    pub altitude_feet: Option<f64>,
    /// Climb (positive) or descent (negative), in feet per minute.
    pub vertical_rate_fpm: Option<f64>,
    /// How fast its track turns, degrees per second (positive: right).
    pub turn_rate_deg_s: Option<f64>,
    pub observed_unix: f64,
    /// What else ADSB.lol reports (OpenSky reports none of it).
    pub details: Option<Arc<AircraftDetails>>,
    /// What kind of aircraft its transponder says it is.
    pub class: AircraftClass,
}

/// What kind of aircraft it is, from the ADS-B emitter category both feeds report
/// (DO-260B §2.2.3.2.5.2: ADSB.lol as `A1`…`C7`, OpenSky as a number). The map draws
/// each with its own symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AircraftClass {
    /// Not reported: drawn as an airliner, as most traffic is.
    #[default]
    Unknown,
    /// Light, under 15,500 lb (A1).
    Light,
    /// Small to large airliners and business jets (A2, A3, A4).
    Airliner,
    /// Heavy, over 300,000 lb (A5).
    Heavy,
    /// High performance: fast and agile, mostly military jets (A6).
    Fast,
    Helicopter,
    Glider,
    /// Balloons and airships.
    Balloon,
    /// Ultralights, hang gliders and paragliders.
    Ultralight,
    Drone,
}

impl AircraftClass {
    /// Every class with a symbol of its own, in the legend's order.
    pub const ALL: [Self; 9] = [
        Self::Airliner,
        Self::Heavy,
        Self::Light,
        Self::Fast,
        Self::Helicopter,
        Self::Glider,
        Self::Balloon,
        Self::Ultralight,
        Self::Drone,
    ];

    /// From ADSB.lol's `category` (`A1`…`C7`).
    pub fn from_adsb(category: &str) -> Self {
        match category.trim().to_ascii_uppercase().as_str() {
            "A1" => Self::Light,
            "A2" | "A3" | "A4" => Self::Airliner,
            "A5" => Self::Heavy,
            "A6" => Self::Fast,
            "A7" => Self::Helicopter,
            "B1" => Self::Glider,
            "B2" => Self::Balloon,
            "B4" => Self::Ultralight,
            "B6" => Self::Drone,
            _ => Self::Unknown,
        }
    }

    /// From OpenSky's `category` (state vector index 17, with `extended=1`).
    pub fn from_opensky(category: i64) -> Self {
        match category {
            2 => Self::Light,
            3..=5 => Self::Airliner,
            6 => Self::Heavy,
            7 => Self::Fast,
            8 => Self::Helicopter,
            9 => Self::Glider,
            10 => Self::Balloon,
            12 => Self::Ultralight,
            14 => Self::Drone,
            _ => Self::Unknown,
        }
    }

    /// The translation key of its name.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Unknown => "aircraft-class-unknown",
            Self::Light => "aircraft-class-light",
            Self::Airliner => "aircraft-class-airliner",
            Self::Heavy => "aircraft-class-heavy",
            Self::Fast => "aircraft-class-fast",
            Self::Helicopter => "aircraft-class-helicopter",
            Self::Glider => "aircraft-class-glider",
            Self::Balloon => "aircraft-class-balloon",
            Self::Ultralight => "aircraft-class-ultralight",
            Self::Drone => "aircraft-class-drone",
        }
    }
}

/// What ADSB.lol reports beyond position and motion. Every field may be missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AircraftDetails {
    /// Registration (`r`), e.g. `LX-VCD`.
    pub registration: Option<Box<str>>,
    /// ICAO type designator (`t`), e.g. `B748`.
    pub type_code: Option<Box<str>>,
    /// Emitter category (`category`), e.g. `A3`.
    pub category: Option<Box<str>>,
    pub squawk: Option<Box<str>>,
    /// The emergency it declares (`emergency`), when not `none`.
    pub emergency: Option<Box<str>>,
    /// Height by satellite (`alt_geom`), feet.
    pub geometric_altitude_feet: Option<f64>,
    pub indicated_airspeed_knots: Option<f64>,
    pub true_airspeed_knots: Option<f64>,
    pub mach: Option<f64>,
    /// Wind it flies in: from (degrees), speed (knots).
    pub wind: Option<(f64, f64)>,
    /// Outside air temperature, °C.
    pub outside_air_celsius: Option<f64>,
    pub true_heading_deg: Option<f64>,
    /// Altitude and heading the autopilot is set to.
    pub selected_altitude_feet: Option<f64>,
    pub selected_heading_deg: Option<f64>,
    /// How the position was found (`type`): `adsb_icao`, `mlat`, `tisb_icao`…
    pub source: Option<Box<str>>,
}

#[derive(Clone, Default)]
pub struct ProviderStatus {
    pub loaded: Option<Instant>,
    pub attempted: bool,
    pub error: Option<Arc<crate::Error>>,
}

#[derive(Default)]
pub struct AircraftSnapshot {
    pub aircraft: Arc<[Aircraft]>,
    pub global: ProviderStatus,
    pub local: ProviderStatus,
    pub error: Option<Arc<crate::Error>>,
    pub valid_until: Option<Instant>,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct AircraftQuery {
    pub enabled: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_km: f64,
    /// How far from (latitude, longitude) the view reaches, km. Wider than one
    /// ADSB.lol circle, more circles around it are asked for.
    pub radius_km: f64,
}

pub struct LiveAircraft {
    control: watch::Sender<AircraftQuery>,
    updates: watch::Receiver<Arc<AircraftSnapshot>>,
    latest: Arc<AircraftSnapshot>,
    stopped: bool,
}
impl LiveAircraft {
    pub fn new(
        runtime: &tokio::runtime::Handle,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::with_endpoints(runtime, repaint, worker::Endpoints::default())
    }

    /// The service with other sources: an OpenSky-style `states/all` URL and an
    /// ADSB.lol-style API base (recorded data, local test servers).
    pub fn with_sources(
        runtime: &tokio::runtime::Handle,
        repaint: impl Fn() + Send + Sync + 'static,
        global: String,
        local: String,
    ) -> Self {
        Self::with_endpoints(runtime, repaint, worker::Endpoints { global, local })
    }

    fn with_endpoints(
        runtime: &tokio::runtime::Handle,
        repaint: impl Fn() + Send + Sync + 'static,
        endpoints: worker::Endpoints,
    ) -> Self {
        let (control, queries) = watch::channel(AircraftQuery::default());
        let latest = Arc::new(AircraftSnapshot::default());
        let (publish, updates) = watch::channel(latest.clone());
        runtime.spawn(worker::run(queries, publish, repaint, endpoints));
        Self {
            control,
            updates,
            latest,
            stopped: false,
        }
    }

    pub fn update(&mut self, query: AircraftQuery) {
        let enabling = query.enabled && !self.control.borrow().enabled;
        match self.updates.has_changed() {
            Ok(true) => {
                let latest = self.updates.borrow_and_update().clone();
                self.latest = latest;
            }
            Ok(false) => {}
            Err(_) if !self.stopped => {
                self.stopped = true;
                self.latest = Arc::new(AircraftSnapshot {
                    aircraft: self.latest.aircraft.clone(),
                    global: self.latest.global.clone(),
                    local: self.latest.local.clone(),
                    error: Some(Arc::new(
                        "Aircraft service stopped before publishing its result".into(),
                    )),
                    valid_until: self.latest.valid_until,
                });
            }
            Err(_) => {}
        }
        if enabling
            && self
                .latest
                .valid_until
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            // Revalidate expired cached sources off-thread before displaying them again.
            // Clear before sending the query so a fast completion cannot be overwritten.
            self.latest = Arc::new(AircraftSnapshot {
                global: self.latest.global.clone(),
                local: self.latest.local.clone(),
                error: self.latest.error.clone(),
                ..Default::default()
            });
        }
        self.control.send_if_modified(|previous| {
            if *previous == query {
                false
            } else {
                *previous = query;
                true
            }
        });
    }

    pub fn snapshot(&self) -> Arc<AircraftSnapshot> {
        self.latest.clone()
    }
}
