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
    pub tunnel: bool,
    pub segments: Vec<Vec<Pos2>>,
}

pub struct ProjectedAirport {
    pub position: Pos2,
    /// The airport: tile `tile` of the snapshot, airport `index` of that tile.
    pub tile: usize,
    pub index: usize,
    /// Whether it gets a name label (else only a marker).
    pub labelled: bool,
}

#[derive(Default)]
pub struct ProjectedTransport {
    /// Drawn in this order: ferries, service rail, rail, trams, subways, then runways.
    pub lines: Vec<ProjectedTransportLine>,
    pub airports: Vec<ProjectedAirport>,
}

fn shown(kind: LineKind, layers: TransportLayers, altitude_km: f64) -> bool {
    match kind {
        LineKind::Rail => layers.railways,
        LineKind::RailService => layers.railways && altitude_km <= SERVICE_RAIL_MAX_ALTITUDE_KM,
        LineKind::Subway | LineKind::Tram => {
            layers.railways && altitude_km <= URBAN_RAIL_MAX_ALTITUDE_KM
        }
        LineKind::Ferry => layers.ferries,
        LineKind::Runway => layers.airports && altitude_km <= RUNWAY_MAX_ALTITUDE_KM,
        LineKind::Taxiway => layers.airports && altitude_km <= TAXIWAY_MAX_ALTITUDE_KM,
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
    for tile in &snapshot.tiles {
        for line in &tile.lines {
            if !shown(line.kind, layers, view.altitude_km) {
                continue;
            }
            let mut segments: Vec<Vec<Pos2>> = Vec::new();
            let mut run: Vec<Pos2> = Vec::new();
            let mut previous = None;
            for &(lat, lon) in &line.points {
                let current = visibility
                    .as_ref()
                    .is_none_or(|visibility| visibility.visible(lat, lon))
                    .then(|| {
                        view.basis.project_precise(
                            ground_position(&view.basis, ground, lat, lon),
                            view.rect,
                        )
                    })
                    .flatten();
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
                    tunnel: line.tunnel,
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
    let max_importance = if view.altitude_km <= SMALL_AIRPORT_MAX_ALTITUDE_KM {
        4
    } else if view.altitude_km <= REGIONAL_AIRPORT_MAX_ALTITUDE_KM {
        3
    } else {
        1
    };
    let mut seen = HashSet::new();
    let mut found: Vec<(u8, ProjectedAirport)> = Vec::new();
    for (tile_index, tile) in snapshot.tiles.iter().enumerate() {
        for (index, airport) in tile.airports.iter().enumerate() {
            let importance = airport.importance();
            if importance > max_importance {
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
            if visibility
                .as_ref()
                .is_some_and(|visibility| !visibility.visible(airport.lat, airport.lon))
            {
                continue;
            }
            let Some(position) = view.basis.project(
                ground_position(&view.basis, ground, airport.lat, airport.lon),
                view.rect,
            ) else {
                continue;
            };
            if !view.rect.contains(position) {
                continue;
            }
            found.push((
                importance,
                ProjectedAirport {
                    position,
                    tile: tile_index,
                    index,
                    labelled: false,
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
