//! Railways, ferries and airports on screen: lines clipped to the view in f64 (as
//! roads), and the airports to label, most important first and kept apart.
use super::LiveInput;
use crate::overlays::{Ground, TerrainVisibility, ground_position};
use emath::Pos2;
use globecraft_data::transport::{LineKind, TransportSnapshot};
use std::collections::HashSet;

/// Yards and sidings only close up.
const SERVICE_RAIL_MAX_ALTITUDE_KM: f64 = 40.0;
/// Trams and subways below this.
const URBAN_RAIL_MAX_ALTITUDE_KM: f64 = 400.0;
/// Runways and taxiways below this.
const RUNWAY_MAX_ALTITUDE_KM: f64 = 80.0;
const TAXIWAY_MAX_ALTITUDE_KM: f64 = 15.0;
/// Small airfields (private, other) below this; military and regional ones below the
/// next; international and public airports at every shown altitude.
const SMALL_AIRPORT_MAX_ALTITUDE_KM: f64 = 250.0;
const REGIONAL_AIRPORT_MAX_ALTITUDE_KM: f64 = 900.0;
const MAX_AIRPORT_LABELS: usize = 60;
const AIRPORT_LABEL_SEPARATION_POINTS: f32 = 70.0;

/// Which transport layers are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransportLayers {
    pub railways: bool,
    pub ferries: bool,
    pub airports: bool,
}

pub struct ProjectedTransportLine {
    pub kind: LineKind,
    /// The snapshot tile it came from (for its fade).
    pub tile: usize,
    pub tunnel: bool,
    /// The altitude fade of its kind (1 when fully shown).
    pub opacity: f32,
    pub segments: Vec<Vec<Pos2>>,
}

pub struct ProjectedAirport {
    pub position: Pos2,
    /// The airport: tile `tile` of the snapshot, airport `index` of that tile.
    pub tile: usize,
    pub index: usize,
    /// Whether it gets a name label (else only a marker).
    pub labelled: bool,
    /// The altitude fade of its importance (1 when fully shown).
    pub opacity: f32,
}

#[derive(Default)]
pub struct ProjectedTransport {
    /// Drawn in this order: ferries, service rail, rail, trams, subways, then runways.
    pub lines: Vec<ProjectedTransportLine>,
    pub airports: Vec<ProjectedAirport>,
}

/// How visible a kind of line is: 0 when its layer is off or the camera is above its
/// altitude, fading in below that (`super::altitude_fade`).
fn line_opacity(kind: LineKind, layers: TransportLayers, altitude_km: f64) -> f32 {
    let (on, max_altitude_km) = match kind {
        LineKind::Rail => (layers.railways, None),
        LineKind::RailService => (layers.railways, Some(SERVICE_RAIL_MAX_ALTITUDE_KM)),
        LineKind::Subway | LineKind::Tram => (layers.railways, Some(URBAN_RAIL_MAX_ALTITUDE_KM)),
        LineKind::Ferry => (layers.ferries, None),
        LineKind::Runway => (layers.airports, Some(RUNWAY_MAX_ALTITUDE_KM)),
        LineKind::Taxiway => (layers.airports, Some(TAXIWAY_MAX_ALTITUDE_KM)),
    };
    match (on, max_altitude_km) {
        (false, _) => 0.0,
        (true, None) => 1.0,
        (true, Some(max)) => super::altitude_fade(max, altitude_km),
    }
}

/// The highest altitude an airport of this importance shows at (`None`: always).
fn airport_max_altitude_km(importance: u8) -> Option<f64> {
    match importance {
        0..=1 => None,
        2..=3 => Some(REGIONAL_AIRPORT_MAX_ALTITUDE_KM),
        _ => Some(SMALL_AIRPORT_MAX_ALTITUDE_KM),
    }
}

fn order(kind: LineKind) -> u8 {
    match kind {
        LineKind::Ferry => 0,
        LineKind::Taxiway => 1,
        LineKind::Runway => 2,
        LineKind::RailService => 3,
        LineKind::Rail => 4,
        LineKind::Tram => 5,
        LineKind::Subway => 6,
    }
}

pub(super) fn project(input: &LiveInput, ground: Option<&dyn Ground>) -> ProjectedTransport {
    let Some(snapshot) = &input.transport else {
        return ProjectedTransport::default();
    };
    let layers = input.transport_layers;
    let view = input.view;
    let visibility = TerrainVisibility::new(&view.basis, ground);
    let clip = view
        .rect
        .expand(crate::overlays::SEGMENT_CLIP_MARGIN_POINTS);
    let mut lines = Vec::new();
    for (tile_index, tile) in snapshot.tiles.iter().enumerate() {
        for line in &tile.lines {
            let opacity = line_opacity(line.kind, layers, view.altitude_km);
            if opacity <= 0.0 {
                continue;
            }
            let mut segments: Vec<Vec<Pos2>> = Vec::new();
            let mut run: Vec<Pos2> = Vec::new();
            let mut previous = None;
            for &(lat, lon) in &line.points {
                let current = crate::overlays::project_unhidden(
                    &view.basis,
                    view.rect,
                    ground,
                    visibility.as_ref(),
                    lat,
                    lon,
                );
                match previous
                    .zip(current)
                    .and_then(|(a, b)| crate::overlays::clip_segment([a, b], clip))
                {
                    Some([a, b]) if run.last() == Some(&a) => run.push(b),
                    Some([a, b]) => {
                        if run.len() >= 2 {
                            segments.push(std::mem::take(&mut run));
                        }
                        run = vec![a, b];
                    }
                    None if run.len() >= 2 => segments.push(std::mem::take(&mut run)),
                    None => run.clear(),
                }
                previous = current;
            }
            if run.len() >= 2 {
                segments.push(run);
            }
            if !segments.is_empty() {
                lines.push(ProjectedTransportLine {
                    kind: line.kind,
                    tile: tile_index,
                    tunnel: line.tunnel,
                    opacity,
                    segments,
                });
            }
        }
    }
    lines.sort_by_key(|line| (order(line.kind), line.tunnel));
    ProjectedTransport {
        lines,
        airports: if layers.airports {
            airports(input, snapshot, ground)
        } else {
            Vec::new()
        },
    }
}

fn airports(
    input: &LiveInput,
    snapshot: &TransportSnapshot,
    ground: Option<&dyn Ground>,
) -> Vec<ProjectedAirport> {
    let view = input.view;
    let visibility = TerrainVisibility::new(&view.basis, ground);
    let mut seen = HashSet::new();
    let mut found: Vec<(u8, ProjectedAirport)> = Vec::new();
    for (tile_index, tile) in snapshot.tiles.iter().enumerate() {
        for (index, airport) in tile.airports.iter().enumerate() {
            let importance = airport.importance();
            let opacity = airport_max_altitude_km(importance)
                .map_or(1.0, |max| super::altitude_fade(max, view.altitude_km));
            if opacity <= 0.0 {
                continue;
            }
            // A coarser tile shown while a finer one loads may repeat an airport.
            let identity = (
                airport.name.clone(),
                (airport.lat * 100.0).round() as i64,
                (airport.lon * 100.0).round() as i64,
            );
            if !seen.insert(identity) {
                continue;
            }
            let Some(position) = view.basis.project(
                ground_position(&view.basis, ground, airport.lat, airport.lon),
                view.rect,
            ) else {
                continue;
            };
            // On screen first: the line-of-sight test is the expensive part.
            if !view.rect.contains(position)
                || visibility
                    .as_ref()
                    .is_some_and(|visibility| !visibility.visible(airport.lat, airport.lon))
            {
                continue;
            }
            found.push((
                importance,
                ProjectedAirport {
                    position,
                    tile: tile_index,
                    index,
                    labelled: false,
                    opacity,
                },
            ));
        }
    }
    found.sort_by_key(|(importance, _)| *importance);
    let separation = AIRPORT_LABEL_SEPARATION_POINTS * view.map_scale;
    let mut labelled: Vec<Pos2> = Vec::new();
    let mut airports: Vec<ProjectedAirport> =
        found.into_iter().map(|(_, airport)| airport).collect();
    for airport in &mut airports {
        if labelled.len() < MAX_AIRPORT_LABELS
            && labelled
                .iter()
                .all(|other| other.distance(airport.position) >= separation)
        {
            labelled.push(airport.position);
            airport.labelled = true;
        }
    }
    airports
}
