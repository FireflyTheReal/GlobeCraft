//! Airports by code (roadmap 5, `TRK-02`): OurAirports' large and medium airports and
//! the small ones with an IATA code, published into the `map-data` release by
//! `.github/workflows/airports.yml` (public domain). Found by IATA or ICAO code, for
//! the search box and the airports of a selected flight's route.
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// The file is about 300 KB compressed.
const MAX_BYTES: u64 = 8_000_000;
const RETRY: Duration = Duration::from_secs(10 * 60);

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

/// Every airport, with lookups by code.
#[derive(Debug, Default)]
pub struct Airports {
    airports: Vec<Airport>,
    codes: HashMap<Box<str>, u32>,
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

    /// The published list (`GLOBECRAFT_AIRPORTS_FILE`, the `.tsv.gz` from the
    /// map-data release) reads whole and finds the big airports.
    #[test]
    #[ignore = "needs GLOBECRAFT_AIRPORTS_FILE"]
    fn the_published_list_reads() {
        let path = std::env::var("GLOBECRAFT_AIRPORTS_FILE").expect("GLOBECRAFT_AIRPORTS_FILE");
        let bytes = std::fs::read(path).unwrap();
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .read_to_string(&mut text)
            .unwrap();
        let airports = Airports::parse(&text).unwrap();
        println!("AIRPORTS {}", airports.len());
        assert!(airports.len() > 5000);
        for code in ["FRA", "EDDF", "JFK", "GRU", "SBGR", "NRT", "SYD"] {
            assert!(airports.by_code(code).is_some(), "{code}");
        }
    }
}
