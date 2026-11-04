//! Provider-specific decoding; coordinates share core validation and CPU work is bounded.
use super::{Aircraft, AircraftClass, AircraftDetails, MAX_POSITION_AGE_SECS};
use globecraft_core::math::now_unix;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
const GLOBAL_RESPONSE_LIMIT: u64 = 20 * 1024 * 1024;
const LOCAL_RESPONSE_LIMIT: u64 = 8 * 1024 * 1024;
const ADSB_LOCAL_RADIUS_NM: u32 = 250;
/// Faster turns are bad data: airliners turn at 3°/s at most, fighters at ~25°/s.
const MAX_TURN_RATE_DEG_S: f64 = 30.0;
const FEET_PER_METER: f64 = 3.28084;
const KNOTS_PER_METER_PER_SECOND: f64 = 1.94384449;
/// When ADSB.lol answers "too many requests", every request to it waits at least
/// this long (or as long as it asks).
const ADSB_LOL_MIN_PAUSE: Duration = Duration::from_secs(20);

/// Until when ADSB.lol asked to be left alone, shared by every request to it.
static ADSB_LOL_PAUSED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// An ADSB.lol API answer, or an error without asking while it asked to wait.
pub(super) async fn adsb_lol(
    request: reqwest::RequestBuilder,
    limit: u64,
) -> Result<Vec<u8>, crate::Error> {
    let paused = *ADSB_LOL_PAUSED_UNTIL
        .lock()
        .expect("ADSB.lol pause poisoned");
    if let Some(until) = paused.filter(|until| *until > Instant::now()) {
        let left = until.saturating_duration_since(Instant::now()).as_secs() + 1;
        return Err(
            format!("ADSB.lol asked to wait (too many requests); again in {left} s").into(),
        );
    }
    match crate::http::body_or_retry_after(request, limit + 1).await? {
        crate::http::Fetched::Body(bytes) if bytes.len() as u64 > limit => {
            Err(format!("Aircraft response exceeds {limit} bytes").into())
        }
        crate::http::Fetched::Body(bytes) => Ok(bytes),
        crate::http::Fetched::RetryAfter(wait) => {
            let wait = wait.max(ADSB_LOL_MIN_PAUSE);
            *ADSB_LOL_PAUSED_UNTIL
                .lock()
                .expect("ADSB.lol pause poisoned") = Some(Instant::now() + wait);
            Err(format!(
                "ADSB.lol asked to wait (too many requests); again in {} s",
                wait.as_secs()
            )
            .into())
        }
    }
}

async fn response(request: reqwest::RequestBuilder, limit: u64) -> Result<Vec<u8>, crate::Error> {
    let bytes = crate::http::body(request, limit + 1).await?;
    if bytes.len() as u64 > limit {
        return Err(format!("Aircraft response exceeds {limit} bytes").into());
    }
    Ok(bytes)
}
fn number(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|v| v.as_f64()).filter(|v| v.is_finite())
}
fn safe_heading(v: Option<f64>) -> f64 {
    v.unwrap_or(0.0).rem_euclid(360.0)
}

pub(super) async fn download_global(url: String) -> Result<Arc<[Aircraft]>, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    let bytes = response(client.get(url), GLOBAL_RESPONSE_LIMIT).await?;
    crate::cpu::run(
        crate::cpu::Service::Features,
        move || -> Result<_, crate::Error> {
            let json: serde_json::Value = serde_json::from_slice(&bytes)?;
            if !json.get("states").is_some_and(|states| states.is_array()) {
                return Err("OpenSky response lacks state vectors".into());
            }
            Ok(parse_opensky(&json, now_unix()).into())
        },
    )
    .await
}
pub(super) async fn download_local(
    base_url: String,
    lat: f64,
    lon: f64,
) -> Result<Arc<[Aircraft]>, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    let url = format!("{base_url}/lat/{lat:.4}/lon/{lon:.4}/dist/{ADSB_LOCAL_RADIUS_NM}");
    let bytes = adsb_lol(client.get(url), LOCAL_RESPONSE_LIMIT).await?;
    crate::cpu::run(
        crate::cpu::Service::Features,
        move || -> Result<_, crate::Error> {
            let json: serde_json::Value = serde_json::from_slice(&bytes)?;
            if !json.get("ac").is_some_and(|aircraft| aircraft.is_array()) {
                return Err("ADSB.lol response lacks aircraft".into());
            }
            Ok(parse_adsb_lol(&json, now_unix()).into())
        },
    )
    .await
}

fn parse_opensky(json: &serde_json::Value, now: f64) -> Vec<Aircraft> {
    json.get("states")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let f = v.as_array()?;
            let hex = f.first()?.as_str()?.trim().to_ascii_lowercase();
            if hex.is_empty() || f.get(8)?.as_bool().unwrap_or(false) {
                return None;
            }
            let (lat, lon) = globecraft_core::coordinates::validate_coordinates(
                number(f.get(6))?,
                number(f.get(5))?,
            )
            .ok()?;
            let observed = number(f.get(3)).or_else(|| number(f.get(4)))?;
            if !(0.0..=MAX_POSITION_AGE_SECS).contains(&(now - observed)) {
                return None;
            }
            let callsign = f
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_owned();
            let altitude_feet = number(f.get(7))
                .or_else(|| number(f.get(13)))
                .map(|m| m * FEET_PER_METER);
            let speed_knots = number(f.get(9)).map(|mps| mps * KNOTS_PER_METER_PER_SECOND);
            let vertical_rate_fpm = number(f.get(11)).map(|mps| mps * FEET_PER_METER * 60.0);
            Some(Aircraft {
                hex: hex.into(),
                callsign: callsign.into(),
                latitude: lat,
                longitude: lon,
                heading_deg: safe_heading(number(f.get(10))),
                speed_knots,
                altitude_feet,
                vertical_rate_fpm,
                // OpenSky has no turn rate.
                turn_rate_deg_s: None,
                observed_unix: observed,
                details: None,
                class: f
                    .get(17)
                    .and_then(serde_json::Value::as_i64)
                    .map_or(AircraftClass::Unknown, AircraftClass::from_opensky),
            })
        })
        .collect()
}
/// The aircraft of an ADSB.lol answer (`ac`), in the air with a recent position.
pub(super) fn parse_adsb_lol(json: &serde_json::Value, now: f64) -> Vec<Aircraft> {
    json.get("ac")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let hex = v.get("hex")?.as_str()?.trim().to_ascii_lowercase();
            if hex.is_empty() || v.get("alt_baro").and_then(|v| v.as_str()) == Some("ground") {
                return None;
            }
            let (lat, lon) = globecraft_core::coordinates::validate_coordinates(
                number(v.get("lat"))?,
                number(v.get("lon"))?,
            )
            .ok()?;
            let age = number(v.get("seen_pos"))?;
            if !(0.0..=MAX_POSITION_AGE_SECS).contains(&age) {
                return None;
            }
            Some(Aircraft {
                hex: hex.into(),
                callsign: v
                    .get("flight")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .into(),
                latitude: lat,
                longitude: lon,
                heading_deg: safe_heading(number(v.get("track"))),
                speed_knots: number(v.get("gs")),
                altitude_feet: number(v.get("alt_baro")).or_else(|| number(v.get("alt_geom"))),
                vertical_rate_fpm: number(v.get("baro_rate"))
                    .or_else(|| number(v.get("geom_rate"))),
                turn_rate_deg_s: number(v.get("track_rate"))
                    .filter(|rate| rate.abs() <= MAX_TURN_RATE_DEG_S),
                observed_unix: now - age,
                details: Some(Arc::new(adsb_lol_details(v))),
                class: v
                    .get("category")
                    .and_then(serde_json::Value::as_str)
                    .map_or(AircraftClass::Unknown, AircraftClass::from_adsb),
            })
        })
        .collect()
}

/// Everything else an ADSB.lol aircraft record says that GlobeCraft shows.
fn adsb_lol_details(v: &serde_json::Value) -> AircraftDetails {
    let text = |key: &str| -> Option<Box<str>> {
        v.get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(Into::into)
    };
    let value = |key: &str| number(v.get(key));
    AircraftDetails {
        registration: text("r"),
        type_code: text("t"),
        category: text("category"),
        squawk: text("squawk"),
        emergency: text("emergency").filter(|state| &**state != "none"),
        geometric_altitude_feet: value("alt_geom"),
        indicated_airspeed_knots: value("ias"),
        true_airspeed_knots: value("tas"),
        mach: value("mach"),
        wind: value("wd").zip(value("ws")),
        outside_air_celsius: value("oat"),
        true_heading_deg: value("true_heading"),
        selected_altitude_feet: value("nav_altitude_mcp").or_else(|| value("nav_altitude_fms")),
        selected_heading_deg: value("nav_heading"),
        source: text("type"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_oversized_response_is_rejected_instead_of_accepting_a_truncated_prefix() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
                )
                .unwrap();
        });
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let result = runtime.handle().block_on(async {
            response(
                crate::http::client(crate::http::Policy::Aircraft)
                    .unwrap()
                    .get(format!("http://{address}")),
                8,
            )
            .await
        });
        assert!(result.unwrap_err().to_string().contains("exceeds 8 bytes"));
        server.join().unwrap();
    }
    #[test]
    fn parses_opensky_positions_not_ground_vehicles_or_invalid_locations() {
        let now = 1000.0;
        let data = serde_json::json!({"states": [
            ["e490aa", "AZU123 ", "Brazil", 990, 995, -43.2, -19.7, 10000.0, false, 250.0, 45.0, null, null, null],
            // With `extended=1`, index 17 is the emitter category (8: rotorcraft).
            ["e490ab", "PRHEL", "Brazil", 990, 995, -43.1, -19.6, 300.0, false, 50.0, 90.0, null, null, null, null, false, 0, 8],
            ["x", "GROUND", "Brazil", 990, 995, -43.2, -19.7, 0, true, 0, 0],
            ["z", "OLD", "Brazil", 800, 800, -43.2, -19.7, 1000, false, 100, 0]
        ]});
        let flights = parse_opensky(&data, now);
        assert_eq!(flights.len(), 2);
        assert_eq!(flights[0].callsign.as_ref(), "AZU123");
        assert!((flights[0].altitude_feet.unwrap() - 32808.4).abs() < 0.1);
        assert_eq!(flights[0].class, AircraftClass::Unknown);
        assert_eq!(flights[1].class, AircraftClass::Helicopter);
    }
    #[test]
    fn parses_adsb_position_and_rejects_ground_and_stale_records() {
        let data = serde_json::json!({"ac": [
            {"hex":"ab1234", "flight":"GLO123 ", "lat":-23.4, "lon":-46.4,
             "seen_pos":4.2,"gs":440.0,"track":110.0,"alt_baro":36000},
            {"hex":"ab1235","lat":-23.4,"lon":-46.4,"seen_pos":1,"alt_baro":"ground"},
            {"hex":"ab1236","lat":-23.4,"lon":-46.4,"seen_pos":400}
        ]});
        let flights = parse_adsb_lol(&data, 1000.0);
        assert_eq!(flights.len(), 1);
        assert_eq!(flights[0].callsign.as_ref(), "GLO123");
        assert!((flights[0].observed_unix - 995.8).abs() < 0.01);
    }
    #[test]
    fn adsb_details_are_kept() {
        // A record as ADSB.lol sent it (2026-11-04), shortened.
        let data = serde_json::json!({"ac": [{
            "hex":"4d0104","type":"adsb_icao","flight":"CLX9422 ","r":"LX-VCD","t":"B748",
            "alt_baro":22975,"alt_geom":23425,"gs":454.1,"ias":356,"tas":488,"mach":0.808,
            "wd":110,"ws":39,"oat":-33,"track":82.28,"track_rate":-0.03,"true_heading":84.43,
            "baro_rate":1152,"squawk":"3506","emergency":"none","category":"A5",
            "nav_altitude_mcp":27008,"nav_heading":80.86,"lat":49.980391,"lon":7.887961,
            "seen_pos":0.045
        }]});
        let flight = &parse_adsb_lol(&data, 1000.0)[0];
        assert_eq!(flight.turn_rate_deg_s, Some(-0.03));
        assert_eq!(flight.class, AircraftClass::Heavy, "A5");
        let details = flight.details.as_deref().unwrap();
        assert_eq!(details.registration.as_deref(), Some("LX-VCD"));
        assert_eq!(details.type_code.as_deref(), Some("B748"));
        assert_eq!(details.emergency, None, "\"none\" is no emergency");
        assert_eq!(details.wind, Some((110.0, 39.0)));
        assert_eq!(details.selected_altitude_feet, Some(27008.0));
        assert_eq!(details.source.as_deref(), Some("adsb_icao"));
    }
}
