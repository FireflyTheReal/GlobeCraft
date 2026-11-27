//! Roof shapes (D3): Overture's `roof_shape`, `roof_height`, `roof_direction` and
//! `roof_orientation` as a roof over the block. Every shape is the lowest of a few
//! planes over the footprint (curved ones as planes touching the curve): the footprint
//! is cut into the part where each plane is the lowest, each part triangulated, and
//! each wall rises to the roof above it, bending where the roof creases. All of it
//! is worked out once, when the tile is decoded.
//!
//! The planes are laid out over the footprint's smallest enclosing rectangle (or, with
//! a mapped direction, the rectangle turned that way), the ridge along its longer side
//! unless the roof is mapped `across`.
use glam::DVec2;

/// Slopes of roofs mapped without a height.
const DEFAULT_PITCH_DEG: f64 = 30.0;
const DEFAULT_SKILLION_PITCH_DEG: f64 = 15.0;
/// A roof mapped without a height takes at most this share of the block's height.
const MAX_GUESSED_ROOF_SHARE: f32 = 0.5;
/// Lower roofs are drawn flat (metres).
const MIN_ROOF_M: f32 = 0.3;
/// Gambrel and mansard roofs: the steep lower slope covers this share of the
/// rectangle's half width and rises to this share of the roof's height.
const BROKEN_LOWER_WIDTH_SHARE: f64 = 0.3;
const BROKEN_LOWER_RISE_SHARE: f64 = 0.6;
/// A half-hipped roof's gable walls rise to this share of the roof's height.
const HALF_HIP_GABLE_SHARE: f64 = 0.5;
/// A dome is drawn as the planes touching it on rings at these shares of its radius,
/// at this many bearings each, under a flat top.
const DOME_RINGS: [f64; 3] = [0.4, 0.75, 0.93];
const DOME_BEARINGS: usize = 8;
/// A barrel roof (`round`) is drawn as the planes touching it at these shares of its
/// half width, on both sides, under a flat top.
const BARREL_SHARES: [f64; 4] = [0.3, 0.6, 0.85, 0.97];
/// Parts of a roof smaller than this (square metres) are left out: slivers along a
/// crease.
const MIN_PART_M2: f64 = 0.01;
/// Planes this close (metres over the footprint's size) are one.
const SAME_PLANE_M: f64 = 1e-6;
/// Bends of a wall closer together than this share of its edge are one.
const MIN_BEND_SHARE: f64 = 1e-4;

/// A roof that is not flat, over a block's rings.
#[derive(Clone, Debug, Default)]
pub struct ShapedRoof {
    /// Per point of the rings, ring after ring: the roof's height above the ground
    /// there (metres), where the wall's top meets it.
    pub tops_m: Vec<f32>,
    /// Where walls' tops bend under a crease of the roof, by edge (the index of the
    /// edge's first point, counted ring after ring) and then along it; wall edges only.
    pub bends: Vec<WallBend>,
    /// The roof's points: latitude and longitude from the block's first point
    /// (degrees), height above the ground (metres).
    pub points: Vec<[f32; 3]>,
    /// The roof as triangles over `points`, each in one flat part of the roof.
    pub triangles: Vec<u32>,
}

/// A point along a wall's edge where the roof above it creases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallBend {
    pub edge: u32,
    /// From the edge's first point (0) to its second (1).
    pub share: f32,
    pub height_m: f32,
}

/// A building's roof tags.
pub(super) struct RoofTags<'a> {
    pub(super) shape: Option<&'a str>,
    pub(super) height_m: Option<f32>,
    /// The compass bearing the roof's slope faces (degrees).
    pub(super) direction_deg: Option<f32>,
    /// The ridge runs across the footprint's longer side.
    pub(super) across: bool,
}

/// A building's footprint and what the roof is laid over, in tile units (y down).
pub(super) struct Footprint<'a> {
    /// The outline before it was cut to the tile: the roof is laid out over it, so
    /// both halves of a block on a tile's edge get the same roof.
    pub(super) outline: &'a [(f64, f64)],
    /// The block's rings in this tile and the triangles over them.
    pub(super) rings: &'a [Vec<(f64, f64)>],
    pub(super) walls: &'a [Vec<bool>],
    pub(super) metres_per_unit: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Skillion,
    Gabled,
    HalfHipped,
    Hipped,
    Pyramidal,
    Gambrel,
    Mansard,
    Dome,
    Round,
}

impl Shape {
    /// Overture's `roof_shape`; flat and unknown roofs are none.
    fn of(text: &str) -> Option<Self> {
        Some(match text {
            "skillion" => Self::Skillion,
            // A saltbox's two slopes differ in length; drawn as a gable.
            "gabled" | "saltbox" => Self::Gabled,
            "half_hipped" => Self::HalfHipped,
            "hipped" => Self::Hipped,
            "pyramidal" => Self::Pyramidal,
            "gambrel" => Self::Gambrel,
            "mansard" => Self::Mansard,
            // An onion is drawn as the dome under it.
            "dome" | "onion" => Self::Dome,
            "round" => Self::Round,
            _ => return None,
        })
    }
}

/// A plane over the footprint: its height (metres above the eaves) at a point
/// (metres east and north of the outline's first point).
#[derive(Clone, Copy, Debug)]
struct Plane {
    slope: DVec2,
    at_origin: f64,
}

impl Plane {
    fn at(self, point: DVec2) -> f64 {
        self.slope.dot(point) + self.at_origin
    }
}

/// The rectangle a roof is laid over: its middle, the ridge's direction and the one
/// across it (toward which a skillion falls), and its half sizes (metres).
struct Frame {
    middle: DVec2,
    along: DVec2,
    across: DVec2,
    half_along: f64,
    half_across: f64,
}

impl Frame {
    /// The rectangle round `points` turned along `along`.
    fn turned(points: &[DVec2], along: DVec2) -> Self {
        let across = DVec2::new(-along.y, along.x);
        let (mut low, mut high) = (DVec2::INFINITY, DVec2::NEG_INFINITY);
        for &point in points {
            let at = DVec2::new(point.dot(along), point.dot(across));
            (low, high) = (low.min(at), high.max(at));
        }
        let middle = 0.5 * (low + high);
        Self {
            middle: along * middle.x + across * middle.y,
            along,
            across,
            half_along: 0.5 * (high.x - low.x),
            half_across: 0.5 * (high.y - low.y),
        }
    }

    /// Where `point` is in the frame: along the ridge and across it, from the middle.
    fn local(&self, point: DVec2) -> DVec2 {
        let offset = point - self.middle;
        DVec2::new(offset.dot(self.along), offset.dot(self.across))
    }

    /// The plane `along` × u + `across` × v + `middle` (u, v in the frame).
    fn plane(&self, along: f64, across: f64, middle: f64) -> Plane {
        let slope = self.along * along + self.across * across;
        Plane {
            slope,
            at_origin: middle - slope.dot(self.middle),
        }
    }
}

/// The roof over a block, or none for a flat one (or one too low or too small to
/// shape). `height_m` is the block's (base, top); its top is the roof's.
pub(super) fn shaped(
    tags: &RoofTags<'_>,
    footprint: &Footprint<'_>,
    (base_m, top_m): (f32, f32),
    to_lat_lon: impl Fn((f64, f64)) -> (f64, f64),
) -> Option<Box<ShapedRoof>> {
    let shape = tags.shape.and_then(Shape::of)?;
    // Metres east and north of the outline's first point.
    let &origin = footprint.outline.first()?;
    let scale = footprint.metres_per_unit;
    let local = |(x, y): (f64, f64)| DVec2::new((x - origin.0) * scale, (origin.1 - y) * scale);
    let tile = |point: DVec2| (origin.0 + point.x / scale, origin.1 - point.y / scale);
    let outline: Vec<DVec2> = footprint
        .outline
        .iter()
        .map(|&point| local(point))
        .collect();
    let frame = frame(&outline, tags)?;
    let rise = rise(shape, &frame, tags.height_m, top_m - base_m)?;
    let planes = planes(shape, &frame, &outline, f64::from(rise));
    let eaves_m = f64::from(top_m - rise);
    let height_m = |value: f64| (eaves_m + value.clamp(0.0, f64::from(rise))) as f32;
    let roof_at = |point: DVec2| {
        planes
            .iter()
            .map(|plane| plane.at(point))
            .fold(f64::INFINITY, f64::min)
    };
    let points: Vec<DVec2> = footprint
        .rings
        .iter()
        .flatten()
        .map(|&p| local(p))
        .collect();
    let mut roof = ShapedRoof {
        tops_m: points
            .iter()
            .map(|&point| height_m(roof_at(point)))
            .collect(),
        ..ShapedRoof::default()
    };
    let mut first = 0;
    for (ring, walls) in footprint.rings.iter().zip(footprint.walls) {
        for (index, &is_wall) in walls.iter().enumerate() {
            if !is_wall {
                continue;
            }
            let (a, b) = (first + index, first + (index + 1) % ring.len());
            for (share, value) in bends(points[a], points[b], &planes) {
                roof.bends.push(WallBend {
                    edge: a as u32,
                    share: share as f32,
                    height_m: height_m(value),
                });
            }
        }
        first += ring.len();
    }
    // Each plane's part: the footprint where it is the lowest. Cutting is exact for a
    // convex outline, so a footprint that is not one (with a courtyard, or a reflex
    // corner) is cut triangle by triangle; every part is then convex, a fan.
    let &first = footprint.rings.first()?.first()?;
    // Latitude and longitude change evenly across a block (to well under a millimetre).
    let first_point = to_lat_lon(first);
    let (east, south) = (
        to_lat_lon((first.0 + 1.0, first.1)),
        to_lat_lon((first.0, first.1 + 1.0)),
    );
    let offset = |point: DVec2| {
        let (x, y) = tile(point);
        let (dx, dy) = (x - first.0, y - first.1);
        (
            dx * (east.0 - first_point.0) + dy * (south.0 - first_point.0),
            dx * (east.1 - first_point.1) + dy * (south.1 - first_point.1),
        )
    };
    let rings: Vec<Vec<DVec2>> = footprint
        .rings
        .iter()
        .map(|ring| ring.iter().map(|&p| local(p)).collect())
        .collect();
    let shapes: Vec<Vec<DVec2>> = if rings.len() == 1 && convex(&rings[0]) {
        rings
    } else {
        let (mut flat, mut holes, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
        for ring in &rings {
            if !flat.is_empty() {
                holes.push(flat.len() as u32);
            }
            flat.extend(ring.iter().copied());
        }
        earcut::Earcut::new().earcut(
            flat.iter().map(|point| [point.x, point.y]),
            &holes,
            &mut triangles,
        );
        triangles
            .chunks_exact(3)
            .map(|triangle| {
                [0, 1, 2]
                    .map(|corner| flat[triangle[corner] as usize])
                    .to_vec()
            })
            .collect()
    };
    for index in 0..planes.len() {
        for shape in &shapes {
            let Some(part) = part(shape, &planes, index) else {
                continue;
            };
            let start = roof.points.len() as u32;
            for &point in &part {
                let (lat, lon) = offset(point);
                roof.points
                    .push([lat as f32, lon as f32, height_m(planes[index].at(point))]);
            }
            for corner in 1..part.len() as u32 - 1 {
                roof.triangles
                    .extend([start, start + corner, start + corner + 1]);
            }
        }
    }
    roof.points.shrink_to_fit();
    roof.triangles.shrink_to_fit();
    roof.bends.shrink_to_fit();
    Some(Box::new(roof))
}

/// Whether `ring` turns one way only.
fn convex(ring: &[DVec2]) -> bool {
    let mut turns = (false, false);
    for (index, &point) in ring.iter().enumerate() {
        let (next, after) = (
            ring[(index + 1) % ring.len()],
            ring[(index + 2) % ring.len()],
        );
        let turn = (next - point).perp_dot(after - next);
        turns.0 |= turn > 0.0;
        turns.1 |= turn < 0.0;
    }
    !(turns.0 && turns.1)
}

/// The convex `shape` where plane `index` is the lowest; none where it never is (or
/// only along a crease).
fn part(shape: &[DVec2], planes: &[Plane], index: usize) -> Option<Vec<DVec2>> {
    let plane = planes[index];
    let mut part = shape.to_vec();
    for (other_index, &other) in planes.iter().enumerate() {
        if other_index == index {
            continue;
        }
        if (other.slope - plane.slope).length() < SAME_PLANE_M
            && (other.at_origin - plane.at_origin).abs() < SAME_PLANE_M
        {
            // A plane twice over: its first stands for both.
            if other_index < index {
                return None;
            }
            continue;
        }
        part = below(&part, |point| plane.at(point) - other.at(point));
        if area(&part) < MIN_PART_M2 {
            return None;
        }
    }
    (area(&part) >= MIN_PART_M2).then_some(part)
}

/// The rectangle the roof is laid over: turned to its mapped direction, else the
/// smallest round the outline (tried along each of its edges); the ridge along the
/// longer side unless mapped across.
fn frame(outline: &[DVec2], tags: &RoofTags<'_>) -> Option<Frame> {
    let frame = match tags.direction_deg {
        Some(bearing) => {
            let (east, north) = f64::from(bearing).to_radians().sin_cos();
            // The slope faces the bearing: the ridge runs square to it.
            let facing = DVec2::new(east, north);
            Frame::turned(outline, DVec2::new(facing.y, -facing.x))
        }
        None => {
            let mut best: Option<Frame> = None;
            for (index, &point) in outline.iter().enumerate() {
                let Some(along) = (outline[(index + 1) % outline.len()] - point).try_normalize()
                else {
                    continue;
                };
                let frame = Frame::turned(outline, along);
                if best.as_ref().is_none_or(|best| {
                    frame.half_along * frame.half_across < best.half_along * best.half_across
                }) {
                    best = Some(frame);
                }
            }
            let frame = best?;
            if (frame.half_along < frame.half_across) == tags.across {
                frame
            } else {
                Frame {
                    along: frame.across,
                    across: -frame.along,
                    half_along: frame.half_across,
                    half_across: frame.half_along,
                    ..frame
                }
            }
        }
    };
    (frame.half_along > 0.0 && frame.half_across > 0.0).then_some(frame)
}

/// How high the roof rises over its eaves (metres): as mapped, else from the
/// shape's usual slope; within the block (`block_m` tall).
fn rise(shape: Shape, frame: &Frame, mapped_m: Option<f32>, block_m: f32) -> Option<f32> {
    let rise = match mapped_m.filter(|&height| height > 0.0) {
        Some(height) => height.min(block_m),
        None => {
            let pitch = DEFAULT_PITCH_DEG.to_radians().tan();
            let guess = match shape {
                Shape::Skillion => {
                    2.0 * frame.half_across * DEFAULT_SKILLION_PITCH_DEG.to_radians().tan()
                }
                Shape::Dome => frame.half_along.min(frame.half_across),
                Shape::Round => frame.half_across,
                _ => frame.half_across * pitch,
            };
            (guess as f32).min(block_m * MAX_GUESSED_ROOF_SHARE)
        }
    };
    (rise >= MIN_ROOF_M).then_some(rise)
}

/// The planes whose lowest is the roof, `rise` metres high over the frame.
fn planes(shape: Shape, frame: &Frame, outline: &[DVec2], rise: f64) -> Vec<Plane> {
    let (half_along, half_across) = (frame.half_along, frame.half_across);
    // Both slopes of a gable: down to the eaves at either long side.
    let gable = |out: &mut Vec<Plane>| {
        for side in [-1.0, 1.0] {
            out.push(frame.plane(0.0, side * rise / half_across, rise));
        }
    };
    let mut planes = Vec::new();
    match shape {
        Shape::Skillion => planes.push(frame.plane(0.0, -rise / (2.0 * half_across), 0.5 * rise)),
        Shape::Gabled => gable(&mut planes),
        Shape::Hipped | Shape::HalfHipped => {
            gable(&mut planes);
            // The hips slope as the sides do, from the eaves (or half the gable up).
            let foot = if shape == Shape::Hipped {
                0.0
            } else {
                HALF_HIP_GABLE_SHARE * rise
            };
            let slope = rise / half_across;
            for side in [-1.0, 1.0] {
                planes.push(frame.plane(side * slope, 0.0, foot + slope * half_along));
            }
        }
        Shape::Pyramidal => {
            // A face from each side of the outline's hull up to its middle, so a
            // spire over an octagon or a circle comes to a point from every side.
            let hull = hull(outline);
            let apex = centroid(&hull).unwrap_or(frame.middle);
            for (index, &from) in hull.iter().enumerate() {
                let to = hull[(index + 1) % hull.len()];
                let Some(along) = (to - from).try_normalize() else {
                    continue;
                };
                let inward = DVec2::new(-along.y, along.x);
                let reach = inward.dot(apex - from);
                if reach > 0.0 {
                    let slope = inward * (rise / reach);
                    planes.push(Plane {
                        slope,
                        at_origin: -slope.dot(from),
                    });
                }
            }
            if planes.is_empty() {
                planes.push(frame.plane(0.0, 0.0, rise));
            }
        }
        Shape::Gambrel | Shape::Mansard => {
            // A steep lower slope up to the break, a shallow one above it.
            let lower_width = BROKEN_LOWER_WIDTH_SHARE * half_across;
            let lower_rise = BROKEN_LOWER_RISE_SHARE * rise;
            let (steep, shallow) = (
                lower_rise / lower_width,
                (rise - lower_rise) / (half_across - lower_width),
            );
            let mut ends = vec![(half_across, false)];
            if shape == Shape::Mansard {
                ends.push((half_along, true));
            }
            for (half, along) in ends {
                for side in [-1.0, 1.0] {
                    let tilt = |slope: f64| {
                        if along {
                            (side * slope, 0.0)
                        } else {
                            (0.0, side * slope)
                        }
                    };
                    let (a, b) = tilt(steep);
                    planes.push(frame.plane(a, b, steep * half));
                    let (a, b) = tilt(shallow);
                    planes.push(frame.plane(a, b, lower_rise + shallow * (half - lower_width)));
                }
            }
        }
        Shape::Dome => {
            // An ellipsoid's cap through the outline's farthest corner.
            let reach = outline
                .iter()
                .map(|&point| {
                    let at = frame.local(point);
                    (at.x / half_along).hypot(at.y / half_across)
                })
                .fold(1.0, f64::max);
            let (radius_along, radius_across) = (half_along * reach, half_across * reach);
            planes.push(frame.plane(0.0, 0.0, rise));
            for ring in DOME_RINGS {
                let up = (1.0 - ring * ring).sqrt();
                for bearing in 0..DOME_BEARINGS {
                    let angle = std::f64::consts::TAU * bearing as f64 / DOME_BEARINGS as f64;
                    let (u, v) = (
                        radius_along * ring * angle.cos(),
                        radius_across * ring * angle.sin(),
                    );
                    let (a, b) = (
                        -rise * u / (radius_along * radius_along * up),
                        -rise * v / (radius_across * radius_across * up),
                    );
                    planes.push(frame.plane(a, b, rise * up - a * u - b * v));
                }
            }
        }
        Shape::Round => {
            let reach = outline
                .iter()
                .map(|&point| (frame.local(point).y / half_across).abs())
                .fold(1.0, f64::max);
            let radius = half_across * reach;
            planes.push(frame.plane(0.0, 0.0, rise));
            for share in BARREL_SHARES {
                let up = (1.0 - share * share).sqrt();
                for side in [-1.0, 1.0] {
                    let v = side * share * radius;
                    let b = -rise * v / (radius * radius * up);
                    planes.push(frame.plane(0.0, b, rise * up - b * v));
                }
            }
        }
    }
    planes
}

/// The convex hull of `points`, anticlockwise (Andrew's monotone chain).
fn hull(points: &[DVec2]) -> Vec<DVec2> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let mut hull: Vec<DVec2> = Vec::with_capacity(sorted.len() + 1);
    for pass in [false, true] {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &DVec2>> = if pass {
            Box::new(sorted.iter().rev())
        } else {
            Box::new(sorted.iter())
        };
        for &point in ordered {
            while hull.len() >= start + 2
                && (hull[hull.len() - 1] - hull[hull.len() - 2])
                    .perp_dot(point - hull[hull.len() - 2])
                    <= 0.0
            {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

/// A polygon's centre of area; none for one without area.
fn centroid(polygon: &[DVec2]) -> Option<DVec2> {
    let (mut sum, mut twice_area) = (DVec2::ZERO, 0.0);
    for (index, &from) in polygon.iter().enumerate() {
        let to = polygon[(index + 1) % polygon.len()];
        let cross = from.perp_dot(to);
        sum += (from + to) * cross;
        twice_area += cross;
    }
    (twice_area.abs() > 0.0).then(|| sum / (3.0 * twice_area))
}

/// Where the roof creases over the edge from `a` to `b`: (share along it, roof height
/// over the eaves there), in order. The roof along an edge is the lowest of the
/// planes, so each bend hands over to a plane falling faster along it.
fn bends(a: DVec2, b: DVec2, planes: &[Plane]) -> Vec<(f64, f64)> {
    let step = b - a;
    let value = |plane: usize, share: f64| planes[plane].at(a + step * share);
    let slope = |plane: usize| planes[plane].slope.dot(step);
    let lowest = (0..planes.len()).fold(0, |best, plane| {
        let (here, there) = (value(plane, 0.0), value(best, 0.0));
        if here < there - 1e-9 || (here <= there + 1e-9 && slope(plane) < slope(best)) {
            plane
        } else {
            best
        }
    });
    let (mut active, mut share, mut out) = (lowest, 0.0, Vec::new());
    loop {
        let mut next: Option<(f64, usize)> = None;
        for plane in 0..planes.len() {
            let falls = slope(active) - slope(plane);
            if falls <= 0.0 {
                continue;
            }
            let at = share + (value(plane, share) - value(active, share)) / falls;
            if at <= share + MIN_BEND_SHARE || at >= 1.0 - MIN_BEND_SHARE {
                continue;
            }
            let better = next.is_none_or(|(best, chosen)| {
                at < best - MIN_BEND_SHARE
                    || (at < best + MIN_BEND_SHARE && slope(plane) < slope(chosen))
            });
            if better {
                next = Some((at, plane));
            }
        }
        let Some((at, plane)) = next else { break };
        out.push((at, value(plane, at)));
        (share, active) = (at, plane);
    }
    out
}

/// The part of the convex `polygon` where `side` is at most zero (Sutherland–Hodgman).
fn below(polygon: &[DVec2], side: impl Fn(DVec2) -> f64) -> Vec<DVec2> {
    let mut below = Vec::with_capacity(polygon.len() + 2);
    for (index, &from) in polygon.iter().enumerate() {
        let to = polygon[(index + 1) % polygon.len()];
        let (at_from, at_to) = (side(from), side(to));
        if at_from <= 0.0 {
            below.push(from);
        }
        if (at_from < 0.0 && at_to > 0.0) || (at_from > 0.0 && at_to < 0.0) {
            below.push(from.lerp(to, at_from / (at_from - at_to)));
        }
    }
    below
}

/// A polygon's area (square metres).
fn area(polygon: &[DVec2]) -> f64 {
    if polygon.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for (index, &point) in polygon.iter().enumerate() {
        sum += point.perp_dot(polygon[(index + 1) % polygon.len()]);
    }
    0.5 * sum.abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rectangle `long` × `wide` metres (one unit a metre), long side east–west.
    fn rectangle(long: f64, wide: f64) -> Vec<(f64, f64)> {
        vec![(0.0, 0.0), (long, 0.0), (long, -wide), (0.0, -wide)]
    }

    fn roof(shape: &str, ring: &[(f64, f64)], tags: Option<(f32, f32, bool)>) -> ShapedRoof {
        let (height_m, direction_deg, across) = tags.map_or((None, None, false), |(h, d, a)| {
            (Some(h), (d >= 0.0).then_some(d), a)
        });
        let rings = vec![ring.to_vec()];
        let walls = vec![vec![true; ring.len()]];
        *shaped(
            &RoofTags {
                shape: Some(shape),
                height_m,
                direction_deg,
                across,
            },
            &Footprint {
                outline: ring,
                rings: &rings,
                walls: &walls,
                metres_per_unit: 1.0,
            },
            (0.0, 10.0),
            |(x, y)| (-y, x),
        )
        .unwrap()
    }

    /// The roof's points as (east, north, height): the test's first point is 0, 0.
    fn corners(roof: &ShapedRoof) -> Vec<(f64, f64, f32)> {
        roof.points
            .iter()
            .map(|&[lat, lon, h]| (f64::from(lon), f64::from(lat), h))
            .collect()
    }

    /// The roof's area seen from above: the footprint's, with no gap or overlap.
    fn covered(roof: &ShapedRoof) -> f64 {
        let points = corners(roof);
        roof.triangles
            .chunks_exact(3)
            .map(|triangle| {
                let face = [0, 1, 2].map(|corner| {
                    let (x, y, _) = points[triangle[corner] as usize];
                    DVec2::new(x, y)
                });
                area(&face)
            })
            .sum()
    }

    #[test]
    fn a_gable_rises_to_its_ridge_along_the_long_side() {
        let gable = roof("gabled", &rectangle(20.0, 10.0), Some((4.0, -1.0, false)));
        // Walls at the eaves (6 m) at every corner, the ridge at 10 m.
        assert_eq!(gable.tops_m, [6.0; 4]);
        // The two short walls bend at their middles up to the ridge: gable ends.
        let mut bends: Vec<_> = gable
            .bends
            .iter()
            .map(|b| (b.edge, b.share, b.height_m))
            .collect();
        bends.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(bends, [(1, 0.5, 10.0), (3, 0.5, 10.0)]);
        assert!((covered(&gable) - 200.0).abs() < 1e-3);
        for (_, north, height) in corners(&gable) {
            let expected = 10.0 - 4.0 * ((north - 5.0).abs() / 5.0);
            assert!(
                (f64::from(height) - expected).abs() < 1e-4,
                "{north} {height}"
            );
        }
        // Mapped across: the ridge runs north–south instead.
        let across = roof("gabled", &rectangle(20.0, 10.0), Some((4.0, -1.0, true)));
        assert_eq!(
            across.bends.iter().map(|b| b.edge).collect::<Vec<_>>(),
            [0, 2]
        );
    }

    #[test]
    fn a_hipped_roof_slopes_on_all_sides_and_its_walls_stay_level() {
        let roof = roof("hipped", &rectangle(20.0, 10.0), Some((4.0, -1.0, false)));
        assert_eq!(roof.tops_m, [6.0; 4]);
        assert!(roof.bends.is_empty());
        assert!((covered(&roof) - 200.0).abs() < 1e-3);
        // The ridge from 5 to 15 m east, at the top.
        let top: Vec<_> = corners(&roof)
            .into_iter()
            .filter(|&(_, _, h)| h > 9.999)
            .map(|(east, _, _)| (east * 1e3).round() / 1e3)
            .collect();
        assert!(
            top.contains(&5.0)
                && top.contains(&15.0)
                && top.iter().all(|e| (5.0..=15.0).contains(e))
        );
    }

    #[test]
    fn a_spire_over_an_octagon_rises_from_every_side_to_its_middle() {
        let octagon: Vec<(f64, f64)> = (0..8)
            .map(|k| {
                let angle = std::f64::consts::TAU * f64::from(k) / 8.0;
                (4.0 * angle.cos(), -4.0 * angle.sin())
            })
            .collect();
        let spire = roof("pyramidal", &octagon, Some((8.0, -1.0, false)));
        assert!(spire.tops_m.iter().all(|&top| (top - 2.0).abs() < 1e-3));
        assert!(spire.bends.is_empty());
        let points = corners(&spire);
        // The faces meet at the top over the middle: offsets from the first corner,
        // 4 m east of it.
        assert!(
            points.iter().any(|&(x, y, h)| (x + 4.0).abs() < 1e-3
                && y.abs() < 1e-3
                && (h - 10.0).abs() < 1e-3)
        );
        assert!((covered(&spire) - 32.0 * 2f64.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn a_skillion_falls_toward_its_mapped_direction() {
        // Facing south: the north wall (the last two corners) is the high one.
        let roof = roof(
            "skillion",
            &rectangle(20.0, 10.0),
            Some((3.0, 180.0, false)),
        );
        assert_eq!(roof.tops_m, [7.0, 7.0, 10.0, 10.0]);
        assert!(roof.bends.is_empty());
    }

    #[test]
    fn domes_and_barrels_stay_within_the_block() {
        let circle: Vec<(f64, f64)> = (0..32)
            .map(|k| {
                let angle = std::f64::consts::TAU * f64::from(k) / 32.0;
                (8.0 * angle.cos(), 8.0 * angle.sin())
            })
            .collect();
        for (shape, ring) in [
            ("dome", &circle),
            ("onion", &circle),
            ("round", &rectangle(20.0, 10.0)),
        ] {
            let roof = roof(shape, ring, None);
            let area: f64 = area(
                &ring
                    .iter()
                    .map(|&(x, y)| DVec2::new(x, y))
                    .collect::<Vec<_>>(),
            );
            assert!(
                (covered(&roof) - area).abs() < 1e-3,
                "{shape}: {} of {area}",
                covered(&roof)
            );
            let heights = corners(&roof).into_iter().map(|(_, _, h)| h);
            assert!(
                heights.clone().all(|h| (5.0..=10.0).contains(&h)),
                "{shape}"
            );
            assert!(heights.clone().any(|h| h == 10.0), "{shape}");
        }
    }

    #[test]
    fn flat_unknown_and_low_roofs_stay_flat() {
        let ring = rectangle(20.0, 10.0);
        let footprint = Footprint {
            outline: &ring,
            rings: &[],
            walls: &[],
            metres_per_unit: 1.0,
        };
        for (shape, height_m) in [
            ("flat", None),
            ("crosspitched", None),
            ("gabled", Some(0.1)),
        ] {
            let tags = RoofTags {
                shape: Some(shape),
                height_m,
                direction_deg: None,
                across: false,
            };
            assert!(
                shaped(&tags, &footprint, (0.0, 10.0), |p| p).is_none(),
                "{shape}"
            );
        }
    }

    #[test]
    fn every_shape_covers_an_l_shaped_block_and_meets_its_walls() {
        let ring = vec![
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, -8.0),
            (8.0, -8.0),
            (8.0, -20.0),
            (0.0, -20.0),
        ];
        for shape in [
            "skillion",
            "gabled",
            "half_hipped",
            "hipped",
            "pyramidal",
            "gambrel",
            "mansard",
            "dome",
            "round",
        ] {
            let roof = roof(shape, &ring, None);
            assert!((covered(&roof) - 256.0).abs() < 1e-3, "{shape}");
            // Each corner of the outline is a corner of the roof at the wall's top.
            for (&(x, y), &top) in ring.iter().zip(&roof.tops_m) {
                assert!(
                    corners(&roof).iter().any(|&(east, north, height)| {
                        (east - x).abs() < 1e-6
                            && (north + y).abs() < 1e-6
                            && (height - top).abs() < 1e-4
                    }),
                    "{shape} at {x},{y}"
                );
            }
        }
    }
}
