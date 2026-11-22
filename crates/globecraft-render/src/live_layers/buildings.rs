//! Buildings as blocks (roadmap 8.6): walls and roofs on the ground (raised onto the
//! relief when it is drawn), coloured from the imagery under them, lit by the sun,
//! as one mesh of triangles drawn from far
//! to near so nearer blocks cover farther ones (there is no depth buffer: the
//! overlays are drawn over the globe's picture).
//!
//! Faces are ordered by their middles, walls cut into pieces of at most
//! `WALL_PIECE_M` and roofs drawn triangle by triangle, so a long wall no longer
//! covers a nearer block beside it.
//! shortcut: roofs triangulate into long slivers on big buildings, and a sliver's
//! middle can still sort wrongly; a depth buffer (the GPU path) is the real fix.
use super::{LiveInput, altitude_fade};
use crate::overlays::{Ground, TerrainVisibility};
use emath::{Pos2, Rect};
use glam::DVec3;
use globecraft_core::VERTICAL_FOV_HALF_TAN;
use globecraft_data::buildings::{Building, MAX_ALTITUDE_KM};
use globecraft_data::tiles::TileCache;
use rayon::prelude::*;

/// The blocks to draw: screen points with their colours (RGBA, not premultiplied),
/// and triangles over them, farthest first.
#[derive(Debug, Default)]
pub struct BuildingMesh {
    pub vertices: Vec<BuildingVertex>,
    pub indices: Vec<u32>,
    /// The corners as a GPU draws them, laid out with the mesh off the interface's
    /// thread (500,000 corners of a dense city took it milliseconds a frame).
    gpu: Vec<GpuCorner>,
}

/// A corner for a GPU: clip position (x and y, -1 to 1, y up), nearness, colour
/// premultiplied. Sixteen bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuCorner {
    clip: [f32; 3],
    colour: [u8; 4],
}

impl BuildingMesh {
    /// The corners' bytes for a vertex buffer (`GpuCorner`: three `f32`, then four
    /// colour bytes).
    pub fn gpu_vertex_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.gpu)
    }
    /// The triangles' bytes for an index buffer of `u32`.
    pub fn index_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.indices)
    }
}

/// A colour's channels multiplied by its opacity, rounded.
fn premultiplied([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let times = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
    [times(r), times(g), times(b), a]
}

/// A corner of a face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingVertex {
    pub at: Pos2,
    /// 0–1, greater nearer the camera, straight between a face's corners on
    /// screen (the inverse of the depth along the view): what a depth buffer
    /// compares, so a renderer with one need not trust the order of the faces.
    pub nearness: f32,
    pub colour: [u8; 4],
}

/// A projected corner: where it is on screen and the inverse of its depth.
type Corner = (Pos2, f32);

/// Walls in the default colour: light stone. Roofs: a little darker.
const WALL: [f32; 3] = [0.86, 0.83, 0.78];
const ROOF: [f32; 3] = [0.74, 0.72, 0.70];
/// Colours mapped in OpenStreetMap are taken this far from the default: mapped
/// colours are often pure ("red", "#000"), and at full strength a city looked like a
/// box of toys (screenshots, 2026-11-16).
const MAPPED_COLOUR_SHARE: f32 = 0.45;
/// No block is darker than this (luminance, 0–1), whatever its mapped colour.
const MIN_LUMINANCE: f32 = 0.35;
/// Walls without a mapped colour take this share of the imagery's colour under the
/// block: facades are not seen from above, but a block the colour of its street
/// looked more like the place than a white one.
const IMAGERY_WALL_SHARE: f32 = 0.35;
/// The share of a block's width (east–west) its roof colour is averaged over, so
/// the street around it stays out.
const ROOF_SAMPLE_SHARE: f64 = 0.6;
/// No roof taken from imagery is darker than this (luminance, 0–1): roofs in
/// shadow in the photo, or dark slate, came out black (screenshots, 2026-11-16).
const MIN_IMAGERY_LUMINANCE: f32 = 0.2;
/// The coarsest imagery a roof takes its colour from.
const COARSEST_IMAGERY_ZOOM: u8 = globecraft_data::tiles::FINE_IMAGERY_ZOOM;
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
/// Walls are cut into pieces at most this long (metres), each ordered by its own
/// middle: a 100 m wall ordered as one face was drawn over the nearer blocks along
/// it (screenshots, 2026-11-16).
const WALL_PIECE_M: f64 = 15.0;
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

/// The blocks of `input` on screen; with `imagery`, roofs and walls take the colours
/// of its loaded tiles of `input.imagery`.
pub(super) fn project(
    input: &LiveInput,
    ground: Option<&dyn Ground>,
    imagery: Option<&TileCache>,
) -> BuildingMesh {
    let imagery = imagery.zip(input.imagery);
    let Some(snapshot) = &input.buildings else {
        return BuildingMesh::default();
    };
    let view = &input.view;
    let opacity = altitude_fade(MAX_ALTITUDE_KM, view.altitude_km);
    if opacity <= 0.0 {
        return BuildingMesh::default();
    }
    let visibility = TerrainVisibility::new(&view.basis, ground);
    let reach = view.rect.expand(VIEW_MARGIN_POINTS);
    // Blocks are projected on every core, a run of them per job, then joined. A tile
    // just shown fades in, one replaced fades out (`TileFade`), instead of popping.
    let runs: Vec<Faces> = snapshot
        .tiles
        .par_iter()
        .enumerate()
        .filter_map(|(index, tile)| {
            let fade = opacity * snapshot.fade(index).opacity(input.now);
            (fade > 0.0).then_some((tile, fade))
        })
        .flat_map(|(tile, fade)| {
            tile.buildings
                .par_chunks(BLOCKS_PER_JOB)
                .map(move |run| (run, fade))
        })
        .map(|(run, fade)| {
            let (mut faces, mut scratch) = (Faces::default(), Scratch::default());
            for building in run {
                block(
                    building,
                    input,
                    ground,
                    imagery,
                    visibility.as_ref(),
                    reach,
                    (fade * 255.0) as u8,
                    &mut scratch,
                    &mut faces,
                );
            }
            faces
        })
        .collect();
    // Every face with the run it is in; the nearest faces, then far to near. The runs
    // keep their points and triangles where they are, and the mesh is laid out on
    // every core too: done on one, these steps took longer than the projection
    // (17 ms of 38 for 40,000 blocks, 2026-11-18).
    let mut faces: Vec<(u32, &Face)> = runs
        .iter()
        .enumerate()
        .flat_map(|(run, faces)| faces.faces.iter().map(move |face| (run as u32, face)))
        .collect();
    if faces.len() > MAX_FACES {
        faces.select_nth_unstable_by(MAX_FACES, |a, b| a.1.depth.total_cmp(&b.1.depth));
        faces.truncate(MAX_FACES);
    }
    faces.par_sort_unstable_by(|a, b| b.1.depth.total_cmp(&a.1.depth));
    // Where each face's corners start in the mesh.
    let mut firsts = Vec::with_capacity(faces.len());
    let mut corners = 0u32;
    for (_, face) in &faces {
        firsts.push(corners);
        corners += face.points.len() as u32;
    }
    let vertices: Vec<BuildingVertex> = faces
        .par_iter()
        .flat_map_iter(|&(run, face)| {
            runs[run as usize].points[face.points.start as usize..face.points.end as usize]
                .iter()
                .map(|&(at, nearness)| BuildingVertex {
                    at,
                    nearness,
                    colour: face.colour,
                })
        })
        .collect();
    let indices: Vec<u32> = faces
        .par_iter()
        .zip(&firsts)
        .flat_map_iter(|(&(run, face), &first)| {
            // A face's triangles count its own corners from zero.
            runs[run as usize].triangles[face.triangles.start as usize..face.triangles.end as usize]
                .iter()
                .map(move |&index| first + index)
        })
        .collect();
    let mut mesh = BuildingMesh {
        vertices,
        indices,
        gpu: Vec::new(),
    };
    let nearest = mesh
        .vertices
        .par_iter()
        .map(|vertex| vertex.nearness)
        .reduce(|| f32::MIN_POSITIVE, f32::max);
    mesh.vertices
        .par_iter_mut()
        .for_each(|vertex| vertex.nearness /= nearest);
    let (min, span) = (view.rect.min, view.rect.size());
    mesh.gpu = mesh
        .vertices
        .par_iter()
        .map(|vertex| GpuCorner {
            clip: [
                (vertex.at.x - min.x) / span.x * 2.0 - 1.0,
                1.0 - (vertex.at.y - min.y) / span.y * 2.0,
                vertex.nearness,
            ],
            colour: premultiplied(vertex.colour),
        })
        .collect();
    mesh
}

/// Faces with their points and triangles.
#[derive(Default)]
struct Faces {
    faces: Vec<Face>,
    points: Vec<Corner>,
    triangles: Vec<u32>,
}

/// Per-block buffers, kept between blocks.
#[derive(Default)]
struct Scratch {
    base: Vec<DVec3>,
    top: Vec<DVec3>,
    base_on_screen: Vec<Option<Corner>>,
    top_on_screen: Vec<Option<Corner>>,
    shadow: ShadowScratch,
}

/// A block's shadow's buffers, kept from block to block like `Scratch`'s.
#[derive(Default)]
struct ShadowScratch {
    ground: Vec<DVec3>,
    points: Vec<DVec3>,
    flat: Vec<(f64, f64)>,
    order: Vec<usize>,
    hull: Vec<usize>,
    seen: Vec<Option<Corner>>,
}

/// The radius of a sphere round a block's middle that holds it (Earth radii): half its
/// footprint's diagonal and half its height.
fn block_radius(outline: &[(f64, f64)], lat: f64, building: &Building) -> f64 {
    let (mut south, mut north, mut west, mut east) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for &(lat, lon) in outline {
        (south, north) = (south.min(lat), north.max(lat));
        (west, east) = (west.min(lon), east.max(lon));
    }
    let across = (north - south).to_radians();
    let along = (east - west).to_radians() * lat.to_radians().cos();
    let height = f64::from(building.top_m - building.base_m) / METRES_PER_RADIUS;
    0.5 * (across * across + along * along + height * height).sqrt()
}

#[allow(clippy::too_many_arguments)]
fn block(
    building: &Building,
    input: &LiveInput,
    ground: Option<&dyn Ground>,
    imagery: Option<(&TileCache, super::BuildingImagery)>,
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
    // Before its corners are projected: a block wholly behind the camera, off the
    // screen or too small to see is left out by its middle and size alone. Most
    // blocks of a city seen from 1 km are, and each corner costs a projection
    // (user, 2026-11-17: 1 km over New York "a bit laggy").
    let middle = basis.surface_position(lat, lon) + up * (0.5 * (base_lift + top_lift));
    let radius = block_radius(outline, lat, building);
    let depth = basis.view_depth(middle);
    if depth < -radius {
        return;
    }
    if depth > radius {
        let points_per_radian = f64::from(rect.height()) * 0.5 / VERTICAL_FOV_HALF_TAN;
        let size = (2.0 * radius / depth * points_per_radian) as f32;
        let seen = basis.project_precise(middle, rect).is_some_and(|at| {
            reach
                .expand(size)
                .contains(Pos2::new(at.x as f32, at.y as f32))
        });
        // Half the drawn minimum: the middle's size is a lower bound only roughly.
        if !seen || size < 0.5 * MIN_SIZE_POINTS {
            return;
        }
    }
    scratch.base.clear();
    scratch.top.clear();
    scratch.base_on_screen.clear();
    scratch.top_on_screen.clear();
    let mut box_min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut box_max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let on_screen = |point: DVec3| {
        basis.project_precise(point, rect).map(|at| -> Corner {
            (
                Pos2::new(at.x as f32, at.y as f32),
                (1.0 / basis.view_depth(point)) as f32,
            )
        })
    };
    for &(lat, lon) in building.rings.iter().flatten() {
        let surface = basis.surface_position(lat, lon);
        let (base, top) = (surface + up * base_lift, surface + up * top_lift);
        let (base_at, top_at) = (on_screen(base), on_screen(top));
        for (at, _) in [base_at, top_at].into_iter().flatten() {
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
    let (west, east) = outline.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(west, east), &(_, lon)| (west.min(lon), east.max(lon)),
    );
    let photo = imagery.and_then(|(cache, imagery)| {
        let span_deg = (east - west) * ROOF_SAMPLE_SHARE;
        cache
            .loaded_colour(
                imagery.provider,
                imagery.release,
                (lat, lon),
                span_deg,
                COARSEST_IMAGERY_ZOOM,
            )
            .map(|colour| lifted([colour.x, colour.y, colour.z], MIN_IMAGERY_LUMINANCE))
    });
    let wall = match (building.wall_colour, photo) {
        (Some(mapped), _) => toned(WALL, mapped),
        (None, Some(photo)) => std::array::from_fn(|channel| {
            WALL[channel] + (photo[channel] - WALL[channel]) * IMAGERY_WALL_SHARE
        }),
        (None, None) => WALL,
    };
    // The imagery is already lit by the sun of its day; only dusk dims it.
    let (roof, roof_light) = match photo {
        Some(photo) => (photo, light.on_imagery()),
        None => (
            building
                .roof_colour
                .map_or(ROOF, |mapped| toned(ROOF, mapped)),
            light.on(up, up),
        ),
    };
    // Its shadow on the ground, under everything else of the block (C3).
    if let Some(sun) = input.sun {
        let mut buffers = std::mem::take(&mut scratch.shadow);
        buffers.ground.clear();
        buffers.ground.extend(
            outline
                .iter()
                .map(|&(lat, lon)| basis.surface_position(lat, lon) + up * floor),
        );
        let behind = ((middle - camera).length() + radius).powi(2);
        let shade = Shade {
            up,
            height: top_lift - floor,
            sun,
            alpha,
        };
        shadow(out, basis, rect, behind, shade, &mut buffers);
        scratch.shadow = buffers;
    }
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
            let shade = colour(wall, light.on(normal, up), alpha);
            let length_m = (scratch.base[b] - scratch.base[a]).length() * METRES_PER_RADIUS;
            let pieces = (length_m / WALL_PIECE_M).ceil().max(1.0) as usize;
            // Each piece's corners: its ends at the base and the top, on screen.
            let end = |piece: usize| {
                if piece == 0 {
                    return (
                        scratch.base[a],
                        scratch.top[a],
                        scratch.base_on_screen[a],
                        scratch.top_on_screen[a],
                    );
                }
                if piece == pieces {
                    return (
                        scratch.base[b],
                        scratch.top[b],
                        scratch.base_on_screen[b],
                        scratch.top_on_screen[b],
                    );
                }
                let share = piece as f64 / pieces as f64;
                let base = scratch.base[a].lerp(scratch.base[b], share);
                let top = scratch.top[a].lerp(scratch.top[b], share);
                (base, top, on_screen(base), on_screen(top))
            };
            let mut start = end(0);
            for piece in 1..=pieces {
                let stop = end(piece);
                let middle = (start.0 + start.1 + stop.0 + stop.1) / 4.0;
                face(
                    out,
                    basis,
                    rect,
                    (middle - camera).length_squared(),
                    &[start.0, stop.0, stop.1, start.1],
                    &[start.2, stop.2, stop.3, start.3],
                    shade,
                );
                start = stop;
            }
        }
        first += n;
    }
    // The roof, when the camera is above it.
    let roof_middle = scratch.top.iter().sum::<DVec3>() / scratch.top.len() as f64;
    if (camera - roof_middle).dot(up) > 0.0 {
        let shade = colour(roof, roof_light, alpha);
        for triangle in building.roof.chunks_exact(3) {
            let at = |corner: usize| triangle[corner] as usize;
            let world = [scratch.top[at(0)], scratch.top[at(1)], scratch.top[at(2)]];
            face(
                out,
                basis,
                rect,
                ((world[0] + world[1] + world[2]) / 3.0 - camera).length_squared(),
                &world,
                &[0, 1, 2].map(|corner| scratch.top_on_screen[at(corner)]),
                shade,
            );
        }
    }
}

/// Below this sun height (sine of its elevation, about 3°) blocks cast no shadow:
/// the light is too dim and the shadows would stretch across the city.
const SHADOW_MIN_SUN_SINE: f64 = 0.05;
/// A shadow reaches at most this many times the block's height (the sun at 11°).
const SHADOW_MAX_LENGTH_RATIO: f64 = 5.0;
/// How dark a shadow is drawn on the ground (opacity of black, at full daylight).
const SHADOW_OPACITY: f32 = 0.35;

/// Adds the shadow a block of footprint `ground` (its outer ring, on the ground) and
/// `height` (Earth radii) casts with the sun toward `sun`: the hull of the footprint
/// and the footprint moved away from the sun by the shadow's length. Each block's
/// shadow falls on the ground only, not on other blocks; `depth` draws it before the
/// block's own faces.
#[allow(clippy::too_many_arguments)]
fn shadow(
    out: &mut Faces,
    basis: &globecraft_core::camera::Basis,
    rect: Rect,
    depth: f64,
    shade: Shade,
    buffers: &mut ShadowScratch,
) {
    let Shade {
        up,
        height,
        sun,
        alpha,
    } = shade;
    let rise = sun.dot(up);
    if rise < SHADOW_MIN_SUN_SINE || height <= 0.0 {
        return;
    }
    let level = sun - up * rise;
    let length = (height * level.length() / rise).min(height * SHADOW_MAX_LENGTH_RATIO);
    let away = -level.normalize_or_zero() * length;
    // Fades in as the sun rises from its lowest.
    let strength = ((rise - SHADOW_MIN_SUN_SINE) / SHADOW_MIN_SUN_SINE).clamp(0.0, 1.0) as f32;
    let shade = (SHADOW_OPACITY * strength * f32::from(alpha)) as u8;
    if shade == 0 {
        return;
    }
    let ShadowScratch {
        ground,
        points,
        flat,
        order,
        hull,
        seen,
    } = buffers;
    points.clear();
    points.extend(ground.iter().copied());
    points.extend(ground.iter().map(|&point| point + away));
    // In the ground's plane, about the footprint's first corner.
    let east = DVec3::Z.cross(up).normalize_or(DVec3::X);
    let north = up.cross(east);
    flat.clear();
    flat.extend(points.iter().map(|&point| {
        (
            (point - points[0]).dot(east),
            (point - points[0]).dot(north),
        )
    }));
    convex_hull_into(flat, order, hull);
    // Each corner projected once; `face` cuts a triangle reaching behind the camera.
    seen.clear();
    seen.extend(hull.iter().map(|&index| {
        let point = points[index];
        let at = basis.project_precise(point, rect)?;
        Some((
            Pos2::new(at.x as f32, at.y as f32),
            (1.0 / basis.view_depth(point)) as f32,
        ))
    }));
    for index in 1..hull.len().saturating_sub(1) {
        let world = [
            points[hull[0]],
            points[hull[index]],
            points[hull[index + 1]],
        ];
        let seen = [seen[0], seen[index], seen[index + 1]];
        face(out, basis, rect, depth, &world, &seen, [0, 0, 0, shade]);
    }
}

/// What a block's shadow is cast from: its ground's up, its height (Earth radii), the
/// sun and the block's opacity.
#[derive(Clone, Copy)]
struct Shade {
    up: DVec3,
    height: f64,
    sun: DVec3,
    alpha: u8,
}

/// The corners of the convex hull of `points`, anticlockwise (Andrew's monotone
/// chain): indices into `points`.
#[cfg(test)]
fn convex_hull(points: &[(f64, f64)]) -> Vec<usize> {
    let (mut order, mut hull) = (Vec::new(), Vec::new());
    convex_hull_into(points, &mut order, &mut hull);
    hull
}

/// `convex_hull` into `hull`, with `order` for room: both reused.
fn convex_hull_into(points: &[(f64, f64)], order: &mut Vec<usize>, hull: &mut Vec<usize>) {
    order.clear();
    order.extend(0..points.len());
    order.sort_by(|&a, &b| {
        points[a]
            .0
            .total_cmp(&points[b].0)
            .then(points[a].1.total_cmp(&points[b].1))
    });
    let cross = |o: usize, a: usize, b: usize| {
        (points[a].0 - points[o].0) * (points[b].1 - points[o].1)
            - (points[a].1 - points[o].1) * (points[b].0 - points[o].0)
    };
    hull.clear();
    for pass in [false, true] {
        let start = hull.len();
        let sequence: Box<dyn Iterator<Item = &usize>> = if pass {
            Box::new(order.iter().rev())
        } else {
            Box::new(order.iter())
        };
        for &point in sequence {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0
            {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
}

/// Adds the flat, convex face with corners `world` (seen on screen at `seen`, where
/// they project). A face reaching behind the camera is cut at `NEAR_RADII` in
/// front of it: left out whole, the walls and roofs beside the camera went
/// missing at street level and the blocks behind showed through (user's
/// recording, 2026-11-16).
fn face(
    out: &mut Faces,
    basis: &globecraft_core::camera::Basis,
    rect: Rect,
    depth: f64,
    world: &[DVec3],
    seen: &[Option<Corner>],
    colour: [u8; 4],
) {
    /// Two metres in front of the camera.
    const NEAR_RADII: f64 = 2.0 / METRES_PER_RADIUS;
    /// A quadrilateral cut by a plane has at most five corners.
    const FAN: [u32; 9] = [0, 1, 2, 0, 2, 3, 0, 3, 4];
    let mut corners: Vec<Corner> = Vec::with_capacity(5);
    if seen.iter().all(Option::is_some) {
        corners.extend(seen.iter().flatten());
    } else {
        let project = |point: DVec3| -> Option<Corner> {
            let at = basis.project_precise(point, rect)?;
            Some((
                Pos2::new(at.x as f32, at.y as f32),
                (1.0 / basis.view_depth(point)) as f32,
            ))
        };
        for (index, &from) in world.iter().enumerate() {
            let to = world[(index + 1) % world.len()];
            let (ahead_from, ahead_to) = (
                basis.view_depth(from) - NEAR_RADII,
                basis.view_depth(to) - NEAR_RADII,
            );
            if ahead_from >= 0.0 {
                corners.extend(project(from));
            }
            if (ahead_from >= 0.0) != (ahead_to >= 0.0) {
                let cut = from.lerp(to, ahead_from / (ahead_from - ahead_to));
                corners.extend(project(cut));
            }
        }
    }
    if corners.len() >= 3 {
        push(
            out,
            depth,
            &corners,
            &FAN[..(corners.len() - 2) * 3],
            colour,
        );
    }
}

fn push(out: &mut Faces, depth: f64, corners: &[Corner], triangles: &[u32], colour: [u8; 4]) {
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
    let colour: [f32; 3] = std::array::from_fn(|channel| {
        let mapped = f32::from(mapped[channel]) / 255.0;
        default[channel] + (mapped - default[channel]) * MAPPED_COLOUR_SHARE
    });
    lifted(colour, MIN_LUMINANCE)
}

/// `colour` lightened to at least `min_luminance`.
fn lifted(colour: [f32; 3], min_luminance: f32) -> [f32; 3] {
    let luminance = 0.2126 * colour[0] + 0.7152 * colour[1] + 0.0722 * colour[2];
    let lift = (min_luminance - luminance).max(0.0);
    colour.map(|channel| (channel + lift).min(1.0))
}

fn colour(base: [f32; 3], light: [f32; 3], alpha: u8) -> [u8; 4] {
    let [r, g, b] = std::array::from_fn(|i| (base[i] * light[i] * 255.0).clamp(0.0, 255.0) as u8);
    [r, g, b, alpha]
}

/// How a block is lit: with the sun as the ground is (`pixel::Daylight`, the sky's
/// light and the sun's apart, so walls facing a low sun glow and the others go
/// blue), without it from a fixed light.
struct Light {
    direction: DVec3,
    sun: Option<(crate::pixel::Daylight, glam::Vec4)>,
}

impl Light {
    fn at(sun: Option<DVec3>, up: DVec3) -> Self {
        match sun {
            Some(sun) => {
                let cosine = up.dot(sun);
                Self {
                    direction: sun,
                    sun: Some((
                        crate::pixel::Daylight::at(cosine),
                        crate::atmosphere::ground_sunlight(cosine as f32).powf(2.0),
                    )),
                }
            }
            None => {
                let east = DVec3::Z.cross(up).normalize_or(DVec3::X);
                let north = up.cross(east);
                let [e, n, u] = UNLIT_LIGHT;
                Self {
                    direction: (east * e + north * n + up * u).normalize(),
                    sun: None,
                }
            }
        }
    }
    /// The light on a face with this outward normal (`up` for a roof).
    fn on(&self, normal: DVec3, up: DVec3) -> [f32; 3] {
        let cosine = normal.dot(self.direction);
        match self.sun {
            Some((daylight, tint)) => {
                // A wall sees half the sky; the sky's light is mostly from above.
                let sky = (0.5 + 0.5 * normal.dot(up)) as f32;
                let gain = crate::pixel::Daylight::gain(cosine, daylight.sun_cosine);
                daylight.light(tint, gain, 1.0, sky).truncate().to_array()
            }
            None => [AMBIENT + DIRECT * cosine.max(0.0) as f32; 3],
        }
    }
    /// The light on imagery of a roof, which shows the sun of its own day already: the
    /// ground's under the same sun (none without it).
    fn on_imagery(&self) -> [f32; 3] {
        self.sun.map_or([1.0; 3], |(daylight, tint)| {
            daylight
                .light(tint, daylight.flat_gain, 1.0, 1.0)
                .truncate()
                .to_array()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;
    use globecraft_data::buildings::{BuildingSnapshot, BuildingTile};
    use std::sync::Arc;

    /// `buildings` as one tile.
    fn tiles(buildings: Vec<Building>) -> Option<Arc<BuildingSnapshot>> {
        Some(Arc::new(BuildingSnapshot {
            tiles: vec![Arc::new(BuildingTile { buildings })],
            zoom: 14,
            fades: Vec::new(),
        }))
    }

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
            roof_colour: None,
            wall_colour: None,
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
            look_up: 0.0,
        };
        input.view.basis = camera.basis_for_model(1.0, Default::default());
        input.view.altitude_km = camera.alt;
        input.buildings = tiles(vec![block_at(48.0, 11.0), block_at(48.001, 11.0)]);
        input
    }

    /// How long the blocks of a dense city take to project: 40,000 blocks 60 m apart
    /// (a 12 km square, as at 1 km over Manhattan), seen from 1 km at a 70° tilt.
    /// Manual: run with `--ignored --nocapture` in release.
    #[test]
    #[ignore = "manual timing"]
    fn dense_city_projection_time() {
        let mut input = view(70.0, 30.0);
        let camera = Cam {
            lat: 40.75,
            lon: -73.98,
            heading: 30.0,
            alt: 1.0,
            tilt: 70.0,
            look_up: 0.0,
        };
        input.view.basis = camera.basis_for_model(1.6, Default::default());
        input.view.altitude_km = camera.alt;
        let step = 0.000_55;
        let blocks = (0..200)
            .flat_map(|row| (0..200).map(move |column| (row, column)))
            .map(|(row, column)| {
                block_at(
                    40.70 + f64::from(row) * step,
                    -74.04 + f64::from(column) * step * 1.3,
                )
            })
            .collect();
        input.buildings = tiles(blocks);
        // An afternoon sun, so the blocks cast shadows.
        input.sun = Some(
            (globecraft_core::math::ll_to_v3(40.75, -73.98) + glam::DVec3::new(0.3, -0.5, -0.2))
                .normalize(),
        );
        let _ = project(&input, None, None);
        let start = std::time::Instant::now();
        let runs = 10;
        let mut triangles = 0;
        for _ in 0..runs {
            triangles = project(&input, None, None).indices.len() / 3;
        }
        println!(
            "dense city: {:?} per projection, {triangles} triangles, {} threads",
            start.elapsed() / runs,
            rayon::current_num_threads()
        );
    }

    #[test]
    fn the_hull_keeps_the_outside_corners_only() {
        let points = [
            (0.0, 0.0),
            (2.0, 0.0),
            (1.0, 1.0),
            (2.0, 2.0),
            (0.0, 2.0),
            (1.0, 0.0),
        ];
        let mut hull = convex_hull(&points);
        hull.sort_unstable();
        assert_eq!(hull, vec![0, 1, 3, 4]);
    }

    #[test]
    fn a_block_casts_its_shadow_away_from_the_sun() {
        // Straight down at noon with the sun 45° high in the south: the shadow lies
        // north of the block (up the screen), as long as the block is tall (30 m).
        let mut input = view(0.0, 0.0);
        input.buildings = tiles(vec![block_at(48.0, 11.0)]);
        let up = globecraft_core::math::ll_to_v3(48.0, 11.0);
        let (la, lo) = (48f64.to_radians(), 11f64.to_radians());
        let north = DVec3::new(-la.sin() * lo.cos(), -la.sin() * lo.sin(), la.cos());
        input.sun = Some((up - north).normalize());
        let lit = project(&input, None, None);
        let dark: Vec<&BuildingVertex> = lit
            .vertices
            .iter()
            .filter(|vertex| vertex.colour[..3] == [0, 0, 0] && vertex.colour[3] > 0)
            .collect();
        assert!(dark.len() >= 3, "{lit:?}");
        let roof_top = lit
            .vertices
            .iter()
            .filter(|vertex| vertex.colour[..3] != [0, 0, 0])
            .map(|vertex| vertex.at.y)
            .fold(f32::INFINITY, f32::min);
        let shadow_top = dark
            .iter()
            .map(|vertex| vertex.at.y)
            .fold(f32::INFINITY, f32::min);
        assert!(shadow_top < roof_top, "{shadow_top} {roof_top}");
        // Drawn before the block.
        assert_eq!(lit.vertices[lit.indices[0] as usize].colour[..3], [0, 0, 0]);
        // At night, none.
        input.sun = Some(-up);
        let night = project(&input, None, None);
        assert!(
            night
                .vertices
                .iter()
                .all(|vertex| vertex.colour[..3] != [0, 0, 0])
        );
    }

    #[test]
    fn a_new_tile_fades_in_and_a_replaced_one_out() {
        use globecraft_data::vector_tiles::TileFade;
        let mut input = view(0.0, 0.0);
        let start = input.now;
        let blocks = input.buildings.as_ref().unwrap().tiles.clone();
        let fading = |fade: TileFade| {
            Some(Arc::new(BuildingSnapshot {
                tiles: blocks.clone(),
                zoom: 14,
                fades: vec![fade],
            }))
        };
        let alpha = |input: &LiveInput| {
            let mesh = project(input, None, None);
            mesh.vertices.iter().map(|v| v.colour[3]).max().unwrap_or(0)
        };
        let full = alpha(&input);
        assert!(full > 200, "{full}");
        let shown = TileFade {
            shown: Some(start),
            retired: None,
        };
        input.buildings = fading(shown);
        assert_eq!(alpha(&input), 0, "just shown");
        assert!(input.buildings_fading());
        let mut later = input.clone();
        later.now = start + std::time::Duration::from_millis(225);
        let half = alpha(&later);
        assert!(0 < half && half < full, "{half}");
        // Its moment matters while it fades, so the blocks are projected again.
        assert!(!input.same_request(&later));
        later.now = start + std::time::Duration::from_secs(1);
        assert_eq!(alpha(&later), full);
        let retired = TileFade {
            shown: None,
            retired: Some(start),
        };
        later.buildings = fading(retired);
        assert_eq!(alpha(&later), 0, "gone");
    }

    #[test]
    fn the_gpu_corners_are_the_mesh_in_clip_space() {
        let input = view(60.0, 0.0);
        let mesh = project(&input, None, None);
        assert_eq!(mesh.gpu.len(), mesh.vertices.len());
        assert_eq!(mesh.gpu_vertex_bytes().len(), 16 * mesh.vertices.len());
        assert_eq!(mesh.index_bytes().len(), 4 * mesh.indices.len());
        let rect = input.view.rect;
        for (corner, vertex) in mesh.gpu.iter().zip(&mesh.vertices) {
            let x = rect.min.x + (corner.clip[0] + 1.0) / 2.0 * rect.width();
            let y = rect.min.y + (1.0 - corner.clip[1]) / 2.0 * rect.height();
            assert!((x - vertex.at.x).abs() < 1e-3 && (y - vertex.at.y).abs() < 1e-3);
            assert_eq!(corner.clip[2], vertex.nearness);
        }
        assert_eq!(premultiplied([200, 100, 50, 128]), [100, 50, 25, 128]);
        assert_eq!(premultiplied([200, 100, 50, 255]), [200, 100, 50, 255]);
    }

    #[test]
    fn straight_down_the_block_below_shows_only_its_roof() {
        let mesh = project(&view(0.0, 0.0), None, None);
        // Two roofs (two triangles each), and the south wall of the block north of the
        // view's centre (seen in perspective; 22 m long, so two pieces of two
        // triangles); the block below shows no wall.
        assert_eq!(mesh.indices.len(), 3 * (2 * 2 + 2 * 2), "{mesh:?}");
    }

    #[test]
    fn a_tilted_view_shows_near_walls_and_draws_far_blocks_first() {
        // Looking north from the south: each block shows its south wall and roof,
        // perhaps an east or west wall edge-on; never the north wall.
        let mesh = project(&view(60.0, 0.0), None, None);
        let triangles = mesh.indices.len() / 3;
        assert!((8..=16).contains(&triangles), "{triangles} triangles");
        // The farther block (north, higher on screen) is drawn first.
        let first = mesh.vertices[mesh.indices[0] as usize].at;
        let last = mesh.vertices[*mesh.indices.last().unwrap() as usize].at;
        assert!(first.y < last.y, "{first:?} {last:?}");
        // And for a depth buffer: nearer corners have the greater nearness, up to one.
        let nearness = |index: u32| mesh.vertices[index as usize].nearness;
        assert!(nearness(mesh.indices[0]) < nearness(*mesh.indices.last().unwrap()));
        let nearest = mesh.vertices.iter().map(|v| v.nearness).fold(0.0, f32::max);
        assert_eq!(nearest, 1.0);
    }

    #[test]
    fn a_long_wall_behind_a_small_block_is_drawn_before_it() {
        // Looking north: a 600 m slab ahead, and a small red block in front of its
        // east end. Ordered as one face, the slab's wall (its middle straight ahead)
        // was nearer than the small block and was drawn over it.
        let mut input = view(60.0, 0.0);
        let mut slab = block_at(48.002, 11.0);
        slab.rings = vec![vec![
            (48.0019, 10.996),
            (48.0019, 11.004),
            (48.0021, 11.004),
            (48.0021, 10.996),
        ]];
        let mut small = block_at(48.0017, 11.0033);
        small.top_m = 6.0;
        small.wall_colour = Some([255, 0, 0]);
        small.roof_colour = Some([255, 0, 0]);
        input.buildings = tiles(vec![slab, small]);
        let mesh = project(&input, None, None);
        let red = |index: u32| {
            let [r, g, ..] = mesh.vertices[index as usize].colour;
            r > g + 20
        };
        let triangles: Vec<&[u32]> = mesh.indices.chunks_exact(3).collect();
        let first_red = triangles
            .iter()
            .position(|t| red(t[0]))
            .expect("small block drawn");
        let red_box = triangles
            .iter()
            .filter(|t| red(t[0]))
            .flat_map(|t| t.iter().map(|&index| mesh.vertices[index as usize].at))
            .fold(Rect::NOTHING, |rect, at| {
                rect.union(Rect::from_min_max(at, at))
            });
        for triangle in &triangles[first_red..] {
            let points = triangle
                .iter()
                .map(|&index| mesh.vertices[index as usize].at);
            let bounds = points.fold(Rect::NOTHING, |rect, at| {
                rect.union(Rect::from_min_max(at, at))
            });
            assert!(
                red(triangle[0]) || !bounds.intersects(red_box.shrink(1.0)),
                "slab drawn over the small block in front of it: {bounds:?} {:?} over {red_box:?}",
                mesh.vertices[triangle[0] as usize].colour
            );
        }
    }

    #[test]
    fn blocks_are_lit_again_whenever_the_sun_steps() {
        let mut preparer = super::super::LivePreparer::default();
        let mut input = view(60.0, 0.0);
        input.sun = Some(DVec3::Z);
        let first = preparer.prepare(input.clone());
        // Another layer's step keeps the blocks.
        input.unix_secs += 5.0;
        let kept = preparer.prepare(input.clone());
        assert!(Arc::ptr_eq(&first.buildings, &kept.buildings));
        // However small the sun's step (once compared with the request before,
        // small steps added up without ever lighting the blocks again).
        input.sun = Some(DVec3::new(0.0, 0.001, 1.0).normalize());
        let lit = preparer.prepare(input);
        assert!(!Arc::ptr_eq(&first.buildings, &lit.buildings));
    }

    #[test]
    fn a_wall_reaching_behind_the_camera_is_cut_not_dropped() {
        // At street level, looking north along a long wall that starts behind the
        // camera: its near end does not project, and the wall was left out.
        let mut input = view(80.0, 0.0);
        input.view.altitude_km = 0.03;
        input.view.basis = Cam {
            lat: 48.0,
            lon: 11.0,
            heading: 0.0,
            alt: 0.03,
            tilt: 80.0,
            look_up: 0.0,
        }
        .basis(1.5);
        // The camera stands 29.5 m south of the point it looks at; the block's
        // west wall, two metres east of the view line, runs from 10 m behind the
        // camera to 8 m ahead of it.
        let mut slab = block_at(48.0, 11.0);
        slab.rings = vec![vec![
            (47.99964, 11.00003),
            (47.99964, 11.00013),
            (47.999807, 11.00013),
            (47.999807, 11.00003),
        ]];
        input.buildings = tiles(vec![slab]);
        let cut = project(&input, None, None);
        assert!(
            !cut.indices.is_empty(),
            "the wall beside the camera is drawn"
        );
        assert!(cut.vertices.iter().all(|vertex| vertex.at.is_finite()));
        assert!(cut.vertices.iter().all(|vertex| vertex.nearness > 0.0));
    }

    #[test]
    fn sunlit_walls_are_brighter_than_shaded_ones() {
        let up = DVec3::Z;
        let light = Light::at(Some(DVec3::new(0.0, 0.6, 0.8)), up);
        assert!(light.on(DVec3::Y, up)[0] > light.on(DVec3::NEG_Y, up)[0]);
        // At night the blocks are dim whatever they face.
        let night = Light::at(Some(DVec3::new(0.0, 0.0, -1.0)), up);
        assert!(night.on(DVec3::Y, up).iter().all(|&c| c < 0.2));
        // A low sun: the wall facing it glows warm, the one turned away goes blue.
        let low = Light::at(Some(DVec3::new(0.0, 0.996, 0.087)), up);
        let [r, _, b] = low.on(DVec3::Y, up);
        assert!(r > b, "lit {r} {b}");
        let [r, _, b] = low.on(DVec3::NEG_Y, up);
        assert!(b > r, "shaded {r} {b}");
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
        assert!(project(&input, None, None).indices.is_empty());
    }
}
