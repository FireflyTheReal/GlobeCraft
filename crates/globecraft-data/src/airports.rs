//! Airports by code (roadmap 5, `TRK-02`): OurAirports' large and medium airports and
//! the small ones with an IATA code, published into the `map-data` release by
//! `.github/workflows/airports.yml` (public domain). Found by IATA or ICAO code, and
//! near a point, for the search box and the guesses of the flight hover card.
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// The file is about 300 KB compressed.
const MAX_BYTES: u64 = 8_000_000;
const RETRY: Duration = Duration::from_secs(10 * 60);
const EARTH_RADIUS_KM: f64 = 6371.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AirportKind {
    Large,
    Medium,
    Small,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Airport {
    pub kind: AirportKind,
    pub iata: Option<Box<str>>,
    pub icao: Option<Box<str>>,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: Option<f64>,
    pub country: Box<str>,
    pub municipality: Box<str>,
    pub name: Box<str>,
}

impl Airport {
    /// The code people know it by: IATA, else ICAO.
    pub fn code(&self) -> &str {
        self.iata.as_deref().or(self.icao.as_deref()).unwrap_or("")
    }
}

/// Every airport, with lookups by code and by 1° cell.
#[derive(Debug, Default)]
pub struct Airports {
    airports: Vec<Airport>,
    codes: HashMap<Box<str>, u32>,
    cells: HashMap<(i16, i16), Vec<u32>>,
}

fn cell(lat: f64, lon: f64) -> (i16, i16) {
    (lat.floor() as i16, lon.rem_euclid(360.0).floor() as i16)
}

/// Great-circle distance (km) and initial bearing (degrees) from one point to another.
fn distance_bearing(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let (la1, lo1) = (from.0.to_radians(), from.1.to_radians());
    let (la2, lo2) = (to.0.to_radians(), to.1.to_radians());
    let dlo = lo2 - lo1;
    let a = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    let km = 2.0 * EARTH_RADIUS_KM * a.sqrt().min(1.0).asin();
    let y = dlo.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlo.cos();
    (km, y.atan2(x).to_degrees().rem_euclid(360.0))
}

impl Airports {
    /// Reads the published table (tab-separated, with a header).
    pub fn parse(text: &str) -> Result<Self, crate::Error> {
        let mut lines = text.lines();
        let header: Vec<&str> = lines
            .next()
            .ok_or("empty airport list")?
            .split('\t')
            .collect();
        let column = |name: &str| {
            header
                .iter()
                .position(|h| *h == name)
                .ok_or_else(|| crate::Error::from(format!("airport list without {name}")))
        };
        let [
            kind,
            iata,
            icao,
            lat,
            lon,
            elevation,
            country,
            municipality,
            name,
        ] = [
            "kind",
            "iata",
            "icao",
            "lat",
            "lon",
            "elevation_ft",
            "country",
            "municipality",
            "name",
        ]
        .map(column);
        let (kind, iata, icao, lat, lon) = (kind?, iata?, icao?, lat?, lon?);
        let (elevation, country, municipality, name) = (elevation?, country?, municipality?, name?);
        let mut airports = Self::default();
        for line in lines {
            let fields: Vec<&str> = line.split('\t').collect();
            let field = |i: usize| fields.get(i).copied().unwrap_or("").trim();
            let kind = match field(kind) {
                "L" => AirportKind::Large,
                "M" => AirportKind::Medium,
                "S" => AirportKind::Small,
                _ => continue,
            };
            let (Ok(lat), Ok(lon)) = (field(lat).parse::<f64>(), field(lon).parse::<f64>()) else {
                continue;
            };
            if !(lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0) {
                continue;
            }
            let code = |text: &str| (!text.is_empty()).then(|| text.to_ascii_uppercase().into());
            airports.push(Airport {
                kind,
                iata: code(field(iata)),
                icao: code(field(icao)),
                lat,
                lon,
                elevation_ft: field(elevation).parse().ok(),
                country: field(country).into(),
                municipality: field(municipality).into(),
                name: field(name).into(),
            });
        }
        if airports.airports.is_empty() {
            return Err("the airport list holds no airports".into());
        }
        Ok(airports)
    }

    fn push(&mut self, airport: Airport) {
        let index = self.airports.len() as u32;
        // A code names the bigger airport when two share it.
        for code in [&airport.iata, &airport.icao].into_iter().flatten() {
            self.codes.entry(code.clone()).or_insert(index);
        }
        self.cells
            .entry(cell(airport.lat, airport.lon))
            .or_default()
            .push(index);
        self.airports.push(airport);
    }

    pub fn len(&self) -> usize {
        self.airports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.airports.is_empty()
    }

    /// The airport with this IATA (3 letters) or ICAO (4) code, in any case.
    pub fn by_code(&self, code: &str) -> Option<&Airport> {
        let code = code.trim();
        if !(3..=4).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        self.codes
            .get(code.to_ascii_uppercase().as_str())
            .map(|&i| &self.airports[i as usize])
    }

    /// Airports within `radius_km` of a point, at least as big as `smallest`, with
    /// their distance (km) and bearing from the point (degrees), nearest first.
    pub fn around(
        &self,
        lat: f64,
        lon: f64,
        radius_km: f64,
        smallest: AirportKind,
    ) -> Vec<(&Airport, f64, f64)> {
        let lat_cells = (radius_km / 111.0).ceil() as i16 + 1;
        let lon_cells =
            ((radius_km / (111.0 * lat.to_radians().cos().max(0.05))).ceil() as i16 + 1).min(180);
        let (row, col) = cell(lat, lon);
        let mut found = Vec::new();
        for dr in -lat_cells..=lat_cells {
            for dc in -lon_cells..=lon_cells {
                let key = (row + dr, (col + dc).rem_euclid(360));
                for &i in self.cells.get(&key).into_iter().flatten() {
                    let airport = &self.airports[i as usize];
                    if airport.kind > smallest {
                        continue;
                    }
                    let (km, bearing) = distance_bearing((lat, lon), (airport.lat, airport.lon));
                    if km <= radius_km {
                        found.push((airport, km, bearing));
                    }
                }
            }
        }
        found.sort_by(|a, b| a.1.total_cmp(&b.1));
        found
    }
}

/// What a flight near the ground is doing, by its height, climb and the airports around.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlightGuess<'a> {
    Departing(&'a Airport),
    Arriving(&'a Airport),
    Near(&'a Airport),
    /// Descending toward an airport ahead.
    ProbablyArriving(&'a Airport),
}

/// Below this (feet) a flight counts as at an airport it is close to.
const NEAR_AIRPORT_FEET: f64 = 8_000.0;
const NEAR_AIRPORT_KM: f64 = 35.0;
/// A descending flight below this (feet) is heading for a large or medium airport
/// within this distance and angle of its heading.
const APPROACH_FEET: f64 = 20_000.0;
const APPROACH_KM: f64 = 180.0;
const APPROACH_DEGREES: f64 = 20.0;
/// Climbs and descents slower than this (feet per minute) are level flight.
const LEVEL_FPM: f64 = 300.0;

impl Airports {
    /// A guess of where a flight comes from or goes, from its position, heading,
    /// height (feet) and vertical rate (feet per minute); `None` at cruise.
    pub fn guess(
        &self,
        lat: f64,
        lon: f64,
        heading_deg: f64,
        altitude_feet: Option<f64>,
        vertical_rate_fpm: Option<f64>,
    ) -> Option<FlightGuess<'_>> {
        let altitude = altitude_feet?;
        let rate = vertical_rate_fpm.unwrap_or(0.0);
        if altitude <= NEAR_AIRPORT_FEET {
            // The nearest of the bigger airports, else of any.
            let near = self
                .around(lat, lon, NEAR_AIRPORT_KM, AirportKind::Medium)
                .into_iter()
                .next()
                .or_else(|| {
                    self.around(lat, lon, NEAR_AIRPORT_KM / 2.0, AirportKind::Small)
                        .into_iter()
                        .next()
                });
            if let Some((airport, _, _)) = near {
                return Some(if rate > LEVEL_FPM {
                    FlightGuess::Departing(airport)
                } else if rate < -LEVEL_FPM {
                    FlightGuess::Arriving(airport)
                } else {
                    FlightGuess::Near(airport)
                });
            }
        }
        if altitude <= APPROACH_FEET && rate < -LEVEL_FPM {
            let ahead = self
                .around(lat, lon, APPROACH_KM, AirportKind::Medium)
                .into_iter()
                .filter(|(_, _, bearing)| {
                    let off = (bearing - heading_deg + 540.0).rem_euclid(360.0) - 180.0;
                    off.abs() <= APPROACH_DEGREES
                })
                // Large airports first among those ahead, then the nearest.
                .min_by(|a, b| a.0.kind.cmp(&b.0.kind).then(a.1.total_cmp(&b.1)));
            if let Some((airport, _, _)) = ahead {
                return Some(FlightGuess::ProbablyArriving(airport));
            }
        }
        None
    }
}

/// The airport list, loaded once when something asks for it.
#[derive(Default)]
pub struct LiveAirports {
    airports: Option<Arc<Airports>>,
    attempted: Option<Instant>,
    error: Option<crate::Error>,
    rx: Option<mpsc::Receiver<Result<Airports, crate::Error>>>,
}

impl LiveAirports {
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        wanted: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(airports)) => {
                    self.airports = Some(Arc::new(airports));
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
            && self.airports.is_none()
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

    pub fn airports(&self) -> Option<Arc<Airports>> {
        self.airports.clone()
    }

    pub fn error(&self) -> Option<String> {
        self.error.as_ref().map(ToString::to_string)
    }

    /// Replaces the list, as a finished download does (tests and screenshots).
    pub fn set(&mut self, airports: Airports) {
        self.airports = Some(Arc::new(airports));
        self.error = None;
    }
}

async fn download() -> Result<Airports, crate::Error> {
    let url = crate::map_data::file("airports").await?;
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(client.get(&url), MAX_BYTES).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || {
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes.as_slice()).read_to_string(&mut text)?;
        Airports::parse(&text)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "kind\tiata\ticao\tlat\tlon\televation_ft\tcountry\tmunicipality\tname\n\
        L\tFRA\tEDDF\t50.03625\t8.55929\t364\tDE\tFrankfurt am Main\tFrankfurt am Main Airport\n\
        M\t\tEDFE\t49.96000\t8.64583\t384\tDE\tEgelsbach\tFrankfurt-Egelsbach Airport\n\
        L\tMUC\tEDDM\t48.35378\t11.78609\t1487\tDE\tMunich\tMunich Airport\n\
        L\tJFK\tKJFK\t40.63945\t-73.77932\t13\tUS\tNew York\tJohn F Kennedy International Airport\n\
        X\tBAD\t\t0\t0\t\t\t\tnot a kind\n";

    #[test]
    fn codes_find_their_airport_in_any_case() {
        let airports = Airports::parse(SAMPLE).unwrap();
        assert_eq!(airports.len(), 4);
        assert_eq!(
            airports.by_code("fra").unwrap().name.as_ref(),
            "Frankfurt am Main Airport"
        );
        assert_eq!(airports.by_code("EDDM").unwrap().code(), "MUC");
        assert_eq!(airports.by_code("EDFE").unwrap().code(), "EDFE");
        assert!(airports.by_code("BAD").is_none());
        assert!(airports.by_code("Frankfurt").is_none());
    }

    #[test]
    fn flights_near_the_ground_are_placed_at_an_airport() {
        let airports = Airports::parse(SAMPLE).unwrap();
        let code = |guess: Option<FlightGuess>| match guess {
            Some(
                FlightGuess::Departing(a)
                | FlightGuess::Arriving(a)
                | FlightGuess::Near(a)
                | FlightGuess::ProbablyArriving(a),
            ) => a.code().to_owned(),
            None => "-".into(),
        };
        // Climbing out of Frankfurt.
        let guess = airports.guess(50.05, 8.45, 250.0, Some(3_000.0), Some(2_000.0));
        assert!(matches!(guess, Some(FlightGuess::Departing(_))));
        assert_eq!(code(guess), "FRA");
        // Descending over Augsburg, heading east toward Munich, 60 km ahead.
        let guess = airports.guess(48.37, 10.90, 90.0, Some(12_000.0), Some(-1_200.0));
        assert!(matches!(guess, Some(FlightGuess::ProbablyArriving(_))));
        assert_eq!(code(guess), "MUC");
        // The same flight heading west is going elsewhere.
        assert_eq!(
            code(airports.guess(48.37, 10.90, 270.0, Some(12_000.0), Some(-1_200.0))),
            "-"
        );
        // At cruise, no guess.
        assert_eq!(
            code(airports.guess(50.05, 8.45, 90.0, Some(36_000.0), Some(0.0))),
            "-"
        );
        // Across the date line the cells wrap.
        let around = airports.around(40.64, -73.78 + 360.0, 5.0, AirportKind::Small);
        assert_eq!(around.len(), 1);
    }
}
