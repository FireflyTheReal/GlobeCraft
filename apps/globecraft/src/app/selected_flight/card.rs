//! The words of the selected flight's card: its aircraft, route, what ADSB.lol
//! reports about it and how far it has come.
use globecraft_core::{language::Lang, units::DistanceUnits};
use globecraft_data::aircraft::{
    Aircraft,
    route::{RouteAirport, SelectedFlight},
};

/// Climbs and descents slower than this read as level flight, ft/min.
const LEVEL_RATE_FPM: f64 = 50.0;
const KM_PER_MILE: f64 = 1.609_344;

pub(super) struct CardLines {
    pub title: String,
    pub route: String,
    pub details: Vec<String>,
    /// A declared emergency, shown in red.
    pub warning: Option<String>,
}

fn duration_text(secs: f64) -> String {
    let minutes = (secs / 60.0).round().max(0.0) as u64;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    }
}

/// How a position was found, from ADSB.lol's `type`.
fn source_text(source: &str) -> &str {
    match source {
        "adsb_icao" | "adsb_icao_nt" | "adsb_other" => "ADS-B",
        "adsr_icao" | "adsr_other" => "ADS-R",
        "tisb_icao" | "tisb_trackfile" | "tisb_other" => "TIS-B",
        "mlat" => "MLAT",
        "mode_s" => "Mode S",
        other => other,
    }
}

/// Lines of whole messages, each joined with " · ".
fn join(parts: Vec<String>) -> Option<String> {
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The aircraft: its type in words (else its type code), registration, operator
/// and year built.
fn aircraft_line(flight: &SelectedFlight, lang: Lang) -> Option<String> {
    let details = flight.position.as_ref().and_then(|p| p.details.as_deref());
    let mut parts: Vec<String> = Vec::new();
    match (
        &flight.aircraft.description,
        details.and_then(|d| d.type_code.as_deref()),
    ) {
        (Some(description), Some(code)) => parts.push(format!("{description} ({code})")),
        (Some(description), None) => parts.push(description.clone()),
        (None, Some(code)) => parts.push(code.to_owned()),
        (None, None) => {}
    }
    parts.extend(
        details
            .and_then(|d| d.registration.as_deref())
            .map(str::to_owned),
    );
    parts.extend(flight.aircraft.operator.clone());
    if let Some(year) = &flight.aircraft.year {
        parts.push(lang.format("flight-card-built", &[("year", year.clone().into())]));
    }
    join(parts)
}

/// What ADSB.lol reports of its flight: heights, speeds, heading and weather, and
/// its transponder.
fn reported_lines(aircraft: &Aircraft, lang: Lang) -> (Vec<String>, Option<String>) {
    let details = aircraft.details.as_deref();
    let message = |id: &str, name: &str, value: String| lang.format(id, &[(name, value.into())]);
    let mut lines = Vec::new();

    let mut heights = Vec::new();
    if let Some(feet) = aircraft.altitude_feet {
        heights.push(message(
            "flight-card-altitude",
            "feet",
            format!("{feet:.0}"),
        ));
    }
    if let Some(feet) = details.and_then(|d| d.geometric_altitude_feet) {
        heights.push(message(
            "flight-card-gps-altitude",
            "feet",
            format!("{feet:.0}"),
        ));
    }
    if let Some(rate) = aircraft.vertical_rate_fpm {
        heights.push(if rate.abs() < LEVEL_RATE_FPM {
            lang.text("flight-card-level").into()
        } else {
            message("flight-card-vertical-rate", "rate", format!("{rate:+.0}"))
        });
    }
    if let Some(feet) = details.and_then(|d| d.selected_altitude_feet) {
        heights.push(message(
            "flight-card-selected-altitude",
            "feet",
            format!("{feet:.0}"),
        ));
    }
    lines.extend(join(heights));

    let mut speeds = Vec::new();
    if let Some(knots) = aircraft.speed_knots {
        speeds.push(message(
            "flight-card-ground-speed",
            "knots",
            format!("{knots:.0}"),
        ));
    }
    if let Some(knots) = details.and_then(|d| d.indicated_airspeed_knots) {
        speeds.push(message(
            "flight-card-indicated-airspeed",
            "knots",
            format!("{knots:.0}"),
        ));
    }
    if let Some(knots) = details.and_then(|d| d.true_airspeed_knots) {
        speeds.push(message(
            "flight-card-true-airspeed",
            "knots",
            format!("{knots:.0}"),
        ));
    }
    if let Some(mach) = details.and_then(|d| d.mach) {
        speeds.push(message("flight-card-mach", "mach", format!("{mach:.2}")));
    }
    lines.extend(join(speeds));

    let mut heading = vec![message(
        "flight-card-track",
        "degrees",
        format!("{:.0}", aircraft.heading_deg),
    )];
    if let Some(degrees) = details.and_then(|d| d.selected_heading_deg) {
        heading.push(message(
            "flight-card-selected-heading",
            "degrees",
            format!("{degrees:.0}"),
        ));
    }
    if let Some((from, knots)) = details.and_then(|d| d.wind) {
        heading.push(lang.format(
            "flight-card-wind",
            &[
                ("degrees", format!("{from:.0}").into()),
                ("knots", format!("{knots:.0}").into()),
            ],
        ));
    }
    if let Some(celsius) = details.and_then(|d| d.outside_air_celsius) {
        heading.push(message(
            "flight-card-temperature",
            "celsius",
            format!("{celsius:.0}"),
        ));
    }
    lines.extend(join(heading));

    let mut transponder = Vec::new();
    if let Some(squawk) = details.and_then(|d| d.squawk.as_deref()) {
        transponder.push(message("flight-card-squawk", "code", squawk.to_owned()));
    }
    if let Some(category) = details.and_then(|d| d.category.as_deref()) {
        transponder.push(message(
            "flight-card-category",
            "category",
            category.to_owned(),
        ));
    }
    if let Some(source) = details.and_then(|d| d.source.as_deref()) {
        transponder.push(message(
            "flight-card-position-source",
            "source",
            source_text(source).to_owned(),
        ));
    }
    lines.extend(join(transponder));

    let warning = details
        .and_then(|d| d.emergency.as_deref())
        .map(|kind| message("flight-card-emergency", "kind", kind.to_owned()));
    (lines, warning)
}

pub(super) fn lines(
    flight: &SelectedFlight,
    lang: Lang,
    units: DistanceUnits,
    loading: bool,
) -> CardLines {
    let title = if flight.callsign.is_empty() {
        flight.hex.to_uppercase()
    } else {
        format!("{} · {}", flight.callsign, flight.hex.to_uppercase())
    };
    let place = |airport: &RouteAirport| {
        if airport.name.is_empty() {
            airport.code.clone()
        } else {
            format!("{} ({})", airport.name, airport.code)
        }
    };
    let route = match &flight.route {
        Some(route) => lang.format(
            "flight-card-route",
            &[
                ("from", place(&route.origin).into()),
                ("to", place(&route.destination).into()),
            ],
        ),
        None if !flight.route_known => lang.text("flight-card-route-loading").into(),
        None => lang.text("flight-card-route-unknown").into(),
    };
    let mut details = Vec::new();
    if flight.route.as_ref().is_some_and(|route| !route.plausible) {
        details.push(lang.text("flight-card-route-unsure").into());
    }
    details.extend(aircraft_line(flight, lang));
    let mut warning = None;
    if let Some(aircraft) = &flight.position {
        let (reported, emergency) = reported_lines(aircraft, lang);
        details.extend(reported);
        warning = emergency;
    }
    let mut progress = Vec::new();
    if let (Some(first), Some(last)) = (flight.track.first(), flight.track.last()) {
        let end = flight
            .position
            .as_ref()
            .map_or(last.unix, |p| p.observed_unix.max(last.unix));
        progress.push(lang.format(
            "flight-card-flown",
            &[("time", duration_text(end - first.unix).into())],
        ));
    }
    let at = flight
        .position
        .as_ref()
        .map(|p| (p.latitude, p.longitude))
        .or_else(|| flight.track.last().map(|p| (p.lat, p.lon)));
    if let (Some(route), Some(at)) = (&flight.route, at) {
        let km =
            globecraft_core::math::haversine_km(at, (route.destination.lat, route.destination.lon));
        let distance = match units {
            DistanceUnits::Metric => format!("{km:.0} km"),
            DistanceUnits::Imperial => format!("{:.0} mi", km / KM_PER_MILE),
        };
        progress.push(lang.format("flight-card-to-go", &[("distance", distance.into())]));
    }
    details.extend(join(progress));
    if flight.track.is_empty() && loading {
        details.push(lang.text("flight-card-track-loading").into());
    } else if flight.track.is_empty()
        && let Some(error) = &flight.error
    {
        details.push(lang.format("flight-card-error", &[("error", error.clone().into())]));
    }
    CardLines {
        title,
        route,
        details,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_data::aircraft::{AircraftDetails, route::AircraftInfo};
    use std::sync::Arc;

    #[test]
    fn a_flight_time_reads_in_hours_and_minutes() {
        assert_eq!(duration_text(40.0 * 60.0), "40 min");
        assert_eq!(duration_text(125.0 * 60.0), "2 h 05 min");
    }

    #[test]
    fn the_card_names_the_aircraft_and_what_it_reports() {
        let flight = SelectedFlight {
            hex: "4d0104".into(),
            callsign: "CLX9422".into(),
            aircraft: AircraftInfo {
                description: Some("BOEING 747-8".into()),
                operator: Some("Cargolux".into()),
                year: Some("2015".into()),
            },
            position: Some(Aircraft {
                hex: "4d0104".into(),
                callsign: "CLX9422".into(),
                latitude: 49.98,
                longitude: 7.89,
                heading_deg: 82.3,
                speed_knots: Some(454.1),
                altitude_feet: Some(22975.0),
                vertical_rate_fpm: Some(1152.0),
                turn_rate_deg_s: Some(-0.03),
                observed_unix: 1000.0,
                details: Some(Arc::new(AircraftDetails {
                    registration: Some("LX-VCD".into()),
                    type_code: Some("B748".into()),
                    squawk: Some("7700".into()),
                    emergency: Some("general".into()),
                    mach: Some(0.808),
                    wind: Some((110.0, 39.0)),
                    source: Some("adsb_icao".into()),
                    ..AircraftDetails::default()
                })),
                class: globecraft_data::aircraft::AircraftClass::Heavy,
            }),
            route_known: true,
            ..SelectedFlight::default()
        };
        let card = lines(&flight, Lang::En, DistanceUnits::Metric, false);
        let text = card.details.join("\n");
        assert!(
            text.contains("BOEING 747-8 (B748) · LX-VCD · Cargolux"),
            "{text}"
        );
        assert!(text.contains("2015"), "{text}");
        assert!(text.contains("22975"), "{text}");
        assert!(text.contains("0.81"), "{text}");
        assert!(text.contains("110"), "{text}");
        assert!(text.contains("7700"), "{text}");
        assert!(text.contains("ADS-B"), "{text}");
        assert!(
            card.warning
                .is_some_and(|warning| warning.contains("general"))
        );
    }
}
