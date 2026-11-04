//! Places of interest, peaks, waters and parks on screen: rivers, canals and park
//! outlines clipped to the view in f64, and the names to show, most important first
//! and kept apart.
use super::LiveInput;
use crate::overlays::{Ground, TerrainVisibility, ground_position};
use emath::Pos2;
use globecraft_data::poi::{PoiLineKind, PoiPoint, PoiSnapshot, PointKind};
use std::collections::{HashMap, HashSet};

/// Below these altitudes (km) each kind shows.
const POI_MAX_ALTITUDE_KM: f64 = 15.0;
const PEAK_MAX_ALTITUDE_KM: f64 = 300.0;
const PARK_NAME_MAX_ALTITUDE_KM: f64 = 300.0;
const PARK_OUTLINE_MAX_ALTITUDE_KM: f64 = 150.0;
const RIVER_MAX_ALTITUDE_KM: f64 = 1_500.0;
const CANAL_MAX_ALTITUDE_KM: f64 = 100.0;
/// Lake and bay names below this; seas and oceans at every shown altitude.
const SMALL_WATER_MAX_ALTITUDE_KM: f64 = 400.0;
const MAX_LABELS: usize = 200;
/// The least distance between two markers. Names are kept apart by the app, which
/// measures them, so this only thins out the candidates.
const LABEL_SEPARATION_POINTS: f32 = 24.0;
/// Lifts names already on screen above every other priority (all are below it).
const SHOWN_FIRST: i64 = 1_000_000;

/// One name on the map, whichever tile carries it: the same place in a coarser and
/// a finer tile has the same key (its name, and its position to about 100 m).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PoiKey {
    name: String,
    lat_mdeg: i64,
    lon_mdeg: i64,
}

impl PoiKey {
    pub fn of(point: &PoiPoint) -> Self {
        Self {
            name: point.name.clone(),
            lat_mdeg: (point.lat * 1_000.0).round() as i64,
            lon_mdeg: (point.lon * 1_000.0).round() as i64,
        }
    }
}

/// The names on screen now (and for a moment after they left), which keep their
/// places: they come first and are never thinned out for a newly arrived one.
pub type PoiShown = HashSet<PoiKey>;

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
    /// The snapshot tile it came from (for its fade), and its line there (its name).
    pub tile: usize,
    pub index: usize,
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
    for (tile_index, tile) in snapshot.tiles.iter().enumerate() {
        for (line_index, line) in tile.lines.iter().enumerate() {
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
                    tile: tile_index,
                    index: line_index,
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
    let shown = input.poi_shown.as_deref();
    // Each name once: the index in `found`, and whether its tile is being retired.
    let mut seen: HashMap<PoiKey, (usize, bool)> = HashMap::new();
    let mut found: Vec<(i64, ProjectedPoiLabel)> = Vec::new();
    for (tile_index, tile) in snapshot.tiles.iter().enumerate() {
        let retiring = snapshot.fade(tile_index).retired.is_some();
        for (index, point) in tile.points.iter().enumerate() {
            let Some((priority, opacity)) = point_priority(point, layers, view.altitude_km) else {
                continue;
            };
            // A coarser tile shown while a finer one loads may repeat a point: keep
            // the copy whose tile stays, so the name does not leave with the old tile.
            let key = PoiKey::of(point);
            if let Some(&(at, was_retiring)) = seen.get(&key) {
                if was_retiring && !retiring {
                    found[at].1.tile = tile_index;
                    found[at].1.index = index;
                    seen.insert(key, (at, false));
                }
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
                // Names on screen keep their places: before every new one.
                let priority = if shown.is_some_and(|shown| shown.contains(&key)) {
                    priority - SHOWN_FIRST
                } else {
                    priority
                };
                seen.insert(key, (found.len(), retiring));
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
    for (priority, label) in found {
        if kept.len() == MAX_LABELS {
            break;
        }
        if priority < 0
            || kept
                .iter()
                .all(|other| other.position.distance(label.position) >= separation)
        {
            kept.push(label);
        }
    }
    kept
}
