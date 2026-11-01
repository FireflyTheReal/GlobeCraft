//! Places of interest, peaks, waters and parks on screen: rivers, canals and park
//! outlines clipped to the view in f64, and the names to show, most important first
//! and kept apart.
use super::LiveInput;
use crate::overlays::{Ground, TerrainVisibility, ground_position};
use emath::Pos2;
use globecraft_data::poi::{PoiLineKind, PoiSnapshot, PointKind};
use std::collections::HashSet;

/// Below these altitudes (km) each kind shows.
const POI_MAX_ALTITUDE_KM: f64 = 15.0;
const PEAK_MAX_ALTITUDE_KM: f64 = 300.0;
const PARK_NAME_MAX_ALTITUDE_KM: f64 = 300.0;
const PARK_OUTLINE_MAX_ALTITUDE_KM: f64 = 150.0;
const RIVER_MAX_ALTITUDE_KM: f64 = 1_500.0;
const CANAL_MAX_ALTITUDE_KM: f64 = 100.0;
/// Lake and bay names below this; seas and oceans at every shown altitude.
const SMALL_WATER_MAX_ALTITUDE_KM: f64 = 400.0;
const MAX_LABELS: usize = 120;
const LABEL_SEPARATION_POINTS: f32 = 60.0;

/// Which of the layers are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoiLayers {
    pub places: bool,
    pub peaks: bool,
    pub waters: bool,
    pub parks: bool,
}

impl PoiLayers {
    pub fn any(self) -> bool {
        self.places || self.peaks || self.waters || self.parks
    }
}

pub struct ProjectedPoiLine {
    pub kind: PoiLineKind,
    /// The altitude fade of its kind (1 when fully shown).
    pub opacity: f32,
    pub segments: Vec<Vec<Pos2>>,
}

pub struct ProjectedPoiLabel {
    pub position: Pos2,
    /// The point: tile `tile` of the snapshot, point `index` of that tile.
    pub tile: usize,
    pub index: usize,
    /// The altitude fade of its kind (1 when fully shown).
    pub opacity: f32,
}

#[derive(Default)]
pub struct ProjectedPoi {
    pub lines: Vec<ProjectedPoiLine>,
    pub labels: Vec<ProjectedPoiLabel>,
}

/// How visible a kind of line is: 0 when its layer is off or the camera is above its
/// altitude, fading in below that (`super::altitude_fade`).
fn line_opacity(kind: PoiLineKind, layers: PoiLayers, altitude_km: f64) -> f32 {
    let (on, max_altitude_km) = match kind {
        PoiLineKind::River => (layers.waters, RIVER_MAX_ALTITUDE_KM),
        PoiLineKind::Canal => (layers.waters, CANAL_MAX_ALTITUDE_KM),
        PoiLineKind::Park => (layers.parks, PARK_OUTLINE_MAX_ALTITUDE_KM),
    };
    if on {
        super::altitude_fade(max_altitude_km, altitude_km)
    } else {
        0.0
    }
}

/// Whether a point shows: its priority (lower first) and opacity (its altitude fade).
fn point_priority(
    point: &globecraft_data::poi::PoiPoint,
    layers: PoiLayers,
    altitude_km: f64,
) -> Option<(i64, f32)> {
    let fade = |on: bool, max_altitude_km: f64| {
        let opacity = if on {
            super::altitude_fade(max_altitude_km, altitude_km)
        } else {
            0.0
        };
        (opacity > 0.0).then_some(opacity)
    };
    match point.kind {
        PointKind::Water => {
            let big = matches!(point.class.as_str(), "ocean" | "sea");
            if big {
                layers.waters.then_some((0, 1.0))
            } else {
                fade(layers.waters, SMALL_WATER_MAX_ALTITUDE_KM).map(|opacity| (1_000, opacity))
            }
        }
        PointKind::Peak => fade(layers.peaks, PEAK_MAX_ALTITUDE_KM).map(|opacity| {
            (
                2_000 - i64::from(point.elevation_m.unwrap_or(0)) / 10,
                opacity,
            )
        }),
        PointKind::Park => fade(layers.parks, PARK_NAME_MAX_ALTITUDE_KM)
            .map(|opacity| (3_000 + i64::from(point.rank), opacity)),
        PointKind::Poi => fade(layers.places, POI_MAX_ALTITUDE_KM)
            .map(|opacity| (4_000 + i64::from(point.rank), opacity)),
    }
}

pub(super) fn project(input: &LiveInput, ground: Option<&dyn Ground>) -> ProjectedPoi {
    let Some(snapshot) = &input.poi else {
        return ProjectedPoi::default();
    };
    let layers = input.poi_layers;
    if !layers.any() {
        return ProjectedPoi::default();
    }
    let view = input.view;
    let visibility = TerrainVisibility::new(&view.basis, ground);
    let clip = view
        .rect
        .expand(crate::overlays::SEGMENT_CLIP_MARGIN_POINTS);
    let mut lines = Vec::new();
    for tile in &snapshot.tiles {
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
                lines.push(ProjectedPoiLine {
                    kind: line.kind,
                    opacity,
                    segments,
                });
            }
        }
    }
    ProjectedPoi {
        lines,
        labels: labels(input, snapshot, layers, ground),
    }
}

fn labels(
    input: &LiveInput,
    snapshot: &PoiSnapshot,
    layers: PoiLayers,
    ground: Option<&dyn Ground>,
) -> Vec<ProjectedPoiLabel> {
    let view = input.view;
    let visibility = TerrainVisibility::new(&view.basis, ground);
    let mut seen = HashSet::new();
    let mut found: Vec<(i64, ProjectedPoiLabel)> = Vec::new();
    for (tile_index, tile) in snapshot.tiles.iter().enumerate() {
        for (index, point) in tile.points.iter().enumerate() {
            let Some((priority, opacity)) = point_priority(point, layers, view.altitude_km) else {
                continue;
            };
            // A coarser tile shown while a finer one loads may repeat a point.
            let identity = (
                point.name.clone(),
                (point.lat * 1_000.0).round() as i64,
                (point.lon * 1_000.0).round() as i64,
            );
            if !seen.insert(identity) {
                continue;
            }
            let Some(position) = view.basis.project(
                ground_position(&view.basis, ground, point.lat, point.lon),
                view.rect,
            ) else {
                continue;
            };
            // On screen first: the line-of-sight test is the expensive part.
            if view.rect.contains(position)
                && visibility
                    .as_ref()
                    .is_none_or(|visibility| visibility.visible(point.lat, point.lon))
            {
                found.push((
                    priority,
                    ProjectedPoiLabel {
                        position,
                        tile: tile_index,
                        index,
                        opacity,
                    },
                ));
            }
        }
    }
    found.sort_by_key(|(priority, _)| *priority);
    let separation = LABEL_SEPARATION_POINTS * view.map_scale;
    let mut kept: Vec<ProjectedPoiLabel> = Vec::new();
    for (_, label) in found {
        if kept.len() == MAX_LABELS {
            break;
        }
        if kept
            .iter()
            .all(|other| other.position.distance(label.position) >= separation)
        {
            kept.push(label);
        }
    }
    kept
}
