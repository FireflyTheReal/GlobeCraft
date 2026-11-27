//! Buildings as blocks (roadmap 8.6): walls and roofs on the ground (raised onto the
//! relief when it is drawn), coloured from the imagery under them, lit by the sun,
//! as one mesh of triangles drawn from far
//! to near so nearer blocks cover farther ones (there is no depth buffer: the
//! overlays are drawn over the globe's picture).
//!
//! Faces are ordered by their middles, walls cut into pieces of at most
//! `WALL_PIECE_M` and roofs drawn triangle by triangle, so a long wall no longer
//! covers a nearer block beside it.
//! Shaped roofs (`ShapedRoof`, D3) are drawn triangle by triangle, each lit by its
//! slope, and walls under them rise to the roof, cut where it creases.
//! shortcut: roofs triangulate into long slivers on big buildings, and a sliver's
//! middle can still sort wrongly; a depth buffer (the GPU path) is the real fix.
use super::{LiveInput, altitude_fade};
use crate::overlays::{Ground, TerrainVisibility};
use emath::{Pos2, Rect};
use glam::DVec3;
use globecraft_core::VERTICAL_FOV_HALF_TAN;
use globecraft_data::buildings::{Building, MAX_ALTITUDE_KM, WallBend};
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
/// premultiplied, and where the frame's picture shows the ground under a roof
/// (`BuildingVertex::photo`; `NO_PHOTO` elsewhere). Twenty bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuCorner {
    clip: [f32; 3],
    colour: [u8; 4],
    photo: [u16; 2],
}

/// A corner's `photo` where it takes its colour alone: walls, shadows, and roofs
/// where the frame is no photograph. Real ones reach at most `PHOTO_SCALE`.
pub const NO_PHOTO: [u16; 2] = [u16::MAX; 2];
/// `BuildingVertex::photo` at the frame's right or bottom edge.
pub const PHOTO_SCALE: f32 = (u16::MAX - 1) as f32;

impl BuildingMesh {
    /// The corners' bytes for a vertex buffer (`GpuCorner`: three `f32`, four colour
    /// bytes, then two `u16` of the photo's place).
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
    /// A roof's corner on an imagery basemap: where the frame's picture shows the
    /// ground under it, from its left and top (0) to its right and bottom
    /// (`PHOTO_SCALE`); `NO_PHOTO` elsewhere. A renderer that can sample the frame
    /// draws the roof as that photograph, times `tint`, in place of `colour`.
    pub photo: [u16; 2],
    /// The light on a photographed roof (dusk and the sun are in the frame already:
    /// only the shade of other blocks and the slope), with the block's opacity.
    pub tint: [u8; 4],
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
/// Blocks projected per parallel job: enough to outweigh a job's cost.
const BLOCKS_PER_JOB: usize = 2_048;

/// One wall or one roof: where its points are in `points`, its triangles in
/// `triangles` (indices into `points`), its colour and its distance squared.
struct Face {
    depth: f64,
    points: std::ops::Range<u32>,
    triangles: std::ops::Range<u32>,
    colour: [u8; 4],
    /// A photographed roof's light (`BuildingVertex::tint`).
    tint: [u8; 4],
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
    let visibility = TerrainVisibility::new(&view.basis, ground)
        .map(|visibility| visibility.with_depth(input.depth.as_deref(), view.rect));
    let reach = view.rect.expand(VIEW_MARGIN_POINTS);
    // Which part of each wall and roof the sun reaches past the other blocks (C3b).
    let march = input
        .sun
        .map(|sun| super::sun_march::SunMarch::new(&snapshot.tiles, sun));
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
                    march.as_ref(),
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
            let range = face.points.start as usize..face.points.end as usize;
            let run = &runs[run as usize];
            run.points[range.clone()].iter().zip(&run.photo[range]).map(
                |(&(at, nearness), &photo)| BuildingVertex {
                    at,
                    nearness,
                    colour: face.colour,
                    photo,
                    tint: face.tint,
                },
            )
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
            colour: premultiplied(if vertex.photo == NO_PHOTO {
                vertex.colour
            } else {
                vertex.tint
            }),
            photo: vertex.photo,
        })
        .collect();
    mesh
}

/// Faces with their points and triangles.
#[derive(Default)]
struct Faces {
    faces: Vec<Face>,
    points: Vec<Corner>,
    /// Each point's place in the frame's picture (`BuildingVertex::photo`).
    photo: Vec<[u16; 2]>,
    triangles: Vec<u32>,
}

/// Per-block buffers, kept between blocks.
#[derive(Default)]
struct Scratch {
    base: Vec<DVec3>,
    top: Vec<DVec3>,
    base_on_screen: Vec<Option<Corner>>,
    top_on_screen: Vec<Option<Corner>>,
    /// A shaped roof's points and where they are on screen.
    roof: Vec<DVec3>,
    roof_on_screen: Vec<Option<Corner>>,
    /// The ground under each point of the rings, then of a shaped roof: where a
    /// photographed roof's corner takes its picture from.
    ground: Vec<DVec3>,
    roof_ground: Vec<DVec3>,
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
    march: Option<&super::sun_march::SunMarch<'_>>,
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
    scratch.ground.clear();
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
    let shaped = building.shaped.as_deref();
    for (index, &(lat, lon)) in building.rings.iter().flatten().enumerate() {
        let surface = basis.surface_position(lat, lon);
        // Under a shaped roof each wall's top is the roof's height there.
        let top_lift = shaped.map_or(top_lift, |roof| {
            floor + f64::from(roof.tops_m[index]) / METRES_PER_RADIUS
        });
        let (base, top) = (surface + up * base_lift, surface + up * top_lift);
        scratch.ground.push(surface + up * floor);
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
    let light = Light::at(input.sun, up, top_lift);
    // Where the sun reaches the block, only where its light comes in at all.
    let far = (middle - camera).length() * METRES_PER_RADIUS > super::sun_march::ONE_MARCH_BEYOND_M;
    let sunlit = march
        .filter(|_| light.has_sun())
        .map(|march| march.block(building, (lat, lon), up, far));
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
    // The roof is in the sun when no block toward it rises above its line.
    let roof_seen = match &sunlit {
        Some(sunlit) if !sunlit.roof => 0.0,
        _ => 1.0,
    };
    // A photographed roof shows the frame, lit as the ground is: only the shade of
    // other blocks is its own.
    let shaded: [f32; 3] = {
        let (shade, sun) = (light.on_imagery(roof_seen), light.on_imagery(1.0));
        std::array::from_fn(|channel| shade[channel] / sun[channel].max(MIN_LIGHT))
    };
    // The imagery is already lit by the sun of its day; only dusk dims it.
    let (roof, roof_light) = match photo {
        Some(photo) => (photo, light.on_imagery(roof_seen)),
        None => (
            building
                .roof_colour
                .map_or(ROOF, |mapped| toned(ROOF, mapped)),
            light.on(up, up, roof_seen),
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
    // Their pieces are counted through `sunlit.walls` in the march's order; under a
    // shaped roof a piece is cut again where the roof above it creases.
    let mut first = 0;
    let mut piece_index = 0;
    let (base_m, top_m) = (f64::from(building.base_m), f64::from(building.top_m));
    let all_bends = shaped.map_or(&[][..], |roof| &roof.bends[..]);
    let mut bend_from = 0;
    for (ring, walls) in building.rings.iter().zip(&building.walls) {
        let n = ring.len();
        let turn = super::sun_march::wall_turn(ring, first == 0);
        for (index, &is_wall) in walls.iter().enumerate() {
            let (a, b) = (first + index, first + (index + 1) % n);
            if !is_wall {
                continue;
            }
            let pieces = super::sun_march::wall_pieces(ring[index], ring[(index + 1) % n]);
            let lit_from = piece_index;
            piece_index += pieces;
            // This edge's bends: the bends run edge by edge.
            while all_bends
                .get(bend_from)
                .is_some_and(|bend| (bend.edge as usize) < a)
            {
                bend_from += 1;
            }
            let bend_to = bend_from
                + all_bends[bend_from..]
                    .iter()
                    .take_while(|bend| bend.edge as usize == a)
                    .count();
            let bends = &all_bends[bend_from..bend_to];
            bend_from = bend_to;
            let normal = (scratch.base[b] - scratch.base[a])
                .cross(up)
                .normalize_or_zero()
                * turn;
            let middle =
                (scratch.base[a] + scratch.base[b] + scratch.top[a] + scratch.top[b]) / 4.0;
            if normal.dot(camera - middle) <= 0.0 {
                continue;
            }
            let lit = colour(wall, light.on(normal, up, 1.0), alpha);
            let dark = colour(wall, light.on(normal, up, 0.0), alpha);
            let tops_m = shaped.map_or((top_m, top_m), |roof| {
                (f64::from(roof.tops_m[a]), f64::from(roof.tops_m[b]))
            });
            // A point along the edge at its base and its top, on screen too.
            let end = |share: f64| {
                if share == 0.0 || share == 1.0 {
                    let (at, top_m) = if share == 0.0 {
                        (a, tops_m.0)
                    } else {
                        (b, tops_m.1)
                    };
                    return WallEnd {
                        base: scratch.base[at],
                        top: scratch.top[at],
                        base_seen: scratch.base_on_screen[at],
                        top_seen: scratch.top_on_screen[at],
                        top_m,
                    };
                }
                let base = scratch.base[a].lerp(scratch.base[b], share);
                let height_m = wall_top_m(share, tops_m, bends);
                let top = base + up * ((height_m - base_m) / METRES_PER_RADIUS);
                WallEnd {
                    base,
                    top,
                    base_seen: on_screen(base),
                    top_seen: on_screen(top),
                    top_m: height_m,
                }
            };
            let mut start = end(0.0);
            let mut next_bend = 0;
            for piece in 1..=pieces {
                // The height up from which the sun reaches the piece.
                let lit_m = sunlit
                    .as_ref()
                    .map(|sunlit| sunlit.walls[lit_from + piece - 1]);
                let piece_end = piece as f64 / pieces as f64;
                loop {
                    let share = match bends.get(next_bend) {
                        Some(bend) if f64::from(bend.share) < piece_end => {
                            next_bend += 1;
                            f64::from(bend.share)
                        }
                        _ => piece_end,
                    };
                    let stop = end(share);
                    // The share of each end's height in the shade, below `lit_m`.
                    let shaded = |end: &WallEnd| {
                        // A wall of no height (a roof down to the base) is all shade.
                        lit_m.map_or(0.0, |lit_m| {
                            ((lit_m - base_m) / (end.top_m - base_m).max(MIN_WALL_M))
                                .clamp(0.0, 1.0)
                        })
                    };
                    let shade = [shaded(&start), shaded(&stop)];
                    let mut quad = |from: [f64; 2], to: [f64; 2], colour: [u8; 4]| {
                        let world = [
                            start.base.lerp(start.top, from[0]),
                            stop.base.lerp(stop.top, from[1]),
                            stop.base.lerp(stop.top, to[1]),
                            start.base.lerp(start.top, to[0]),
                        ];
                        let seen = if from == [0.0; 2] && to == [1.0; 2] {
                            [
                                start.base_seen,
                                stop.base_seen,
                                stop.top_seen,
                                start.top_seen,
                            ]
                        } else {
                            world.map(on_screen)
                        };
                        let middle = world.iter().sum::<DVec3>() / 4.0;
                        face(
                            out,
                            basis,
                            rect,
                            (middle - camera).length_squared(),
                            &world,
                            &seen,
                            colour.into(),
                        );
                    };
                    // In the shade below that height, in the sun above it.
                    if shade.iter().any(|&share| share > 0.0) {
                        quad([0.0; 2], shade, dark);
                    }
                    if shade.iter().any(|&share| share < 1.0) {
                        quad(shade, [1.0; 2], lit);
                    }
                    start = stop;
                    if share == piece_end {
                        break;
                    }
                }
            }
        }
        first += n;
    }
    match shaped {
        Some(shaped) => {
            let flat = light.on(up, up, roof_seen);
            let (first_lat, first_lon) = outline[0];
            scratch.roof.clear();
            scratch
                .roof
                .extend(shaped.points.iter().map(|&[lat, lon, height_m]| {
                    basis.surface_position(first_lat + f64::from(lat), first_lon + f64::from(lon))
                        + up * (floor + f64::from(height_m) / METRES_PER_RADIUS)
                }));
            scratch.roof_on_screen.clear();
            scratch
                .roof_on_screen
                .extend(scratch.roof.iter().map(|&point| on_screen(point)));
            scratch.roof_ground.clear();
            if photo.is_some() {
                scratch
                    .roof_ground
                    .extend(shaped.points.iter().map(|&[lat, lon, _]| {
                        basis.surface_position(
                            first_lat + f64::from(lat),
                            first_lon + f64::from(lon),
                        ) + up * floor
                    }));
            }
            for triangle in shaped.triangles.chunks_exact(3) {
                let at = |corner: usize| triangle[corner] as usize;
                let world = [0, 1, 2].map(|corner| scratch.roof[at(corner)]);
                let normal = (world[1] - world[0])
                    .cross(world[2] - world[0])
                    .normalize_or(up);
                let normal = if normal.dot(up) < 0.0 {
                    -normal
                } else {
                    normal
                };
                // Only slopes turned to the camera show.
                if normal.dot(camera - world[0]) <= 0.0 {
                    continue;
                }
                let on_slope = light.on(normal, up, roof_seen);
                // Imagery is lit already: a slope takes only its light against a flat
                // roof's.
                let lighting: [f32; 3] = match photo {
                    Some(_) => std::array::from_fn(|channel| {
                        roof_light[channel] * on_slope[channel] / flat[channel].max(MIN_LIGHT)
                    }),
                    None => on_slope,
                };
                let ground = photo.map(|_| [0, 1, 2].map(|corner| scratch.roof_ground[at(corner)]));
                let tint: [f32; 3] = std::array::from_fn(|channel| {
                    shaded[channel] * on_slope[channel] / flat[channel].max(MIN_LIGHT)
                });
                face(
                    out,
                    basis,
                    rect,
                    ((world[0] + world[1] + world[2]) / 3.0 - camera).length_squared(),
                    &world,
                    &[0, 1, 2].map(|corner| scratch.roof_on_screen[at(corner)]),
                    Paint {
                        colour: colour(roof, lighting, alpha),
                        photo: ground.as_ref().map(|ground| Photo {
                            ground,
                            tint: colour([1.0; 3], tint, alpha),
                        }),
                    },
                );
            }
        }
        // A flat roof, when the camera is above it.
        None => {
            let roof_middle = scratch.top.iter().sum::<DVec3>() / scratch.top.len() as f64;
            if (camera - roof_middle).dot(up) > 0.0 {
                let shade = colour(roof, roof_light, alpha);
                let tint = colour([1.0; 3], shaded, alpha);
                for triangle in building.roof.chunks_exact(3) {
                    let at = |corner: usize| triangle[corner] as usize;
                    let world = [scratch.top[at(0)], scratch.top[at(1)], scratch.top[at(2)]];
                    let ground = photo.map(|_| [0, 1, 2].map(|corner| scratch.ground[at(corner)]));
                    face(
                        out,
                        basis,
                        rect,
                        ((world[0] + world[1] + world[2]) / 3.0 - camera).length_squared(),
                        &world,
                        &[0, 1, 2].map(|corner| scratch.top_on_screen[at(corner)]),
                        Paint {
                            colour: shade,
                            photo: ground.as_ref().map(|ground| Photo { ground, tint }),
                        },
                    );
                }
            }
        }
    }
}

/// A wall's height is taken as at least this (metres) when splitting it into shade
/// and sun.
const MIN_WALL_M: f64 = 1e-3;
/// Light below this (on a flat roof) is not divided by: a slope's light against it.
const MIN_LIGHT: f32 = 1e-3;

/// A point along a wall: at its base and its top, both on screen, and the top's
/// height above the ground (metres).
struct WallEnd {
    base: DVec3,
    top: DVec3,
    base_seen: Option<Corner>,
    top_seen: Option<Corner>,
    top_m: f64,
}

/// A wall's top at `share` along its edge: straight between its ends' `tops_m` and
/// the roof's `bends` over it (in order along the edge).
fn wall_top_m(share: f64, tops_m: (f64, f64), bends: &[WallBend]) -> f64 {
    let mut before = (0.0, tops_m.0);
    for bend in bends
        .iter()
        .map(|bend| (f64::from(bend.share), f64::from(bend.height_m)))
    {
        if bend.0 >= share {
            return before.1 + (bend.1 - before.1) * (share - before.0) / (bend.0 - before.0);
        }
        before = bend;
    }
    before.1 + (tops_m.1 - before.1) * (share - before.0) / (1.0 - before.0)
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
        face(
            out,
            basis,
            rect,
            depth,
            &world,
            &seen,
            [0, 0, 0, shade].into(),
        );
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
    paint: Paint<'_>,
) {
    let Paint { colour, photo } = paint;
    /// Two metres in front of the camera.
    const NEAR_RADII: f64 = 2.0 / METRES_PER_RADIUS;
    let mut corners: Vec<Corner> = Vec::with_capacity(world.len() + 1);
    // The ground under each corner kept, where the face is photographed.
    let mut ground: Vec<DVec3> = Vec::new();
    if seen.iter().all(Option::is_some) {
        corners.extend(seen.iter().flatten());
        if let Some(photo) = &photo {
            ground.extend_from_slice(photo.ground);
        }
    } else {
        let project = |point: DVec3| -> Option<Corner> {
            let at = basis.project_precise(point, rect)?;
            Some((
                Pos2::new(at.x as f32, at.y as f32),
                (1.0 / basis.view_depth(point)) as f32,
            ))
        };
        for (index, &from) in world.iter().enumerate() {
            let next = (index + 1) % world.len();
            let to = world[next];
            let (ahead_from, ahead_to) = (
                basis.view_depth(from) - NEAR_RADII,
                basis.view_depth(to) - NEAR_RADII,
            );
            if ahead_from >= 0.0
                && let Some(corner) = project(from)
            {
                corners.push(corner);
                if let Some(photo) = &photo {
                    ground.push(photo.ground[index]);
                }
            }
            if (ahead_from >= 0.0) != (ahead_to >= 0.0) {
                let share = ahead_from / (ahead_from - ahead_to);
                if let Some(corner) = project(from.lerp(to, share)) {
                    corners.push(corner);
                    if let Some(photo) = &photo {
                        ground.push(photo.ground[index].lerp(photo.ground[next], share));
                    }
                }
            }
        }
    }
    if corners.len() < 3 {
        return;
    }
    // Where the frame shows each corner's ground; a corner whose ground is behind the
    // camera leaves the face to its colour.
    let places: Option<Vec<[u16; 2]>> = photo.and_then(|_| {
        ground
            .iter()
            .map(|&point| {
                let at = basis.project_precise(point, rect)?;
                let share = |at: f64, min: f32, size: f32| {
                    (((at - f64::from(min)) / f64::from(size).max(1.0)).clamp(0.0, 1.0) as f32
                        * PHOTO_SCALE)
                        .round() as u16
                };
                Some([
                    share(at.x, rect.min.x, rect.width()),
                    share(at.y, rect.min.y, rect.height()),
                ])
            })
            .collect()
    });
    let tint = photo.map_or(colour, |photo| photo.tint);
    match places {
        Some(places) => out.photo.extend(places),
        None => out
            .photo
            .extend(std::iter::repeat_n(NO_PHOTO, corners.len())),
    }
    push(out, depth, &corners, colour, tint);
}

/// A face's colour, and for a roof drawn as the frame's photograph, the photo.
#[derive(Clone, Copy)]
struct Paint<'a> {
    colour: [u8; 4],
    photo: Option<Photo<'a>>,
}

impl From<[u8; 4]> for Paint<'_> {
    fn from(colour: [u8; 4]) -> Self {
        Self {
            colour,
            photo: None,
        }
    }
}

/// A roof face drawn as the frame's photograph: the ground under each of its
/// corners (`face`'s `world`, in order) and its light.
#[derive(Clone, Copy)]
struct Photo<'a> {
    ground: &'a [DVec3],
    tint: [u8; 4],
}

/// Adds a convex face, as a fan of triangles from its first corner; its points'
/// photo places are already in `out.photo`.
fn push(out: &mut Faces, depth: f64, corners: &[Corner], colour: [u8; 4], tint: [u8; 4]) {
    let (point, triangle) = (out.points.len() as u32, out.triangles.len() as u32);
    out.points.extend_from_slice(corners);
    out.triangles
        .extend((1..corners.len() as u32 - 1).flat_map(|corner| [0, corner, corner + 1]));
    out.faces.push(Face {
        depth,
        points: point..point + corners.len() as u32,
        triangles: triangle..out.triangles.len() as u32,
        colour,
        tint,
    });
}

/// Whether a ring of (latitude, longitude) runs anticlockwise seen from above.
pub(super) fn anticlockwise(ring: &[(f64, f64)]) -> bool {
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
    /// The light on a block whose top stands `top` (Earth radii) over the sea: the top
    /// sees the sun a little before the ground does (`Daylight::raised`).
    fn at(sun: Option<DVec3>, up: DVec3, top: f64) -> Self {
        match sun {
            Some(sun) => {
                let cosine = up.dot(sun);
                Self {
                    direction: sun,
                    sun: Some((
                        // Blocks are drawn only close to the ground: the eye is
                        // adapted as it is there.
                        crate::pixel::Daylight::at(cosine, 1.0).raised((2.0 * top.max(0.0)).sqrt()),
                        crate::atmosphere::ground_sunlight(cosine as f32),
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
    /// Whether the sun's own light comes in (not only the sky's).
    fn has_sun(&self) -> bool {
        self.sun.is_some_and(|(daylight, _)| daylight.direct > 0.0)
    }
    /// The light on a face with this outward normal (`up` for a roof), `seen` the
    /// share of the sun other blocks leave it.
    fn on(&self, normal: DVec3, up: DVec3, seen: f32) -> [f32; 3] {
        let cosine = normal.dot(self.direction);
        match self.sun {
            Some((daylight, tint)) => {
                // A wall sees half the sky; the sky's light is mostly from above.
                let sky = (0.5 + 0.5 * normal.dot(up)) as f32;
                daylight
                    .light(tint, cosine as f32, seen, sky)
                    .truncate()
                    .to_array()
            }
            None => [AMBIENT + DIRECT * cosine.max(0.0) as f32; 3],
        }
    }
    /// The light on imagery of a roof, which shows the sun of its own day already: the
    /// ground's under the same sun (none without it).
    fn on_imagery(&self, seen: f32) -> [f32; 3] {
        self.sun.map_or([1.0; 3], |(daylight, tint)| {
            daylight
                .light(tint, daylight.flat_cosine, seen, 1.0)
                .truncate()
                .to_array()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;
    use globecraft_data::buildings::{BuildingSnapshot, BuildingTile, Heights};
    use std::sync::Arc;

    /// `buildings` as one tile.
    fn tiles(buildings: Vec<Building>) -> Option<Arc<BuildingSnapshot>> {
        Some(Arc::new(BuildingSnapshot {
            tiles: vec![Arc::new(BuildingTile {
                heights: Default::default(),
                buildings,
            })],
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
            shaped: None,
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
            .collect::<Vec<_>>();
        // Their heights, so the march toward the sun finds them.
        let heights = Heights::of((40.82, -74.05), (40.69, -73.89), &blocks);
        input.buildings = Some(Arc::new(BuildingSnapshot {
            tiles: vec![Arc::new(BuildingTile {
                heights,
                buildings: blocks,
            })],
            zoom: 14,
            fades: Vec::new(),
        }));
        // An afternoon sun, so the blocks cast shadows.
        input.sun = Some(
            (globecraft_core::math::ll_to_v3(40.75, -73.98) + glam::DVec3::new(0.3, -0.5, -0.2))
                .normalize(),
        );
        // The first projection marches every block toward the sun; the next ones
        // keep its results.
        // The first projection with a sun also makes the atmosphere's tables; the
        // first with a new sun marches every block toward it, the next ones keep its
        // results.
        let _ = project(&input, None, None);
        input.sun = input
            .sun
            .map(|sun| (sun + glam::DVec3::splat(0.01)).normalize());
        let start = std::time::Instant::now();
        let _ = project(&input, None, None);
        println!("dense city: {:?} with a new sun", start.elapsed());
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
        assert_eq!(mesh.gpu_vertex_bytes().len(), 20 * mesh.vertices.len());
        assert_eq!(mesh.index_bytes().len(), 4 * mesh.indices.len());
        let rect = input.view.rect;
        for (corner, vertex) in mesh.gpu.iter().zip(&mesh.vertices) {
            let x = rect.min.x + (corner.clip[0] + 1.0) / 2.0 * rect.width();
            let y = rect.min.y + (1.0 - corner.clip[1]) / 2.0 * rect.height();
            assert!((x - vertex.at.x).abs() < 1e-3 && (y - vertex.at.y).abs() < 1e-3);
            assert_eq!(corner.clip[2], vertex.nearness);
            // Without imagery nothing is photographed.
            assert_eq!(corner.photo, NO_PHOTO);
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

    /// `block_at` under a gable 10 m high, its ridge east–west.
    fn gabled_at(lat: f64, lon: f64) -> Building {
        let mut block = block_at(lat, lon);
        let d = 0.0002_f32;
        block.shaped = Some(Box::new(globecraft_data::buildings::ShapedRoof {
            tops_m: vec![20.0; 4],
            // The east and west walls rise to the ridge at their middles.
            bends: [1, 3]
                .map(|edge| WallBend {
                    edge,
                    share: 0.5,
                    height_m: 30.0,
                })
                .to_vec(),
            // From the first corner (south-west): the south slope, then the north.
            points: vec![
                [0.0, 0.0, 20.0],
                [0.0, 2.0 * d, 20.0],
                [d, 2.0 * d, 30.0],
                [d, 0.0, 30.0],
                [2.0 * d, 2.0 * d, 20.0],
                [2.0 * d, 0.0, 20.0],
            ],
            triangles: vec![0, 1, 2, 0, 2, 3, 3, 2, 4, 3, 4, 5],
        }));
        block
    }

    #[test]
    fn a_gabled_roof_shows_both_slopes_lit_apart_and_its_gable_ends() {
        let mut input = view(0.0, 0.0);
        input.buildings = tiles(vec![gabled_at(48.0, 11.0), block_at(48.001, 11.0)]);
        let mesh = project(&input, None, None);
        // Two slopes (two triangles each) for the flat roof's two triangles; the
        // block north still shows its roof and its south wall.
        assert_eq!(mesh.indices.len(), 3 * (2 * 2 + 2 + 2 * 2), "{mesh:?}");
        // Lit from the north-west without the sun: the northern slope is the lighter.
        let colours: std::collections::BTreeSet<[u8; 4]> =
            mesh.vertices.iter().map(|vertex| vertex.colour).collect();
        assert!(colours.len() >= 3, "{colours:?}");
        // From the east and low, the gable end shows as a wall up to the ridge: its
        // highest corner is above the eaves' corners on screen.
        let mut input = view(80.0, 270.0);
        input.buildings = tiles(vec![gabled_at(48.0, 11.0)]);
        let flat = project(
            &{
                let mut flat = view(80.0, 270.0);
                flat.buildings = tiles(vec![block_at(48.0, 11.0)]);
                flat
            },
            None,
            None,
        );
        let gabled = project(&input, None, None);
        let highest = |mesh: &BuildingMesh| {
            mesh.vertices
                .iter()
                .map(|vertex| vertex.at.y)
                .fold(f32::INFINITY, f32::min)
        };
        // A gable 10 m under the flat block's 30 m top at the eaves still reaches it.
        assert!((highest(&gabled) - highest(&flat)).abs() < 1.0);
        assert!(gabled.indices.len() > flat.indices.len());
    }

    #[test]
    fn a_photographed_roof_takes_the_frame_where_its_ground_is_shown() {
        let input = view(0.0, 0.0);
        let (basis, rect) = (&input.view.basis, input.view.rect);
        let ground: Vec<DVec3> = [(47.9998, 10.9998), (47.9998, 11.0002), (48.0002, 11.0002)]
            .iter()
            .map(|&(lat, lon)| basis.surface_position(lat, lon))
            .collect();
        let up = basis.surface_position(48.0, 11.0).normalize();
        let roof: Vec<DVec3> = ground
            .iter()
            .map(|&point| point + up * (30.0 / METRES_PER_RADIUS))
            .collect();
        let seen: Vec<Option<Corner>> = roof
            .iter()
            .map(|&point| {
                basis
                    .project_precise(point, rect)
                    .map(|at| (Pos2::new(at.x as f32, at.y as f32), 1.0))
            })
            .collect();
        let mut out = Faces::default();
        let tint = [200, 200, 200, 255];
        let paint = Paint {
            colour: [10, 20, 30, 255],
            photo: Some(Photo {
                ground: &ground,
                tint,
            }),
        };
        face(&mut out, basis, rect, 1.0, &roof, &seen, paint);
        // Each corner takes the frame's pixel where its ground is drawn, not where
        // the roof above it is.
        for (place, &point) in out.photo.iter().zip(&ground) {
            let at = basis.project_precise(point, rect).unwrap();
            let expected = [
                (at.x - f64::from(rect.min.x)) / f64::from(rect.width()),
                (at.y - f64::from(rect.min.y)) / f64::from(rect.height()),
            ];
            for axis in 0..2 {
                let share = f64::from(place[axis]) / f64::from(PHOTO_SCALE);
                assert!(
                    (share - expected[axis]).abs() < 1e-4,
                    "{place:?} {expected:?}"
                );
            }
        }
        assert_eq!(out.faces[0].tint, tint);
        // Walls and shadows keep their colour alone.
        let mut out = Faces::default();
        face(
            &mut out,
            basis,
            rect,
            1.0,
            &roof,
            &seen,
            [1, 2, 3, 255].into(),
        );
        assert!(out.photo.iter().all(|&place| place == NO_PHOTO));
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
        let light = Light::at(Some(DVec3::new(0.0, 0.6, 0.8)), up, 0.0);
        assert!(light.on(DVec3::Y, up, 1.0)[0] > light.on(DVec3::NEG_Y, up, 1.0)[0]);
        // At night the blocks are dim whatever they face.
        let night = Light::at(Some(DVec3::new(0.0, 0.0, -1.0)), up, 0.0);
        assert!(night.on(DVec3::Y, up, 1.0).iter().all(|&c| c < 0.2));
        // A low sun: the wall facing it glows warm, the one turned away goes blue.
        let low = Light::at(Some(DVec3::new(0.0, 0.996, 0.087)), up, 0.0);
        let [r, _, b] = low.on(DVec3::Y, up, 1.0);
        assert!(r > b, "lit {r} {b}");
        let [r, _, b] = low.on(DVec3::NEG_Y, up, 1.0);
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
