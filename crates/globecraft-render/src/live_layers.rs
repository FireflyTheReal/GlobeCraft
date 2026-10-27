//! Geographic preparation for roads, AIS, predicted satellites and traffic.
mod worker;
use crate::motion;
use emath::{Pos2, Rect, Vec2};
use globecraft_core::{camera::Basis, math::destination};
use globecraft_data::{roads::Road, satellites::Satellite, traffic::TrafficTiles, vessels::Vessel};
use std::sync::Arc;
pub use worker::LiveWorker;

const VESSEL_HEADING_PROBE_KM: f64 = 2.0;
/// Dead reckoning moves a vessel along its course for at most this long after its
/// last AIS report (reports come every few seconds to minutes; a ship that stops
/// reporting is not sailed across the map).
const VESSEL_EXTRAPOLATION_MAX_SECS: f64 = 600.0;
/// Reports this old are drawn at the stale opacity.
const VESSEL_STALE_SECS: f64 = 1800.0;
/// Below this, AIS speed is mostly GPS jitter of a moored or anchored ship.
const VESSEL_MIN_MOVING_KNOTS: f64 = 0.5;
const VESSEL_MAX_SPEED_KNOTS: f64 = 60.0;
const KM_PER_NAUTICAL_MILE: f64 = 1.852;
const SECONDS_PER_HOUR: f64 = 3600.0;
const VESSEL_ICON_POINTS: f32 = 8.0;
const VESSEL_WIDTH_RATIO: f32 = 0.55;
const VESSEL_REAR_RATIO: f32 = 0.8;
const VESSEL_TAIL_RATIO: f32 = 0.65;
const MIN_HEADING_LENGTH_SQUARED: f32 = 0.0001;
const MIN_ROAD_SEGMENT_LENGTH_SQUARED: f32 = 0.01;

#[derive(Clone, Copy, PartialEq)]
pub struct LiveView {
    pub basis: Basis,
    pub rect: Rect,
    pub altitude_km: f64,
    pub map_scale: f32,
    pub pixels_per_point: f32,
    pub traffic_width_px: usize,
    pub traffic_height_px: usize,
}
impl LiveView {
    /// How often vessels move in this view, in seconds (see [`crate::motion`]).
    pub fn vessel_step(&self) -> f64 {
        motion::step_secs(
            self.altitude_km,
            self.rect.height(),
            motion::VESSEL_SPEED_KM_S,
        )
    }
    /// How often satellites move in this view, in seconds.
    pub fn satellite_step(&self) -> f64 {
        motion::step_secs(
            self.altitude_km,
            self.rect.height(),
            motion::SATELLITE_SPEED_KM_S,
        )
    }
}
#[derive(Clone)]
pub struct LiveInput {
    pub view: LiveView,
    pub roads: Option<Arc<[Road]>>,
    pub vessels: Option<Arc<[Vessel]>>,
    pub satellites: Option<Arc<[Satellite]>>,
    pub traffic: Option<Arc<TrafficTiles>>,
    pub unix_secs: f64,
}
fn same<T: ?Sized>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}
impl LiveInput {
    pub fn same_request(&self, other: &Self) -> bool {
        self.view == other.view
            && same(&self.roads, &other.roads)
            && same(&self.vessels, &other.vessels)
            && same(&self.satellites, &other.satellites)
            && same(&self.traffic, &other.traffic)
            && (self.vessels.is_none() || self.same_vessel_time(other))
            && (self.satellites.is_none() || self.same_satellite_time(other))
    }
    fn same_vessel_time(&self, other: &Self) -> bool {
        let step = self.view.vessel_step();
        motion::tick(self.unix_secs, step) == motion::tick(other.unix_secs, step)
    }
    fn same_satellite_time(&self, other: &Self) -> bool {
        let step = self.view.satellite_step();
        motion::tick(self.unix_secs, step) == motion::tick(other.unix_secs, step)
    }
}

pub struct ProjectedRoad {
    pub source_index: usize,
    pub segments: Vec<Vec<Pos2>>,
    bounds: Rect,
}
pub struct ProjectedVessel {
    pub source_index: usize,
    pub position: Pos2,
    pub icon: [Pos2; 4],
    /// Dimmed as the last AIS report ages.
    pub opacity: f32,
}
pub struct ProjectedSatellite {
    pub source_index: usize,
    pub position: Pos2,
}
pub struct LiveGeometry {
    pub input: LiveInput,
    pub roads: Arc<Vec<ProjectedRoad>>,
    pub vessels: Arc<Vec<ProjectedVessel>>,
    pub satellites: Arc<Vec<ProjectedSatellite>>,
    pub traffic: Option<Arc<Vec<u8>>>,
}
impl LiveGeometry {
    pub fn nearest_road(&self, mouse: Pos2, radius: f32) -> Option<(usize, Pos2)> {
        let mut nearest = None;
        let mut minimum = radius;
        for road in self
            .roads
            .iter()
            .filter(|road| road.bounds.expand(radius).contains(mouse))
        {
            for segment in road.segments.iter().flat_map(|line| line.windows(2)) {
                let a = segment[0];
                let ab = segment[1] - a;
                let factor = if ab.length_sq() > MIN_ROAD_SEGMENT_LENGTH_SQUARED {
                    ((mouse - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let point = a + ab * factor;
                let distance = (point - mouse).length();
                if distance < minimum {
                    minimum = distance;
                    nearest = Some((road.source_index, point));
                }
            }
        }
        nearest
    }
}
#[derive(Default)]
pub struct LivePreparer {
    latest: Option<Arc<LiveGeometry>>,
}
impl LivePreparer {
    pub fn prepare(&mut self, input: LiveInput) -> Arc<LiveGeometry> {
        if let Some(old) = &self.latest
            && old.input.same_request(&input)
        {
            return old.clone();
        }
        let previous = self
            .latest
            .as_ref()
            .filter(|old| old.input.view == input.view);
        let roads = previous
            .filter(|old| same(&old.input.roads, &input.roads))
            .map(|old| old.roads.clone())
            .unwrap_or_else(|| Arc::new(project_roads(&input)));
        let vessels = previous
            .filter(|old| {
                same(&old.input.vessels, &input.vessels) && old.input.same_vessel_time(&input)
            })
            .map(|old| old.vessels.clone())
            .unwrap_or_else(|| Arc::new(project_vessels(&input)));
        let satellites = previous
            .filter(|old| {
                same(&old.input.satellites, &input.satellites)
                    && old.input.same_satellite_time(&input)
            })
            .map(|old| old.satellites.clone())
            .unwrap_or_else(|| Arc::new(project_satellites(&input)));
        let traffic =
            if let Some(old) = previous.filter(|old| same(&old.input.traffic, &input.traffic)) {
                old.traffic.clone()
            } else {
                input.traffic.as_ref().map(|tiles| {
                    Arc::new(crate::weather::project_traffic_tiles(
                        &input.view.basis,
                        input.view.rect,
                        input.view.traffic_width_px,
                        input.view.traffic_height_px,
                        tiles,
                    ))
                })
            };
        let geometry = Arc::new(LiveGeometry {
            input,
            roads,
            vessels,
            satellites,
            traffic,
        });
        self.latest = Some(geometry.clone());
        geometry
    }
}
fn project_roads(input: &LiveInput) -> Vec<ProjectedRoad> {
    let Some(roads) = &input.roads else {
        return Vec::new();
    };
    roads
        .iter()
        .enumerate()
        .map(|(source_index, road)| {
            let mut segments = Vec::new();
            let mut visible = Vec::with_capacity(road.points.len());
            for &(lat, lon) in &road.points {
                if let Some(position) = input
                    .view
                    .basis
                    .project(input.view.basis.surface_position(lat, lon), input.view.rect)
                {
                    visible.push(position);
                } else if !visible.is_empty() {
                    segments.push(std::mem::take(&mut visible));
                }
            }
            if !visible.is_empty() {
                segments.push(visible);
            }
            let bounds = segments
                .iter()
                .flatten()
                .fold(Rect::NOTHING, |bounds, point| {
                    bounds.union(Rect::from_min_max(*point, *point))
                });
            ProjectedRoad {
                source_index,
                segments,
                bounds,
            }
        })
        .collect()
}
fn project_vessels(input: &LiveInput) -> Vec<ProjectedVessel> {
    let Some(vessels) = &input.vessels else {
        return Vec::new();
    };
    let view = input.view;
    vessels
        .iter()
        .enumerate()
        .filter_map(|(source_index, ship)| {
            let age = (input.unix_secs - ship.observed_unix).max(0.0);
            let (lat, lon) = dead_reckon(ship, age);
            let at = view
                .basis
                .project(view.basis.surface_position(lat, lon), view.rect)?;
            if !view.rect.contains(at) {
                return None;
            }
            // Preserve the original 2 km course probe; project it onto the selected model.
            let (lat, lon) = destination(lat, lon, ship.course, VESSEL_HEADING_PROBE_KM);
            let forward = view
                .basis
                .project(view.basis.surface_position(lat, lon), view.rect)
                .map(|p| p - at)
                .filter(|v| v.length_sq() > MIN_HEADING_LENGTH_SQUARED)
                .map(|v| v.normalized() * VESSEL_ICON_POINTS * view.map_scale)
                .unwrap_or(Vec2::new(0.0, -VESSEL_ICON_POINTS * view.map_scale));
            let right = Vec2::new(-forward.y, forward.x) * VESSEL_WIDTH_RATIO;
            Some(ProjectedVessel {
                opacity: motion::age_opacity(age, VESSEL_EXTRAPOLATION_MAX_SECS, VESSEL_STALE_SECS),
                source_index,
                position: at,
                icon: [
                    at + forward,
                    at + right - forward * VESSEL_REAR_RATIO,
                    at - forward * VESSEL_TAIL_RATIO,
                    at - right - forward * VESSEL_REAR_RATIO,
                ],
            })
        })
        .collect()
}
/// Where `ship` is `age` seconds after its report, sailing its course at its speed.
fn dead_reckon(ship: &Vessel, age: f64) -> (f64, f64) {
    let knots = ship.speed_knots.unwrap_or(0.0);
    if !(VESSEL_MIN_MOVING_KNOTS..=VESSEL_MAX_SPEED_KNOTS).contains(&knots)
        || !ship.course.is_finite()
    {
        return (ship.lat, ship.lon);
    }
    let km =
        knots * KM_PER_NAUTICAL_MILE / SECONDS_PER_HOUR * age.min(VESSEL_EXTRAPOLATION_MAX_SECS);
    destination(ship.lat, ship.lon, ship.course, km)
}
fn project_satellites(input: &LiveInput) -> Vec<ProjectedSatellite> {
    let Some(satellites) = &input.satellites else {
        return Vec::new();
    };
    satellites
        .iter()
        .enumerate()
        .filter_map(|(source_index, sat)| {
            let (lat, lon) = sat.ground_position(input.unix_secs)?;
            let position = input
                .view
                .basis
                .project(input.view.basis.surface_position(lat, lon), input.view.rect)?;
            input
                .view
                .rect
                .contains(position)
                .then_some(ProjectedSatellite {
                    source_index,
                    position,
                })
        })
        .collect()
}

#[cfg(test)]
mod tests;
