//! One selected flight: its track so far (the trace ADSB.lol's map draws), where it
//! flies from and to (ADSB.lol's route lookup), and its position every few seconds.
//! ADSB.lol data is ODbL.
use super::Aircraft;
use globecraft_core::math::now_unix;
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const TRACE_BASE: &str = "https://globe.adsb.lol/data/traces";
const ROUTESET: &str = "https://api.adsb.lol/api/0/routeset";
const HEX: &str = "https://api.adsb.lol/v2/hex";
/// A day's trace is at most a few MB.
const MAX_TRACE_BYTES: u64 = 16_000_000;
const MAX_SMALL_BYTES: u64 = 2_000_000;
/// How often the selected flight's position, and its recent trace, are asked for.
const POSITION_EVERY: Duration = Duration::from_secs(3);
const TRACE_EVERY: Duration = Duration::from_secs(30);
/// A trace point's flag for the first point of a new leg (readsb).
const NEW_LEG_FLAG: u64 = 2;

/// One point of a track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPoint {
    pub unix: f64,
    pub lat: f64,
    pub lon: f64,
    /// `None` on the ground.
    pub altitude_feet: Option<f64>,
}

/// An airport at one end of a route.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteAirport {
    pub code: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Where a flight goes, by its callsign's published route (not always right:
/// `plausible` is ADSB.lol's check against where the flight is).
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub origin: RouteAirport,
    pub destination: RouteAirport,
    pub plausible: bool,
}

/// The current leg of a readsb trace (`trace_full`/`trace_recent` JSON): from the
/// last point flagged as a new leg on.
pub fn parse_trace(json: &serde_json::Value) -> Result<Vec<TrackPoint>, crate::Error> {
    let start = json
        .get("timestamp")
        .and_then(serde_json::Value::as_f64)
        .ok_or("trace without timestamp")?;
    let points = json
        .get("trace")
        .and_then(serde_json::Value::as_array)
        .ok_or("trace without points")?;
    let mut track = Vec::new();
    for point in points {
        let Some(fields) = point.as_array() else {
            continue;
        };
        let number = |i: usize| fields.get(i).and_then(serde_json::Value::as_f64);
        let (Some(offset), Some(lat), Some(lon)) = (number(0), number(1), number(2)) else {
            continue;
        };
        let flags = fields
            .get(6)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if flags & NEW_LEG_FLAG != 0 {
            track.clear();
        }
        track.push(TrackPoint {
            unix: start + offset,
            lat,
            lon,
            // "ground" for a plane on the ground.
            altitude_feet: number(3),
        });
    }
    Ok(track)
}

/// The route in an ADSB.lol `routeset` answer: an array with one entry per plane
/// asked for (or `null`), each with its `_airports` in flight order (iata, icao,
/// location, name, lat, lon; more than two for a multi-hop flight), `airport_codes`
/// ("EDDF-KJFK") and `plausible`. Codes without airport details are found in
/// `airports`.
pub fn parse_route(
    json: &serde_json::Value,
    airports: Option<&crate::airports::Airports>,
) -> Option<Route> {
    let entry = json.as_array()?.iter().find(|entry| entry.is_object())?;
    let text = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_owned()
    };
    let listed = |airport: &serde_json::Value| -> Option<RouteAirport> {
        let code = [text(airport, "iata"), text(airport, "icao")]
            .into_iter()
            .find(|code| !code.is_empty())?;
        Some(RouteAirport {
            code,
            name: [text(airport, "location"), text(airport, "name")]
                .into_iter()
                .find(|name| !name.is_empty())
                .unwrap_or_default(),
            lat: airport.get("lat")?.as_f64()?,
            lon: airport.get("lon")?.as_f64()?,
        })
    };
    let known = |code: &str| -> Option<RouteAirport> {
        let known = airports?.by_code(code)?;
        Some(RouteAirport {
            code: known.code().to_owned(),
            name: if known.municipality.is_empty() {
                known.name.to_string()
            } else {
                known.municipality.to_string()
            },
            lat: known.lat,
            lon: known.lon,
        })
    };
    let ends = entry
        .get("_airports")
        .and_then(serde_json::Value::as_array)
        .filter(|list| list.len() >= 2)
        .and_then(|list| Some((listed(list.first()?)?, listed(list.last()?)?)))
        .or_else(|| {
            let codes = text(entry, "airport_codes");
            let codes: Vec<&str> = codes
                .split('-')
                .filter(|code| !code.is_empty() && *code != "unknown")
                .collect();
            Some((known(codes.first()?)?, known(codes.last()?)?))
        })?;
    if ends.0.code == ends.1.code {
        return None;
    }
    Some(Route {
        origin: ends.0,
        destination: ends.1,
        plausible: entry
            .get("plausible")
            .map(|p| p.as_bool().unwrap_or_else(|| p.as_u64() != Some(0)))
            .unwrap_or(true),
    })
}

/// A JSON body, gunzipped when the server sends the stored gzip file as it is.
fn json_body(bytes: &[u8]) -> Result<serde_json::Value, crate::Error> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut text)?;
        Ok(serde_json::from_slice(&text)?)
    } else {
        Ok(serde_json::from_slice(bytes)?)
    }
}

async fn trace(hex: &str, kind: &str) -> Result<Vec<TrackPoint>, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    let last = &hex[hex.len().saturating_sub(2)..];
    let url = format!("{TRACE_BASE}/{last}/trace_{kind}_{hex}.json");
    let bytes = crate::http::body(client.get(url), MAX_TRACE_BYTES).await?;
    let json = json_body(&bytes)?;
    crate::cpu::run(crate::cpu::Service::Features, move || parse_trace(&json)).await
}

/// The flight's track: the day's trace, brought up to date by the recent one.
async fn full_track(hex: String) -> Result<Vec<TrackPoint>, crate::Error> {
    let mut track = trace(&hex, "full").await.unwrap_or_default();
    let recent = trace(&hex, "recent").await?;
    merge(&mut track, recent);
    Ok(track)
}

/// Appends the points of `recent` after the end of `track`; a new leg in `recent`
/// (it starts later than the track ends and is flagged) replaces the track.
fn merge(track: &mut Vec<TrackPoint>, recent: Vec<TrackPoint>) {
    let end = track.last().map_or(f64::NEG_INFINITY, |p| p.unix);
    match recent.first() {
        Some(first) if first.unix > end + 30.0 * 60.0 => *track = recent,
        _ => track.extend(recent.into_iter().filter(|p| p.unix > end)),
    }
}

async fn route(
    callsign: String,
    lat: f64,
    lon: f64,
    airports: Option<Arc<crate::airports::Airports>>,
) -> Result<Option<Route>, crate::Error> {
    if callsign.trim().is_empty() {
        return Ok(None);
    }
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    let body =
        serde_json::json!({"planes": [{"callsign": callsign.trim(), "lat": lat, "lng": lon}]});
    let bytes = crate::http::body(client.post(ROUTESET).json(&body), MAX_SMALL_BYTES).await?;
    let json = json_body(&bytes)?;
    Ok(parse_route(&json, airports.as_deref()))
}

async fn position(hex: String) -> Result<Option<Aircraft>, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    let bytes = crate::http::body(client.get(format!("{HEX}/{hex}")), MAX_SMALL_BYTES).await?;
    let json = json_body(&bytes)?;
    Ok(super::decode::parse_adsb_lol(&json, now_unix())
        .into_iter()
        .find(|aircraft| *aircraft.hex == *hex))
}

enum Update {
    Track(Result<Vec<TrackPoint>, crate::Error>),
    Route(Result<Option<Route>, crate::Error>),
    Position(Result<Option<Aircraft>, crate::Error>),
}

/// What is known of the selected flight.
#[derive(Clone, Debug, Default)]
pub struct SelectedFlight {
    pub hex: String,
    pub callsign: String,
    pub track: Arc<[TrackPoint]>,
    pub route: Option<Route>,
    /// Its latest position, every few seconds.
    pub position: Option<Aircraft>,
    /// The route was asked for and the answer came (with or without a route).
    pub route_known: bool,
    pub error: Option<String>,
}

/// The selected flight and its downloads.
#[derive(Default)]
pub struct FlightSelection {
    flight: Option<SelectedFlight>,
    tx: Option<mpsc::Sender<(String, Update)>>,
    rx: Option<mpsc::Receiver<(String, Update)>>,
    position_asked: Option<Instant>,
    trace_asked: Option<Instant>,
    route_asked: bool,
    busy: usize,
}

impl FlightSelection {
    /// Selects the flight with this transponder address (and callsign, for its route).
    pub fn select(&mut self, hex: &str, callsign: &str, at: Option<Aircraft>) {
        let (tx, rx) = mpsc::channel();
        *self = Self {
            flight: Some(SelectedFlight {
                hex: hex.to_ascii_lowercase(),
                callsign: callsign.trim().to_owned(),
                position: at,
                ..SelectedFlight::default()
            }),
            tx: Some(tx),
            rx: Some(rx),
            ..Self::default()
        };
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn selected(&self) -> Option<&SelectedFlight> {
        self.flight.as_ref()
    }

    /// Takes finished downloads and starts the ones due.
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        airports: Option<Arc<crate::airports::Airports>>,
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        let (Some(flight), Some(rx), Some(tx)) = (&mut self.flight, &self.rx, &self.tx) else {
            return;
        };
        while let Ok((hex, update)) = rx.try_recv() {
            self.busy = self.busy.saturating_sub(1);
            if hex != flight.hex {
                continue;
            }
            match update {
                Update::Track(Ok(track)) => {
                    let mut merged = flight.track.to_vec();
                    if merged.is_empty() {
                        merged = track;
                    } else {
                        merge(&mut merged, track);
                    }
                    flight.track = merged.into();
                }
                Update::Route(Ok(route)) => {
                    flight.route = route;
                    flight.route_known = true;
                }
                Update::Position(Ok(Some(aircraft))) => {
                    if flight.callsign.is_empty() {
                        flight.callsign = aircraft.callsign.to_string();
                    }
                    flight.position = Some(aircraft);
                }
                Update::Position(Ok(None)) => {}
                Update::Route(Err(error)) => {
                    flight.route_known = true;
                    flight.error = Some(error.to_string());
                }
                Update::Track(Err(error)) | Update::Position(Err(error)) => {
                    flight.error = Some(error.to_string());
                }
            }
        }
        let now = Instant::now();
        let mut spawn =
            |job: std::pin::Pin<Box<dyn std::future::Future<Output = Update> + Send>>| {
                let (tx, hex, repaint) = (tx.clone(), flight.hex.clone(), repaint.clone());
                self.busy += 1;
                runtime.spawn(async move {
                    let _ = tx.send((hex, job.await));
                    repaint();
                });
            };
        if self
            .trace_asked
            .is_none_or(|t| now.duration_since(t) >= TRACE_EVERY)
        {
            self.trace_asked = Some(now);
            let hex = flight.hex.clone();
            let first = flight.track.is_empty();
            spawn(Box::pin(async move {
                Update::Track(if first {
                    full_track(hex).await
                } else {
                    trace(&hex, "recent").await
                })
            }));
        }
        if self
            .position_asked
            .is_none_or(|t| now.duration_since(t) >= POSITION_EVERY)
        {
            self.position_asked = Some(now);
            let hex = flight.hex.clone();
            spawn(Box::pin(
                async move { Update::Position(position(hex).await) },
            ));
        }
        if !self.route_asked
            && let Some((lat, lon)) = flight
                .position
                .as_ref()
                .map(|p| (p.latitude, p.longitude))
                .or_else(|| flight.track.last().map(|p| (p.lat, p.lon)))
        {
            self.route_asked = true;
            let callsign = flight.callsign.clone();
            spawn(Box::pin(async move {
                Update::Route(route(callsign, lat, lon, airports).await)
            }));
        }
    }

    /// Whether downloads for it are still running.
    pub fn loading(&self) -> bool {
        self.busy > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trace_keeps_its_last_leg() {
        let json = serde_json::json!({
            "icao": "86e79a",
            "timestamp": 1793721901.184,
            "trace": [
                [0.00, 43.75, 76.67, 37000, 523.1, 99.5, 1, 96, null, "adsb_icao", 37150, -64, 277, 0.0],
                [18.79, 43.74, 76.73, 37000, 523.5, 99.7, 0, -96, null, "adsb_icao", 37150, -64, 277, -0.2],
                [9000.0, 35.55, 139.78, "ground", 10.0, 90.0, 2, null, null, "adsb_icao", null, null, null, null],
                [9600.0, 35.60, 139.90, 12000, 300.0, 45.0, 0, 2000, null, "adsb_icao", 12100, 2000, 250, 0.0]
            ]
        });
        let track = parse_trace(&json).unwrap();
        assert_eq!(track.len(), 2, "the earlier leg is dropped");
        assert_eq!(track[0].altitude_feet, None, "on the ground");
        assert_eq!(track[1].altitude_feet, Some(12000.0));
        assert!((track[1].unix - (1793721901.184 + 9600.0)).abs() < 1e-6);
    }

    #[test]
    fn the_recent_trace_continues_the_full_one() {
        let point = |unix: f64| TrackPoint {
            unix,
            lat: 0.0,
            lon: 0.0,
            altitude_feet: Some(1000.0),
        };
        let mut track = vec![point(100.0), point(200.0)];
        merge(&mut track, vec![point(150.0), point(250.0)]);
        assert_eq!(
            track.iter().map(|p| p.unix).collect::<Vec<_>>(),
            [100.0, 200.0, 250.0]
        );
        // A recent trace starting long after the track ends is a new flight.
        merge(&mut track, vec![point(10_000.0)]);
        assert_eq!(track.len(), 1);
    }

    #[test]
    fn a_route_names_its_airports_from_the_answer_or_the_airport_list() {
        let answer = serde_json::json!([null, {
            "callsign": "DLH400",
            "airport_codes": "EDDF-KJFK",
            "_airport_codes_iata": "FRA-JFK",
            "_airports": [
                {"iata": "FRA", "icao": "EDDF", "lat": 50.03, "lon": 8.57, "location": "Frankfurt am Main", "name": "Frankfurt am Main Airport"},
                {"iata": "JFK", "icao": "KJFK", "lat": 40.64, "lon": -73.78, "location": "New York", "name": "John F Kennedy International Airport"}
            ],
            "plausible": true
        }]);
        let route = parse_route(&answer, None).unwrap();
        assert_eq!(route.origin.code, "FRA");
        assert_eq!(route.origin.name, "Frankfurt am Main");
        assert_eq!(route.destination.code, "JFK");
        assert!(route.plausible);
        // Without airports in the answer, the airport list names them.
        let list = crate::airports::Airports::parse(
            "kind\tiata\ticao\tlat\tlon\televation_ft\tcountry\tmunicipality\tname\n\
             L\tMUC\tEDDM\t48.35\t11.79\t1487\tDE\tMunich\tMunich Airport\n\
             L\tLHR\tEGLL\t51.47\t-0.46\t83\tGB\tLondon\tHeathrow\n",
        )
        .unwrap();
        let bare = serde_json::json!([{"airport_codes": "EDDM-EGLL", "plausible": false}]);
        let route = parse_route(&bare, Some(&list)).unwrap();
        assert_eq!(
            (route.origin.code.as_str(), route.destination.code.as_str()),
            ("MUC", "LHR")
        );
        assert!(!route.plausible);
        assert!(
            parse_route(
                &serde_json::json!([{"airport_codes": "unknown"}]),
                Some(&list)
            )
            .is_none()
        );
    }
}
