//! Buildings as blocks (roadmap 8.6): walls and roofs on the ground (raised onto the
//! relief when it is drawn), lit by the sun, as one mesh of triangles drawn from far
//! to near so nearer blocks cover farther ones (there is no depth buffer: the
//! overlays are drawn over the globe's picture).
//!
//! shortcut: faces are ordered by their middles, so where two blocks overlap on
//! screen a long wall can be drawn over a nearer roof; split long faces or draw on
//! the GPU path if that shows in dense cities.
use super::{LiveInput, altitude_fade};
use crate::overlays::{Ground, TerrainVisibility};
use emath::{Pos2, Rect};
use glam::DVec3;
use globecraft_data::buildings::{Building, MAX_ALTITUDE_KM};
use rayon::prelude::*;

/// The blocks to draw: screen points with their colours (RGBA, not premultiplied),
/// and triangles over them, farthest first.
#[derive(Debug, Default)]
pub struct BuildingMesh {
    pub vertices: Vec<(Pos2, [u8; 4])>,
    pub indices: Vec<u32>,
}

/// Walls in the default colour: light stone. Roofs: a little darker.
const WALL: [f32; 3] = [0.86, 0.83, 0.78];
const ROOF: [f32; 3] = [0.74, 0.72, 0.70];
/// Colours mapped in OpenStreetMap are taken this far from the default: mapped
/// colours are often pure ("red", "#000"), and at full strength a city looked like a
/// box of toys (screenshots, 2026-11-16).
const MAPPED_COLOUR_SHARE: f32 = 0.45;
/// No block is darker than this (luminance, 0–1), whatever its mapped colour.
const MIN_LUMINANCE: f32 = 0.35;
/// Light from the sky alone, and from the sun on a face turned to it.
const AMBIENT: f32 = 0.55;
const DIRECT: f32 = 0.45;
/// Without the sun (Sunlight off) blocks are lit from the north-west and above, as
/// relief shading is by convention.
const UNLIT_LIGHT: [f64; 3] = [-0.5, 0.5, 0.7]; // east, north, up
/// Blocks smaller than this on screen (points) are left out: far away in a tilted
/// view, thousands of them would cost more than they show.
const MIN_SIZE_POINTS: f32 = 1.5;
/// The view's edges plus this (points): a block whose screen box misses it is
/// left out.
const VIEW_MARGIN_POINTS: f32 = 32.0;
/// The most faces drawn, the nearest; dense cities seen from 4 km tilted reach
/// several hundred thousand.
const MAX_FACES: usize = 300_000;
const METRES_PER_RADIUS: f64 = globecraft_core::EARTH_RADIUS_M;
/// Blocks projected per parallel job: enough to outweigh a job's cost.
const BLOCKS_PER_JOB: usize = 2_048;

/// One wall or one roof: where its points are in `points`, its triangles in
/// `triangles` (indices into `points`), its colour and its distance squared.
struct Face {
    depth: f64,
    points: std::ops::Range<u32>,
    triangles: std::ops::Range<u32>,
    colour: [u8; 4],
}

pub(super) fn project(input: &LiveInput, ground: Option<&dyn Ground>) -> BuildingMesh {
    let Some(buildings) = &input.buildings else {
        return BuildingMesh::default();
    };
    let view = &input.view;
    let opacity = altitude_fade(MAX_ALTITUDE_KM, view.altitude_km);
    if opacity <= 0.0 {
        return BuildingMesh::default();
    }
    let visibility = TerrainVisibility::new(&view.basis, ground);
    let reach = view.rect.expand(VIEW_MARGIN_POINTS);
    // Blocks are projected on every core, a run of them per job, then joined.
    let runs: Vec<Faces> = buildings
        .par_chunks(BLOCKS_PER_JOB)
        .map(|run| {
            let (mut faces, mut scratch) = (Faces::default(), Scratch::default());
            for building in run {
                block(
                    building,
                    input,
                    ground,
                    visibility.as_ref(),
                    reach,
                    (opacity * 255.0) as u8,
                    &mut scratch,
                    &mut faces,
                );
            }
            faces
        })
        .collect();
    let (mut faces, mut points, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
    for run in runs {
        let (point, triangle) = (points.len() as u32, triangles.len() as u32);
        faces.extend(run.faces.into_iter().map(|face| Face {
            points: face.points.start + point..face.points.end + point,
            triangles: face.triangles.start + triangle..face.triangles.end + triangle,
            ..face
        }));
        points.extend(run.points);
        triangles.extend(run.triangles);
    }
    // The nearest faces, then far to near.
    if faces.len() > MAX_FACES {
        faces.select_nth_unstable_by(MAX_FACES, |a, b| a.depth.total_cmp(&b.depth));
        faces.truncate(MAX_FACES);
    }
    faces.sort_unstable_by(|a, b| b.depth.total_cmp(&a.depth));
    let mut mesh = BuildingMesh {
        vertices: Vec::with_capacity(points.len()),
        indices: Vec::with_capacity(triangles.len()),
    };
    for face in faces {
        let first = mesh.vertices.len() as u32;
        mesh.vertices.extend(
            points[face.points.start as usize..face.points.end as usize]
                .iter()
                .map(|&point| (point, face.colour)),
        );
        mesh.indices.extend(
            triangles[face.triangles.start as usize..face.triangles.end as usize]
                .iter()
                .map(|&index| first + index),
        );
    }
    mesh
}

/// Faces with their points and triangles.
#[derive(Default)]
struct Faces {
    faces: Vec<Face>,
    points: Vec<Pos2>,
    triangles: Vec<u32>,
}

/// Per-block buffers, kept between blocks.
#[derive(Default)]
struct Scratch {
    base: Vec<DVec3>,
    top: Vec<DVec3>,
    base_on_screen: Vec<Option<Pos2>>,
    top_on_screen: Vec<Option<Pos2>>,
}

#[allow(clippy::too_many_arguments)]
fn block(
    building: &Building,
    input: &LiveInput,
    ground: Option<&dyn Ground>,
    visibility: Option<&TerrainVisibility<'_>>,
    reach: Rect,
    alpha: u8,
    scratch: &mut Scratch,
    out: &mut Faces,
) {
    let basis = &input.view.basis;
    let rect = input.view.rect;
    let Some(outline) = building.rings.first().filter(|ring| ring.len() >= 3) else {
        return;
    };
    let count = outline.len() as f64;
    let (lat, lon) = outline.iter().fold((0.0, 0.0), |(lat, lon), &(a, b)| {
        (lat + a / count, lon + b / count)
    });
    if visibility.is_some_and(|visibility| !visibility.visible(lat, lon)) {
        return;
    }
    // One ground height and one "up" for the block: its floor is level.
    let up = basis.surface_position(lat, lon).normalize();
    let floor = ground.map_or(0.0, |ground| ground.height(lat, lon));
    let (base_lift, top_lift) = (
        floor + f64::from(building.base_m) / METRES_PER_RADIUS,
        floor + f64::from(building.top_m) / METRES_PER_RADIUS,
    );
    scratch.base.clear();
    scratch.top.clear();
    scratch.base_on_screen.clear();
    scratch.top_on_screen.clear();
    let mut box_min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut box_max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &(lat, lon) in building.rings.iter().flatten() {
        let surface = basis.surface_position(lat, lon);
        let (base, top) = (surface + up * base_lift, surface + up * top_lift);
        let on_screen = |point: DVec3| {
            basis
                .project_precise(point, rect)
                .map(|at| Pos2::new(at.x as f32, at.y as f32))
        };
        let (base_at, top_at) = (on_screen(base), on_screen(top));
        for at in [base_at, top_at].into_iter().flatten() {
            box_min = box_min.min(at);
            box_max = box_max.max(at);
        }
        scratch.base.push(base);
        scratch.top.push(top);
        scratch.base_on_screen.push(base_at);
        scratch.top_on_screen.push(top_at);
    }
    let size = box_max - box_min;
    if !reach.intersects(Rect::from_min_max(box_min, box_max))
        || size.x.max(size.y) < MIN_SIZE_POINTS
    {
        return;
    }
    let camera = basis.c;
    let light = Light::at(input.sun, up);
    let wall = building
        .wall_colour
        .map_or(WALL, |mapped| toned(WALL, mapped));
    let roof = building
        .roof_colour
        .map_or(ROOF, |mapped| toned(ROOF, mapped));
    // Walls: each faces the right-hand side of its edge (outlines run anticlockwise,
    // courtyards clockwise, seen from above), and only those facing the camera show.
    let mut first = 0;
    for (ring, walls) in building.rings.iter().zip(&building.walls) {
        let n = ring.len();
        let turn = if anticlockwise(ring) == (first == 0) {
            1.0
        } else {
            -1.0
        };
        for (index, &is_wall) in walls.iter().enumerate() {
            let (a, b) = (first + index, first + (index + 1) % n);
            if !is_wall {
                continue;
            }
            let normal = (scratch.base[b] - scratch.base[a])
                .cross(up)
                .normalize_or_zero()
                * turn;
            let middle =
                (scratch.base[a] + scratch.base[b] + scratch.top[a] + scratch.top[b]) / 4.0;
            if normal.dot(camera - middle) <= 0.0 {
                continue;
            }
            let corners = [
                scratch.base_on_screen[a],
                scratch.base_on_screen[b],
                scratch.top_on_screen[b],
                scratch.top_on_screen[a],
            ];
            let Some(corners) = corners.into_iter().collect::<Option<Vec<Pos2>>>() else {
                continue;
            };
            push(
                out,
                (middle - camera).length_squared(),
                &corners,
                &[0, 1, 2, 0, 2, 3],
                colour(wall, light.on(normal), alpha),
            );
        }
        first += n;
    }
    // The roof, when the camera is above it.
    let roof_middle = scratch.top.iter().sum::<DVec3>() / scratch.top.len() as f64;
    if (camera - roof_middle).dot(up) > 0.0
        && let Some(corners) = scratch
            .top_on_screen
            .iter()
            .copied()
            .collect::<Option<Vec<Pos2>>>()
    {
        push(
            out,
            (roof_middle - camera).length_squared(),
            &corners,
            &building.roof,
            colour(roof, light.on(up), alpha),
        );
    }
}

fn push(out: &mut Faces, depth: f64, corners: &[Pos2], triangles: &[u32], colour: [u8; 4]) {
    let (point, triangle) = (out.points.len() as u32, out.triangles.len() as u32);
    out.points.extend_from_slice(corners);
    out.triangles.extend_from_slice(triangles);
    out.faces.push(Face {
        depth,
        points: point..point + corners.len() as u32,
        triangles: triangle..triangle + triangles.len() as u32,
        colour,
    });
}

/// Whether a ring of (latitude, longitude) runs anticlockwise seen from above.
fn anticlockwise(ring: &[(f64, f64)]) -> bool {
    let mut sum = 0.0;
    for (index, &(lat0, lon0)) in ring.iter().enumerate() {
        let (lat1, lon1) = ring[(index + 1) % ring.len()];
        sum += lon0 * lat1 - lon1 * lat0;
    }
    sum > 0.0
}

/// A mapped colour taken `MAPPED_COLOUR_SHARE` of the way from `default`, and
/// lightened to `MIN_LUMINANCE`.
fn toned(default: [f32; 3], mapped: [u8; 3]) -> [f32; 3] {
    let mut colour: [f32; 3] = std::array::from_fn(|channel| {
        let mapped = f32::from(mapped[channel]) / 255.0;
        default[channel] + (mapped - default[channel]) * MAPPED_COLOUR_SHARE
    });
    let luminance = 0.2126 * colour[0] + 0.7152 * colour[1] + 0.0722 * colour[2];
    if luminance < MIN_LUMINANCE {
        let lift = MIN_LUMINANCE - luminance;
        colour = colour.map(|channel| (channel + lift).min(1.0));
    }
    colour
}

fn colour(base: [f32; 3], brightness: f32, alpha: u8) -> [u8; 4] {
    let [r, g, b] = base.map(|channel| (channel * brightness * 255.0).clamp(0.0, 255.0) as u8);
    [r, g, b, alpha]
}

/// How a block is lit: the light's direction and, with the sun, how much daylight
/// there is where it stands (dusk dims blocks as it dims the ground).
struct Light {
    direction: DVec3,
    daylight: f32,
}

impl Light {
    fn at(sun: Option<DVec3>, up: DVec3) -> Self {
        match sun {
            Some(sun) => {
                let k = ((up.dot(sun) + crate::tuning::TWILIGHT_OFFSET)
                    / crate::tuning::TWILIGHT_WIDTH)
                    .clamp(0.0, 1.0) as f32;
                let k = k * k * (3.0 - 2.0 * k);
                Self {
                    direction: sun,
                    daylight: crate::tuning::NIGHT_BRIGHTNESS
                        + (1.0 - crate::tuning::NIGHT_BRIGHTNESS) * k,
                }
            }
            None => {
                let east = DVec3::Z.cross(up).normalize_or(DVec3::X);
                let north = up.cross(east);
                let [e, n, u] = UNLIT_LIGHT;
                Self {
                    direction: (east * e + north * n + up * u).normalize(),
                    daylight: 1.0,
                }
            }
        }
    }
    /// The brightness of a face with this outward normal.
    fn on(&self, normal: DVec3) -> f32 {
        (AMBIENT + DIRECT * normal.dot(self.direction).max(0.0) as f32) * self.daylight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;
    use globecraft_data::buildings::HeightSource;
    use std::sync::Arc;

    /// A block 40 m square and 30 m tall at (lat, lon), its outline anticlockwise.
    fn block_at(lat: f64, lon: f64) -> Building {
        let d = 0.0002;
        Building {
            rings: vec![vec![
                (lat - d, lon - d),
                (lat - d, lon + d),
                (lat + d, lon + d),
                (lat + d, lon - d),
            ]],
            walls: vec![vec![true; 4]],
            roof: vec![0, 1, 2, 0, 2, 3],
            base_m: 0.0,
            top_m: 30.0,
            height: HeightSource::Given,
            roof_colour: None,
            wall_colour: None,
            fade: Default::default(),
        }
    }

    fn view(tilt: f64, heading: f64) -> LiveInput {
        let mut input = super::super::tests::input(Default::default());
        let camera = Cam {
            lat: 48.0,
            lon: 11.0,
            heading,
            alt: 0.6,
            tilt,
        };
        input.view.basis = camera.basis_for_model(1.0, Default::default());
        input.view.altitude_km = camera.alt;
        input.buildings = Some(Arc::from(vec![
            block_at(48.0, 11.0),
            block_at(48.001, 11.0),
        ]));
        input
    }

    #[test]
    fn straight_down_the_block_below_shows_only_its_roof() {
        let mesh = project(&view(0.0, 0.0), None);
        // Two roofs, and the south wall of the block north of the view's centre (seen
        // in perspective); the block below shows no wall.
        assert_eq!(mesh.indices.len(), 3 * 2 * 3, "{mesh:?}");
    }

    #[test]
    fn a_tilted_view_shows_near_walls_and_draws_far_blocks_first() {
        // Looking north from the south: each block shows its south wall and roof,
        // perhaps an east or west wall edge-on; never the north wall.
        let mesh = project(&view(60.0, 0.0), None);
        let faces = mesh.indices.len() / 6;
        assert!((4..=6).contains(&faces), "{faces} faces");
        // The farther block (north, higher on screen) is drawn first.
        let first = mesh.vertices[mesh.indices[0] as usize].0;
        let last = mesh.vertices[*mesh.indices.last().unwrap() as usize].0;
        assert!(first.y < last.y, "{first:?} {last:?}");
    }

    #[test]
    fn sunlit_walls_are_brighter_than_shaded_ones() {
        let up = DVec3::Z;
        let light = Light::at(Some(DVec3::new(0.0, 0.6, 0.8)), up);
        assert!(light.on(DVec3::Y) > light.on(DVec3::NEG_Y));
        // At night the blocks are dim whatever they face.
        let night = Light::at(Some(DVec3::new(0.0, 0.0, -1.0)), up);
        assert!(night.on(DVec3::Y) < 0.2);
    }

    #[test]
    fn mapped_colours_are_toned_down_and_never_black() {
        let red = toned(WALL, [255, 0, 0]);
        assert!(red[0] > red[1] && red[1] > 0.3, "{red:?}");
        let black = toned(WALL, [0, 0, 0]);
        let luminance = 0.2126 * black[0] + 0.7152 * black[1] + 0.0722 * black[2];
        assert!(luminance >= MIN_LUMINANCE - 1e-6, "{black:?}");
    }

    #[test]
    fn blocks_too_high_above_are_not_drawn() {
        let mut input = view(0.0, 0.0);
        input.view.altitude_km = MAX_ALTITUDE_KM + 1.0;
        assert!(project(&input, None).indices.is_empty());
    }
}
