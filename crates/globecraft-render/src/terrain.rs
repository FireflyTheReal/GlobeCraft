//! Relief for the CPU ray caster: each view ray is marched through the elevation model
//! (Mapterhorn Terrarium tiles) until it hits the ground, so mountains hide what lies
//! behind them and stand against the sky up to the horizon.
//!
//! The march uses each tile's pyramid of block maxima (`globecraft_data::tiles::
//! Elevation`): while the ray is above a block's highest point it jumps to the block's
//! far edge, so open air and valleys cost a few steps and only the last texels before
//! the ground are walked one by one. The elevation level follows the distance, so the
//! far landscape uses coarse tiles. Missing tiles are requested with the pixel's screen
//! priority and drawn flat (at sea level) until they arrive.
//!
//! This replaced a 2.5D displacement (2026-10-26) that smeared imagery on tilted
//! views, had no occlusion and switched off toward the horizon.
use super::{Local, LocalTile, Lookup, Provider, TileKey, make_key, rays};
use crate::tuning::WEB_MERCATOR_MAX_LATITUDE_DEG;
use glam::DVec3;
use globecraft_core::{EARTH_RADIUS_M, earth_model::SurfaceGeometry};
use globecraft_data::tiles::{Elevation, Tile};
use std::f64::consts::PI;
use std::sync::Arc;

mod beam;

/// Terrain is drawn below this camera altitude. From `TERRAIN_FADE_FROM_KM` up it
/// flattens with the altitude (`relief_fade`), so it is gone here: switched off at
/// once, a tilted view of the Alps lost its relief in one frame (user's recording,
/// 2026-11-17). 300 km made 150–300 km "ultra laggy" (mostly frames thrown away,
/// since fixed; relief frames also cost four to five flat ones); 150 km faded too
/// quickly, 200 km is nicer (user, 2026-11-17).
pub const TERRAIN_MAX_ALTITUDE_KM: f64 = 200.0;
/// Below this camera altitude relief stands at its full height.
const TERRAIN_FADE_FROM_KM: f64 = 100.0;
/// Without smoothing (`TerrainCfg::without_smoothing`), relief stands at its full
/// height below this altitude and is off above it: the cheapest relief.
const SHARP_RELIEF_MAX_ALTITUDE_KM: f64 = 150.0;

/// The highest camera altitude relief is drawn at, with or without smoothing.
pub fn relief_max_altitude_km(smooth: bool) -> f64 {
    if smooth {
        TERRAIN_MAX_ALTITUDE_KM
    } else {
        SHARP_RELIEF_MAX_ALTITUDE_KM
    }
}

/// The share of its height relief stands at for a camera `altitude_km` high: 1 up to
/// `TERRAIN_FADE_FROM_KM`, easing to 0 at `TERRAIN_MAX_ALTITUDE_KM`; without
/// smoothing, 1 below `SHARP_RELIEF_MAX_ALTITUDE_KM` and 0 above.
pub fn relief_fade(altitude_km: f64, smooth: bool) -> f32 {
    if !smooth {
        return if altitude_km <= SHARP_RELIEF_MAX_ALTITUDE_KM {
            1.0
        } else {
            0.0
        };
    }
    let t = ((altitude_km - TERRAIN_FADE_FROM_KM)
        / (TERRAIN_MAX_ALTITUDE_KM - TERRAIN_FADE_FROM_KM))
        .clamp(0.0, 1.0);
    (1.0 - t * t * (3.0 - 2.0 * t)) as f32
}
/// Accepted vertical exaggeration.
pub const MIN_EXAGGERATION: f32 = 0.5;
pub const MAX_EXAGGERATION: f32 = 3.0;

/// Finest elevation zoom requested; finer imagery reuses its relief.
const DEM_MAX_ZOOM: i32 = 15;
/// Elevation texels per screen pixel along the ray: relief varies slowly, so a texel
/// may cover two pixels.
const DEM_PIXELS_PER_TEXEL: f64 = 2.0;
/// Above every summit (Everest 8,849 m): rays higher than this cannot hit the ground.
const MAX_TERRAIN_M: f64 = 8_900.0;
/// Steps per ray before it gives up and keeps the ellipsoid (a safety bound; typical
/// rays need tens).
const MAX_MARCH_STEPS: usize = 512;
/// Past a block edge by this much (texels), so the next step starts in the next block.
const BLOCK_EXIT_TEXELS: f64 = 1e-3;
/// The most that nudge may be along the ray (world units, about 6 cm): a ray that
/// barely moves across the map (looking straight down) must not leap past the ground.
const MAX_NUDGE: f64 = 1e-8;
/// Elevation tiles are 512 texels across (Mapterhorn).
const DEM_TILE_TEXELS: f64 = 512.0;
/// Linear steps between exact measurements, in texels.
const REANCHOR_TEXELS: f64 = 48.0;
/// Secant refinements of a hit between the last point above and the first below.
const HIT_REFINEMENTS: usize = 3;
/// Hill shading: light from the north-west and above, independent of real sunlight.
const LIGHT_UP: f64 = 0.7;
const LIGHT_NORTH: f64 = 0.55;
const LIGHT_WEST: f64 = 0.45;
const SHADE_BASE: f64 = 0.9;
const SHADE_GAIN: f64 = 0.7;
const MIN_SHADE: f64 = 0.58;
const MAX_SHADE: f64 = 1.25;
/// Steepest slope (rise over run) the shading uses; cliffs beyond it shade the same.
const MAX_SLOPE: f64 = 5.0;
/// Keeps slope distances finite next to the poles.
const MIN_LATITUDE_COSINE: f64 = 0.015;

#[cfg(test)]
thread_local! {
    /// March steps and tile look-ups on this thread (probes).
    pub(crate) static COUNTS: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// Terrain settings of a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainCfg {
    exaggeration: f64,
    /// `relief_fade`: the hill shading fades with the heights, or the view darkened
    /// and the shading appeared at once where relief switched on (user, 2026-11-17).
    fade: f32,
    /// Heights and shading blend between elevation levels, and arriving tiles grow
    /// in; off, each level and tile shows at once (Settings › Graphics, for speed).
    smooth: bool,
}

impl TerrainCfg {
    /// Relief with the given vertical exaggeration (clamped to the accepted range),
    /// flattened by `fade` (0–1, `relief_fade`).
    pub fn new(exaggeration: f32, fade: f32) -> Self {
        Self {
            exaggeration: f64::from(exaggeration.clamp(MIN_EXAGGERATION, MAX_EXAGGERATION))
                * f64::from(fade.clamp(0.0, 1.0)),
            fade: fade.clamp(0.0, 1.0),
            smooth: true,
        }
    }
    /// This relief without blending between levels or growing tiles in: faster.
    pub fn without_smoothing(self) -> Self {
        Self {
            smooth: false,
            ..self
        }
    }
}

/// What a view ray met.
pub(super) enum March {
    /// The ground: the ray moved to the hit, and the hill-shading factor there.
    Hit(rays::RaySample, f32),
    /// No relief in the way before the globe's surface (sea level, or no elevation
    /// loaded): the ellipsoid hit stands.
    Floor,
    /// Nothing: sky.
    Miss,
}

/// Relief casts shadows with the sun on (roadmap C5): ground behind a ridge, seen
/// from the sun, keeps this share of its light. The air's own twilight darkens it
/// further where the sun is low.
const TERRAIN_SHADOW_LIGHT: f32 = 0.6;
/// The march toward the sun starts this high over the ground (metres, before
/// exaggeration), so the slope it leaves does not shade itself.
const SHADOW_LIFT_M: f64 = 20.0;
/// …and at least this many of the elevation texels it reads above it, times the
/// exaggeration: a coarser level's heights stand above the finer ground on slopes.
const SHADOW_LIFT_TEXELS: f64 = 1.5;
/// The march toward the sun reads the elevation this many times coarser than the
/// view: the shadow's edge is soft anyway, and it reads fewer, smaller tiles.
const SHADOW_DETAIL_COARSER: f64 = 4.0;
/// Tiles only the shadows need are asked for after the view's own.
const SHADOW_REQUEST_PRIORITY: f32 = f32::MAX;

/// The light left at `ground` (a relief hit seen from `camera`, `hit.distance` away)
/// by the relief between it and the sun (`sun`, a unit vector): `TERRAIN_SHADOW_LIGHT`
/// in the shadow, all of it (1) in the sun, with the shadow faded as the relief is.
pub(super) fn sun_light(
    hit: &rays::RaySample,
    sun: DVec3,
    geometry: SurfaceGeometry,
    terrain: TerrainCfg,
    pixel_angle: f64,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
) -> f32 {
    let up = hit.normal;
    // Below the horizon there the night darkens the ground already.
    if sun.dot(up) <= 0.0 || terrain.fade <= 0.0 {
        return 1.0;
    }
    let to_world = terrain.exaggeration / EARTH_RADIUS_M;
    let texel = hit.distance * pixel_angle * SHADOW_DETAIL_COARSER * DEM_PIXELS_PER_TEXEL;
    let lift = (SHADOW_LIFT_M * to_world).max(texel * SHADOW_LIFT_TEXELS * terrain.exaggeration);
    let start = hit.point + up * lift;
    // The march picks its elevation level by the distance from where it starts: from
    // a point as far behind as the camera is, it reads the levels the view reads.
    let behind = hit.distance;
    let ray = rays::RaySample {
        direction: sun,
        ..*hit
    };
    let shaded = matches!(
        march(
            &ray,
            behind,
            false,
            start - sun * behind,
            geometry,
            terrain,
            pixel_angle * SHADOW_DETAIL_COARSER,
            SHADOW_REQUEST_PRIORITY,
            loc,
        ),
        March::Hit(..)
    );
    if shaded {
        1.0 + (TERRAIN_SHADOW_LIGHT - 1.0) * terrain.fade
    } else {
        1.0
    }
}

/// A loaded elevation tile (by its index in the batch's `Local::dem`) and where it sits.
#[derive(Clone, Copy)]
pub(super) struct Grid {
    index: usize,
    /// Tiles per side at the tile's level.
    n: f64,
    tx: f64,
    ty: f64,
    /// Texels per side.
    size: f64,
}

impl Grid {
    fn elevation<'a>(&self, dem: &'a [Arc<Tile>]) -> &'a Elevation {
        dem[self.index]
            .elevation()
            .expect("elevation tiles decode with heights")
    }
    /// Texel coordinates of `(u, v)` in this tile (outside `0..size` when not in it).
    fn texel(&self, (u, v): (f64, f64)) -> (f64, f64) {
        (
            (u * self.n - self.tx) * self.size,
            (v * self.n - self.ty) * self.size,
        )
    }
    /// The `(u, v)` of texel coordinates `(x, y)` of this tile.
    fn uv(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            (self.tx + x / self.size) / self.n,
            (self.ty + y / self.size) / self.n,
        )
    }
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        (0.0..self.size).contains(&x) && (0.0..self.size).contains(&y)
    }
    /// No bilinear height between `from` and `to` (`(u, v)` corners of a square no
    /// wider than this tile) is above this, in metres.
    fn top_between_m(&self, dem: &[Arc<Tile>], from: (f64, f64), to: (f64, f64)) -> f32 {
        let elevation = self.elevation(dem);
        let last = self.size - 1.0;
        let (x0, y0) = self.texel(from);
        let (x1, y1) = self.texel(to);
        let (x0, y0) = (x0.clamp(0.0, last), y0.clamp(0.0, last));
        let (x1, y1) = (x1.clamp(0.0, last), y1.clamp(0.0, last));
        // Blocks at least as wide as the square: it lies in at most two by two.
        let width = (x1 - x0).max(y1 - y0).max(1.0);
        let level = (width.log2().ceil() as usize).min(elevation.top_level());
        let (bx, by) = (
            (x0 as usize >> level, x1 as usize >> level),
            (y0 as usize >> level, y1 as usize >> level),
        );
        [(bx.0, by.0), (bx.1, by.0), (bx.0, by.1), (bx.1, by.1)]
            .into_iter()
            .map(|(bx, by)| elevation.block_max_m(level, bx, by))
            .fold(f32::MIN, f32::max)
    }
}

/// An elevation tile that arrived a moment ago: its relief grows from what stood
/// there before instead of snapping to its heights.
#[derive(Clone, Copy)]
struct Morph {
    /// 0 as the tile arrives to 1 when its own heights stand (`Tile::grown`).
    grown: f32,
    /// The coarser tile that stood in for it (its index in `Local::dem`), itself
    /// perhaps still growing; sea level when there was none.
    from: Option<usize>,
}

/// How many growing tiles down a tile's heights follow (its stand-in, and that
/// one's): below, a tile's own heights stand. Only a jump to a new place, every
/// level arriving at once, makes longer chains; followed all the way down at every
/// step, a frame there took minutes instead of a second (2026-11-17).
const MORPH_DEPTH: u8 = 2;

/// The elevation tile a ray is over.
#[derive(Clone, Copy)]
pub(super) struct DemTile {
    grid: Grid,
    morph: Option<Morph>,
}

impl std::ops::Deref for DemTile {
    type Target = Grid;
    fn deref(&self) -> &Grid {
        &self.grid
    }
}

/// The elevation tiles a batch holds: `placed[i]` says where `tiles[i]` sits.
#[derive(Clone, Copy)]
struct Held<'a> {
    tiles: &'a [Arc<Tile>],
    placed: &'a [DemTile],
}

impl<'a> Held<'a> {
    fn of<F>(loc: &'a Local<'_, F>) -> Self {
        Self {
            tiles: &loc.dem,
            placed: &loc.dem_placed,
        }
    }
}

impl DemTile {
    /// The growth this tile shows at `depth` down a chain of growing tiles: none
    /// from `MORPH_DEPTH` on.
    fn morph_at(&self, depth: u8) -> Option<Morph> {
        self.morph.filter(|_| depth < MORPH_DEPTH)
    }
    /// The height at `(u, v)`, in metres: this tile's, or on the way to it from
    /// the stand-in's.
    fn height_m(&self, held: Held, uv: (f64, f64)) -> f32 {
        self.height_at(held, uv, 0)
    }
    fn height_at(&self, held: Held, uv: (f64, f64), depth: u8) -> f32 {
        let (x, y) = self.texel(uv);
        let own = self.elevation(held.tiles).height_m(x, y);
        let Some(morph) = self.morph_at(depth) else {
            return own;
        };
        let before = morph
            .from
            .map_or(0.0, |from| held.placed[from].height_at(held, uv, depth + 1));
        before + (own - before) * morph.grown
    }
    /// No height in block `(bx, by)` of `level` is above this, in metres.
    #[inline]
    fn block_top_m(&self, held: Held, level: usize, bx: usize, by: usize) -> f32 {
        let own = self.elevation(held.tiles).block_max_m(level, bx, by);
        let Some(morph) = self.morph_at(0) else {
            return own;
        };
        let block = (1usize << level) as f64;
        let corner = |bx: usize, by: usize| self.uv((bx as f64 * block, by as f64 * block));
        own.max(Self::top_before_m(
            morph,
            held,
            corner(bx, by),
            corner(bx + 1, by + 1),
            0,
        ))
    }
    /// No height between `from` and `to` (`(u, v)` corners of a square inside this
    /// tile) is above this, in metres. A growing tile's heights lie between its own
    /// and its stand-in's, so under both tops.
    fn top_between_m(&self, held: Held, from: (f64, f64), to: (f64, f64)) -> f32 {
        self.top_between_at(held, from, to, 0)
    }
    fn top_between_at(&self, held: Held, from: (f64, f64), to: (f64, f64), depth: u8) -> f32 {
        let own = self.grid.top_between_m(held.tiles, from, to);
        self.morph_at(depth).map_or(own, |morph| {
            own.max(Self::top_before_m(morph, held, from, to, depth))
        })
    }
    /// `top_between_m` when the square lies inside this tile; `None` otherwise.
    fn top_within_m(&self, held: Held, from: (f64, f64), to: (f64, f64)) -> Option<f32> {
        let inside =
            |(x, y): (f64, f64)| (0.0..=self.size).contains(&x) && (0.0..=self.size).contains(&y);
        (inside(self.texel(from)) && inside(self.texel(to)))
            .then(|| self.top_between_m(held, from, to))
    }
    /// No height of this tile is above this over block `(x, y)` of `level` of the next
    /// finer level's elevation pyramid, numbered across that whole level: the same
    /// ground is one block of the level below here (half a texel at level 0). `None`
    /// when this tile is not of the next coarser level, or the block is not in it.
    fn top_over_finer_m(
        &self,
        held: Held,
        finer: i32,
        level: usize,
        (x, y): (usize, usize),
    ) -> Option<f32> {
        if self.level() != finer - 1 {
            return None;
        }
        let (level, x, y) = if level == 0 {
            (0, x / 2, y / 2)
        } else {
            (level - 1, x, y)
        };
        let per_tile = (self.size as usize) >> level;
        let (x0, y0) = (self.tx as usize * per_tile, self.ty as usize * per_tile);
        ((x0..x0 + per_tile).contains(&x) && (y0..y0 + per_tile).contains(&y))
            .then(|| self.block_top_m(held, level, x - x0, y - y0))
    }
    /// The tile's level.
    fn level(&self) -> i32 {
        self.n.log2().round() as i32
    }
    fn top_before_m(morph: Morph, held: Held, from: (f64, f64), to: (f64, f64), depth: u8) -> f32 {
        morph.from.map_or(0.0, |below| {
            held.placed[below].top_between_at(held, from, to, depth + 1)
        })
    }
    /// The hill shading at `uv`, on the way from the stand-in's while growing.
    fn shade(&self, held: Held, uv: (f64, f64), up: DVec3, to_world: f64) -> f32 {
        self.shade_at(held, uv, up, to_world, 0)
    }
    fn shade_at(&self, held: Held, uv: (f64, f64), up: DVec3, to_world: f64, depth: u8) -> f32 {
        let own = hill_shade(&self.grid, self.elevation(held.tiles), uv, up, to_world);
        let Some(morph) = self.morph_at(depth) else {
            return own;
        };
        let before = morph.from.map_or(SHADE_BASE as f32, |from| {
            held.placed[from].shade_at(held, uv, up, to_world, depth + 1)
        });
        before + (own - before) * morph.grown
    }
}

/// The finest loaded elevation tile at or below `zoom` under `(u, v)`. Missing levels are
/// recorded for the downloader with screen priority `dist`.
fn dem_tile(
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    zoom: i32,
    (u, v): (f64, f64),
    dist: f32,
) -> Option<DemTile> {
    if !(0.0..=1.0).contains(&v) || !(0.0..1.0).contains(&u) {
        return None;
    }
    (0..=zoom.clamp(0, DEM_MAX_ZOOM))
        .rev()
        .find_map(|level| dem_tile_at(loc, level, (u, v), dist))
}

/// The elevation tile of `level` under `(u, v)`, when it is loaded; when it is
/// missing, it is recorded for the downloader with screen priority `dist`.
fn dem_tile_at(
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    level: i32,
    (u, v): (f64, f64),
    dist: f32,
) -> Option<DemTile> {
    let n = (1u64 << level) as f64;
    let tx = (u * n).floor().min(n - 1.0);
    let ty = (v * n).floor().min(n - 1.0);
    let key = make_key(Provider::TerrainDem, level as u8, tx as u32, ty as u32, 0);
    if let Some(&index) = loc.dem_index.get(&key) {
        return Some(loc.dem_placed[index]);
    }
    let tile = loc
        .map
        .entry(key)
        .or_insert_with(|| match (loc.lookup)(key) {
            Lookup::Ready(tile) => LocalTile::Ready(tile),
            Lookup::Pending => LocalTile::Pending,
            Lookup::Absent => LocalTile::Absent(dist),
            Lookup::Unavailable => LocalTile::Unavailable,
        });
    let tile = match tile {
        LocalTile::Ready(tile) if tile.elevation().is_some() => tile.clone(),
        LocalTile::Ready(_) | LocalTile::Pending | LocalTile::Unavailable => return None,
        LocalTile::Absent(nearest) => {
            *nearest = nearest.min(dist);
            return None;
        }
    };
    let grown = tile.grown(loc.now);
    let morph = (grown < 1.0).then(|| Morph {
        grown,
        // What a frame drew here before this tile arrived.
        from: (0..level)
            .rev()
            .find_map(|level| dem_tile_at(loc, level, (u, v), dist))
            .map(|below| below.index),
    });
    let placed = DemTile {
        grid: Grid {
            index: loc.dem.len(),
            n,
            tx,
            ty,
            size: tile.elevation().map_or(0, Elevation::size) as f64,
        },
        morph,
    };
    loc.dem.push(tile);
    loc.dem_placed.push(placed);
    loc.dem_index.insert(key, placed.index);
    Some(placed)
}

/// Web Mercator texture coordinates of a surface normal, as `pixel::surface` computes them.
fn mercator_uv(normal: DVec3) -> (f64, f64) {
    let lon = normal.y.atan2(normal.x);
    let max_sine = WEB_MERCATOR_MAX_LATITUDE_DEG.to_radians().sin();
    let s = normal.z.clamp(-max_sine, max_sine);
    // Clamped: at the Mercator limit rounding would otherwise leave the tile range.
    let v = (0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI)).clamp(0.0, 1.0);
    let u = lon / (2.0 * PI) + 0.5;
    (if u >= 1.0 { u - 1.0 } else { u.max(0.0) }, v)
}

/// Where along `origin + direction·t` a ray enters and leaves the surface grown by
/// `height` (world units); `None` when it passes outside.
fn shell_span(
    geometry: SurfaceGeometry,
    origin: DVec3,
    direction: DVec3,
    height: f64,
) -> Option<(f64, f64)> {
    let axes = geometry.axes() + DVec3::splat(height);
    let q = DVec3::ONE / (axes * axes);
    let a = direction.dot(q * direction);
    let b = origin.dot(q * direction);
    let c = origin.dot(q * origin) - 1.0;
    let disc = b * b - a * c;
    if disc < 0.0 || a <= 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (near, far) = ((-b - root) / a, (-b + root) / a);
    (far > 0.0).then_some((near.max(0.0), far))
}

/// Height above the surface (world units) of `point`: the algebraic distance to the
/// ellipsoid scaled to a height, within metres of the geodetic height at every
/// elevation on Earth.
#[inline]
fn height_above(point: DVec3, q: DVec3) -> f64 {
    let g = point.dot(point * q).sqrt();
    point.length() * (1.0 - 1.0 / g)
}

/// Where the ray was last measured exactly: Mercator coordinates and their rate along
/// the ray, so the steps in between move linearly (exact trigonometry per step was
/// most of the cost).
struct Anchor {
    t: f64,
    uv: (f64, f64),
    duv: (f64, f64),
    /// Rate of height gain along the ray (negative while descending).
    climb: f64,
    latitude_cosine: f64,
}

impl Anchor {
    fn at(geometry: SurfaceGeometry, origin: DVec3, direction: DVec3, t: f64) -> Self {
        let point = origin + direction * t;
        let normal = geometry.normal(point).normalize();
        let uv = mercator_uv(normal);
        // The normal turns as the ray moves sideways over the curved surface: its rate
        // is the direction's part across the normal, over the distance from the centre
        // (exact on a sphere; the ellipsoid's flattening changes it by a fraction of a
        // percent). Mercator's derivatives follow from that rate.
        let turn = (direction - normal * direction.dot(normal)) / point.length();
        let across_sq = (normal.x * normal.x + normal.y * normal.y).max(1e-24);
        let dlon = (normal.x * turn.y - normal.y * turn.x) / across_sq;
        let sine = normal.z.clamp(-0.999_999, 0.999_999);
        let dv = -turn.z / ((1.0 - sine * sine) * 2.0 * PI);
        Self {
            t,
            uv,
            duv: (dlon / (2.0 * PI), dv),
            climb: direction.dot(normal),
            latitude_cosine: across_sq.sqrt(),
        }
    }
    #[inline]
    fn uv(&self, t: f64) -> (f64, f64) {
        let dt = t - self.t;
        let mut u = self.uv.0 + self.duv.0 * dt;
        if u < 0.0 {
            u += 1.0;
        } else if u >= 1.0 {
            u -= 1.0;
        }
        (u, (self.uv.1 + self.duv.1 * dt).clamp(0.0, 1.0))
    }
}

/// The elevation level along a ray and how far its own heights stand: 0 where it
/// takes over from the next coarser level, 1 from `HEIGHT_BLEND_LEVELS` levels on.
/// Heights blend between the two levels by it, so they change with the zoom instead of
/// at once where the level changes (user, 2026-11-17). The zoom falls with the
/// logarithm of the distance, so where each level starts, blends and ends is worked
/// out once per level; the march asks at every step.
pub(super) struct ZoomTrack {
    level: i32,
    /// A distance and its exact zoom, which the others follow from.
    t0: f64,
    zoom0: f64,
    /// The level holds from `nearest` to `until`, blending from `blend_from` on.
    nearest: f64,
    blend_from: f64,
    until: f64,
}

impl ZoomTrack {
    pub(super) fn new() -> Self {
        Self {
            level: 0,
            t0: 0.0,
            zoom0: 0.0,
            nearest: f64::INFINITY,
            blend_from: 0.0,
            until: f64::NEG_INFINITY,
        }
    }
    /// The level for a point `t` away and its weight (see the type).
    pub(super) fn at(&mut self, t: f64, pixel_angle: f64, latitude_cosine: f64) -> (i32, f64) {
        if !(self.nearest..=self.until).contains(&t) {
            let zoom = dem_zoom_exact(t, pixel_angle, latitude_cosine, DEM_TILE_TEXELS);
            let level = (zoom.floor() as i32).clamp(0, DEM_MAX_ZOOM);
            // The distance at which the zoom is `z`.
            let at = |z: f64| t * (zoom - z).exp2();
            *self = Self {
                level,
                t0: t,
                zoom0: zoom,
                nearest: if level == DEM_MAX_ZOOM {
                    0.0
                } else {
                    at(f64::from(level + 1))
                },
                blend_from: at(f64::from(level) + HEIGHT_BLEND_LEVELS),
                until: if level == 0 {
                    f64::INFINITY
                } else {
                    at(f64::from(level))
                },
            };
        }
        if t < self.blend_from {
            return (self.level, 1.0);
        }
        let zoom = self.zoom0 - (t / self.t0).log2();
        (
            self.level,
            ((zoom - f64::from(self.level)) / HEIGHT_BLEND_LEVELS).clamp(0.0, 1.0),
        )
    }
}

/// How much of a level's zoom range its heights take to grow out of the coarser
/// level's: over the zoom of about 15 % more distance. Blending over the whole level
/// made relief frames twice as slow; 0.3 cost 12–21 % in tilted views, 0.15 4–12 %
/// (`terrain_views`, 2026-11-17).
const HEIGHT_BLEND_LEVELS: f64 = 0.2;

/// The elevation zoom for a point `t` away, before it is rounded down to a level:
/// texels of about `DEM_PIXELS_PER_TEXEL` pixels, given the angle per pixel
/// `pixel_angle`.
fn dem_zoom_exact(t: f64, pixel_angle: f64, latitude_cosine: f64, tile_texels: f64) -> f64 {
    let metres_per_pixel = (t * EARTH_RADIUS_M * pixel_angle).max(0.01);
    let texel_m = metres_per_pixel * DEM_PIXELS_PER_TEXEL;
    (2.0 * PI * EARTH_RADIUS_M * latitude_cosine.max(MIN_LATITUDE_COSINE) / (tile_texels * texel_m))
        .log2()
}

/// Where the rays of each block of pixels in rows `rows` may start their march: for
/// every `BEAM_PX` columns, the distance below which no ray of the block meets the
/// relief (see `beam`). Index with `x / BEAM_PX`.
#[allow(clippy::too_many_arguments)]
pub(super) fn beam_starts(
    rays: &rays::RayCaster,
    w: usize,
    h: usize,
    rows: std::ops::Range<usize>,
    terrain: TerrainCfg,
    pixel_angle: f64,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
) -> Vec<f64> {
    let (top, bottom) = (rows.start, rows.end.saturating_sub(1));
    (0..w)
        .step_by(beam::BEAM_PX)
        .map(|left| {
            let right = (left + beam::BEAM_PX).min(w) - 1;
            let cone = beam::Cone::around([
                rays.direction(left, top),
                rays.direction(right, top),
                rays.direction(left, bottom),
                rays.direction(right, bottom),
            ]);
            let offset = (
                (left + right) as f32 / 2.0 - w as f32 / 2.0,
                (top + bottom) as f32 / 2.0 - h as f32 / 2.0,
            );
            beam::clear_distance(
                cone,
                rays.camera(),
                rays.model_geometry(),
                terrain,
                pixel_angle,
                offset.0 * offset.0 + offset.1 * offset.1,
                loc,
            )
        })
        .collect()
}

/// Columns per entry of `beam_starts`.
pub(super) const BEAM_COLUMNS: usize = beam::BEAM_PX;

/// Marches `ray` from the camera at `origin` through the relief. `hits_surface` says
/// whether the ray meets the ellipsoid (at `ray.distance`); `pixel_angle` is the view
/// angle per displayed pixel; `dist` the pixel's screen priority for tile requests;
/// `start` a distance the ray is known to stay above the relief until (`beam_starts`).
#[allow(clippy::too_many_arguments)]
pub(super) fn march(
    ray: &rays::RaySample,
    start: f64,
    hits_surface: bool,
    origin: DVec3,
    geometry: SurfaceGeometry,
    terrain: TerrainCfg,
    pixel_angle: f64,
    dist: f32,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
) -> March {
    let direction = ray.direction;
    let to_world = terrain.exaggeration / EARTH_RADIUS_M;
    let Some((enter, leave)) = shell_span(geometry, origin, direction, MAX_TERRAIN_M * to_world)
    else {
        return if hits_surface {
            March::Floor
        } else {
            March::Miss
        };
    };
    let end = if hits_surface {
        ray.distance.min(leave)
    } else {
        leave
    };
    let q = geometry.inverse_axes_squared();
    let mut t = enter.max(start);
    // The last point known to be above the ground and its clearance, for the hit.
    let mut above: Option<(f64, f64)> = None;
    let mut current: Option<DemTile> = None;
    let mut looked_up: Option<i32> = None;
    let mut anchor = Anchor::at(geometry, origin, direction, t);
    // The next coarser level's tile under the ray, which the heights blend from.
    // Looked up once a blend needs it (`Some(None)`: there is none).
    let mut coarser: Option<Option<DemTile>> = None;
    let mut levels = ZoomTrack::new();
    // The march may climb one level coarser per step (the classic max-mipmap walk).
    let mut start_level = usize::MAX;
    for _ in 0..MAX_MARCH_STEPS {
        #[cfg(test)]
        COUNTS.with(|c| c.set((c.get().0 + 1, c.get().1)));
        if t >= end {
            break;
        }
        let point = origin + direction * t;
        let height = height_above(point, q);
        if height <= 0.0 {
            break;
        }
        let (zoom, weight) = levels.at(t, pixel_angle, anchor.latitude_cosine);
        let weight = if terrain.smooth { weight } else { 1.0 };
        let mut uv = anchor.uv(t);
        // Keep the tile while it covers the point and the wanted zoom holds (it may be
        // a coarser stand-in for a missing tile); look again otherwise.
        let reuse = looked_up == Some(zoom)
            && current
                .as_ref()
                .is_some_and(|tile| tile.contains(tile.texel(uv)));
        if !reuse {
            #[cfg(test)]
            COUNTS.with(|c| c.set((c.get().0, c.get().1 + 1)));
            current = dem_tile(loc, zoom, uv, dist);
            looked_up = current.is_some().then_some(zoom);
            start_level = usize::MAX;
            coarser = None;
        }
        // A stand-in for a missing level shows its own heights.
        let blend = if weight < 1.0 && zoom > 0 && current.is_some_and(|tile| tile.level() == zoom)
        {
            *coarser.get_or_insert_with(|| dem_tile(loc, zoom - 1, uv, dist))
        } else {
            None
        };
        let Some(tile) = current.as_ref() else {
            // No elevation here yet: flat at sea level until this zoom's tile area ends.
            let n = (1u64 << zoom) as f64;
            let (x, y) = (uv.0 * n, uv.1 * n);
            let (vx, vy) = (anchor.duv.0 * n, anchor.duv.1 * n);
            above = Some((t, height));
            t += block_exit(x - (x as u64) as f64, y - (y as u64) as f64, 1.0, vx, vy)
                + (BLOCK_EXIT_TEXELS / ((vx * vx + vy * vy).sqrt() * DEM_TILE_TEXELS).max(1e-12))
                    .min(MAX_NUDGE);
            continue;
        };
        let tile = *tile;
        let held = Held::of(loc);
        // The tile's own heights: all there is to it once it has grown in.
        let elevation = tile.elevation(held.tiles);
        let grown = !terrain.smooth || tile.morph.is_none();
        let (vx, vy) = (
            anchor.duv.0 * tile.n * tile.size,
            anchor.duv.1 * tile.n * tile.size,
        );
        let speed = (vx * vx + vy * vy).sqrt().max(1e-12);
        // Linear texel positions drift on long jumps: measure exactly again.
        if (t - anchor.t) * speed > REANCHOR_TEXELS {
            anchor = Anchor::at(geometry, origin, direction, t);
            uv = anchor.uv;
        }
        let (x, y) = tile.texel(uv);
        // The coarsest block the ray is above.
        let (ix, iy) = (x as usize, y as usize);
        let mut clear = None;
        for level in (0..=elevation.top_level().min(start_level)).rev() {
            let (bx, by) = (ix >> level, iy >> level);
            let mut top = if grown {
                elevation.block_max_m(level, bx, by)
            } else {
                tile.block_top_m(held, level, bx, by)
            };
            // Blended heights lie under both levels' tops.
            if let Some(coarser) = blend
                && height > f64::from(top) * to_world
            {
                let per_tile = (tile.size as usize) >> level;
                let whole = (
                    tile.tx as usize * per_tile + bx,
                    tile.ty as usize * per_tile + by,
                );
                let coarse = coarser
                    .top_over_finer_m(held, tile.level(), level, whole)
                    .or_else(|| {
                        let block = (1usize << level) as f64;
                        let corner =
                            |bx: usize, by: usize| tile.uv((bx as f64 * block, by as f64 * block));
                        coarser.top_within_m(held, corner(bx, by), corner(bx + 1, by + 1))
                    });
                top = top.max(coarse.unwrap_or(f32::MAX));
            }
            let top = f64::from(top) * to_world;
            if height > top {
                clear = Some((level, bx, by, top));
                break;
            }
        }
        match clear {
            Some((level, bx, by, top)) => {
                let block = (1usize << level) as f64;
                let mut step =
                    block_exit(x - bx as f64 * block, y - by as f64 * block, block, vx, vy);
                if anchor.climb < 0.0 {
                    step = step.min((height - top) / -anchor.climb);
                }
                above = Some((t, height - top));
                t += step + (BLOCK_EXIT_TEXELS / speed).min(MAX_NUDGE);
                start_level = level + 1;
            }
            None => {
                let own_m = |uv: (f64, f64)| {
                    if grown {
                        let (x, y) = tile.texel(uv);
                        elevation.height_m(x, y)
                    } else {
                        tile.height_m(held, uv)
                    }
                };
                let ground_m = |uv: (f64, f64)| match blend {
                    Some(coarser) => {
                        let from = coarser.height_m(held, uv);
                        from + (own_m(uv) - from) * weight as f32
                    }
                    None => own_m(uv),
                };
                let ground = f64::from(ground_m(uv)) * to_world;
                if height <= ground {
                    let t = refine(above, t, height - ground, |t| {
                        height_above(origin + direction * t, q)
                            - f64::from(ground_m(anchor.uv(t))) * to_world
                    });
                    let point = origin + direction * t;
                    let normal = geometry.normal(point).normalize();
                    let uv = anchor.uv(t);
                    let mut shade = if grown {
                        hill_shade(&tile.grid, elevation, uv, normal, to_world)
                    } else {
                        tile.shade(held, uv, normal, to_world)
                    };
                    // A finer level steepens the slopes, so the shading darkened at
                    // once where the level changed, all over the view as it zoomed
                    // (user's recording, 2026-11-17). It comes in with the zoom
                    // instead: from the next coarser level's at the change to this
                    // level's one level on.
                    let level = tile.n.log2().round() as i32;
                    let toward =
                        (dem_zoom_exact(t, pixel_angle, anchor.latitude_cosine, DEM_TILE_TEXELS)
                            - f64::from(level))
                        .clamp(0.0, 1.0) as f32;
                    if toward < 1.0
                        && terrain.smooth
                        && level > 0
                        && let Some(coarser) = dem_tile(loc, level - 1, uv, dist)
                    {
                        let held = Held::of(loc);
                        let before = coarser.shade(held, uv, normal, to_world);
                        shade = before + (shade - before) * toward;
                    }
                    // Flat ground shades darker than the flat globe (`SHADE_BASE`).
                    let shade = 1.0 + (shade - 1.0) * terrain.fade;
                    return March::Hit(
                        rays::RaySample {
                            distance: t,
                            point,
                            normal,
                            incidence: (-direction.dot(normal)).max(0.0),
                            ..*ray
                        },
                        shade,
                    );
                }
                above = Some((t, height - ground));
                // To the texel's edge, or down to the ground's height here when that
                // comes first (a steep ray barely crosses texels); a step that ends
                // below the ground is a hit, refined between the two points.
                let mut step = block_exit(x - ix as f64, y - iy as f64, 1.0, vx, vy);
                if anchor.climb < 0.0 {
                    step = step.min((height - ground) / -anchor.climb);
                }
                t += step + (BLOCK_EXIT_TEXELS / speed).min(MAX_NUDGE);
                start_level = 1;
            }
        }
    }
    if hits_surface {
        March::Floor
    } else {
        March::Miss
    }
}

/// Distance along the ray to leave a block `block` texels wide, from `(x, y)` inside
/// it, at texel velocity `(vx, vy)` per unit of distance.
fn block_exit(x: f64, y: f64, block: f64, vx: f64, vy: f64) -> f64 {
    let axis = |position: f64, velocity: f64| {
        if velocity > 0.0 {
            (block - position) / velocity
        } else if velocity < 0.0 {
            -position / velocity
        } else {
            f64::INFINITY
        }
    };
    axis(x, vx).min(axis(y, vy))
}

/// The hit between the last point above the ground and `t` (below it by `-below`),
/// by secant steps on `clearance`, the height above the ground along the ray.
fn refine(above: Option<(f64, f64)>, t: f64, below: f64, clearance: impl Fn(f64) -> f64) -> f64 {
    let Some((mut t0, mut c0)) = above else {
        return t;
    };
    let (mut t1, mut c1) = (t, below);
    for _ in 0..HIT_REFINEMENTS {
        if c0 - c1 <= 0.0 {
            break;
        }
        let tm = t0 + (t1 - t0) * c0 / (c0 - c1);
        let c = clearance(tm);
        if c > 0.0 {
            (t0, c0) = (tm, c);
        } else {
            (t1, c1) = (tm, c);
        }
    }
    t1
}

/// Brightness from the slope at `uv`, so ridges and valleys read without sunlight.
/// 1 on flat ground.
fn hill_shade(tile: &Grid, elevation: &Elevation, uv: (f64, f64), up: DVec3, to_world: f64) -> f32 {
    let (x, y) = tile.texel(uv);
    let (east, north) = elevation.slopes_m(x, y);
    let latitude_cosine = (up.x * up.x + up.y * up.y).sqrt().max(MIN_LATITUDE_COSINE);
    // One texel on the ground (Mercator is conformal: the same both ways).
    let run_m = 2.0 * PI * EARTH_RADIUS_M * latitude_cosine / (tile.n * elevation.size() as f64);
    let exaggeration = to_world * EARTH_RADIUS_M;
    let slope_east = (f64::from(east) / (2.0 * run_m) * exaggeration).clamp(-MAX_SLOPE, MAX_SLOPE);
    let slope_north =
        (f64::from(north) / (2.0 * run_m) * exaggeration).clamp(-MAX_SLOPE, MAX_SLOPE);
    // In the local frame (east, north, up) the relief normal is (-east slope, -north
    // slope, 1) and the light (-west, north, up): their cosine needs no vectors.
    let light_length =
        (LIGHT_UP * LIGHT_UP + LIGHT_NORTH * LIGHT_NORTH + LIGHT_WEST * LIGHT_WEST).sqrt();
    let flat = LIGHT_UP / light_length;
    let relief = ((LIGHT_UP + slope_east * LIGHT_WEST - slope_north * LIGHT_NORTH)
        / ((1.0 + slope_east * slope_east + slope_north * slope_north).sqrt() * light_length))
        .max(0.0);
    (SHADE_BASE + SHADE_GAIN * (relief - flat)).clamp(MIN_SHADE, MAX_SHADE) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercator_uv_matches_the_surface_projection() {
        let (u, v) = mercator_uv(DVec3::X);
        assert!((u - 0.5).abs() < 1e-12);
        assert!((v - 0.5).abs() < 1e-12);
        let north = DVec3::new(
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        );
        let (_, v) = mercator_uv(north);
        // 45° N in Web Mercator.
        let expected = 0.5 - (45f64.to_radians().tan().asinh()) / (2.0 * PI);
        assert!((v - expected).abs() < 1e-12, "{v} vs {expected}");
        // The poles clamp to the Mercator limit instead of diverging.
        let (_, v) = mercator_uv(DVec3::Z);
        assert!(v.is_finite() && v >= 0.0);
    }

    #[test]
    fn exaggeration_is_clamped() {
        assert_eq!(
            TerrainCfg::new(10.0, 1.0),
            TerrainCfg::new(MAX_EXAGGERATION, 1.0)
        );
        assert_eq!(
            TerrainCfg::new(0.0, 1.0),
            TerrainCfg::new(MIN_EXAGGERATION, 1.0)
        );
        // Relief flattens smoothly toward the altitude it stops at.
        assert_eq!(relief_fade(TERRAIN_FADE_FROM_KM, true), 1.0);
        assert_eq!(relief_fade(TERRAIN_MAX_ALTITUDE_KM, true), 0.0);
        assert_eq!(relief_fade(SHARP_RELIEF_MAX_ALTITUDE_KM, false), 1.0);
        assert_eq!(relief_fade(SHARP_RELIEF_MAX_ALTITUDE_KM + 1.0, false), 0.0);
        let mid = relief_fade((TERRAIN_FADE_FROM_KM + TERRAIN_MAX_ALTITUDE_KM) / 2.0, true);
        assert!((mid - 0.5).abs() < 1e-6);
        assert_eq!(
            TerrainCfg::new(2.0, 0.5).exaggeration,
            TerrainCfg::new(1.0, 1.0).exaggeration
        );
    }

    #[test]
    fn a_block_is_left_through_the_nearer_edge() {
        assert_eq!(block_exit(1.0, 3.0, 4.0, 1.0, 0.0), 3.0);
        assert_eq!(block_exit(1.0, 3.0, 4.0, -0.5, 0.0), 2.0);
        assert_eq!(block_exit(1.0, 3.0, 4.0, 1.0, 1.0), 1.0);
        assert!(block_exit(1.0, 3.0, 4.0, 0.0, 0.0).is_infinite());
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use globecraft_core::earth_model::EarthModel;

    #[test]
    fn anchored_mercator_rates_match_finite_differences() {
        let geometry = EarthModel::Wgs84.geometry();
        let origin = geometry.position(30.0, 79.0) * 1.003;
        for direction in [
            DVec3::new(-0.3, 0.6, -0.2),
            DVec3::new(0.5, -0.1, -0.6),
            DVec3::new(0.0, 0.2, 0.9),
        ] {
            let direction = direction.normalize();
            let t = 1e-3;
            let anchor = Anchor::at(geometry, origin, direction, t);
            let delta = 1e-7;
            let ahead = mercator_uv(
                geometry
                    .normal(origin + direction * (t + delta))
                    .normalize(),
            );
            let expected = (
                (ahead.0 - anchor.uv.0) / delta,
                (ahead.1 - anchor.uv.1) / delta,
            );
            for (got, want) in [(anchor.duv.0, expected.0), (anchor.duv.1, expected.1)] {
                assert!(
                    (got - want).abs() <= 0.01 * want.abs().max(1e-3),
                    "{got} vs {want}"
                );
            }
        }
    }
}

#[cfg(test)]
mod march_tests {
    use super::*;
    use crate::{KeyBuild, Local};
    use globecraft_core::earth_model::EarthModel;
    use std::collections::HashMap;

    fn flat_world(metres: f32) -> impl Fn(TileKey) -> Lookup {
        let tile = Arc::new(Tile::from_elevation(Elevation::from_heights(
            512,
            vec![metres; 512 * 512],
        )));
        move |key| {
            if crate::unkey(key).0 == Provider::TerrainDem {
                Lookup::Ready(tile.clone())
            } else {
                Lookup::Absent
            }
        }
    }

    fn ray_from(
        origin: DVec3,
        direction: DVec3,
        geometry: SurfaceGeometry,
    ) -> (rays::RaySample, bool) {
        let hit = geometry.intersect(origin, direction);
        let sample = rays::RaySample {
            direction,
            camera_dot: 0.0,
            discriminant: 0.0,
            distance: hit.map_or(0.0, |p| (p - origin).length()),
            point: hit.unwrap_or_default(),
            normal: hit.map_or(DVec3::ZERO, |p| geometry.normal(p)),
            incidence: 0.0,
        };
        (sample, hit.is_some())
    }

    /// A 2,000 m plateau at level 5 that arrived half a fade ago, over older 400 m
    /// ground at the coarser levels (`with_stand_in`) or over nothing.
    fn growing_world(now: std::time::Instant, with_stand_in: bool) -> impl Fn(TileKey) -> Lookup {
        let flat = |metres: f32| {
            Tile::from_elevation(Elevation::from_heights(512, vec![metres; 512 * 512]))
        };
        let arrived = now - globecraft_data::tiles::ELEVATION_FADE / 2;
        let new = Arc::new(flat(2_000.0).arriving(arrived));
        let old = Arc::new(flat(400.0));
        move |key| match crate::unkey(key) {
            (Provider::TerrainDem, 5, ..) => Lookup::Ready(new.clone()),
            (Provider::TerrainDem, 0..5, ..) if with_stand_in => Lookup::Ready(old.clone()),
            _ => Lookup::Unavailable,
        }
    }

    /// Ground east of longitude 80° stands `metres` high, west of it at sea level, at
    /// every level alike.
    fn cliff_world(metres: f32) -> impl Fn(TileKey) -> Lookup {
        let tiles = std::sync::Mutex::new(HashMap::<TileKey, Arc<Tile>>::new());
        move |key| {
            let (provider, z, tx, _, _) = crate::unkey(key);
            if provider != Provider::TerrainDem {
                return Lookup::Absent;
            }
            let tile = tiles
                .lock()
                .unwrap()
                .entry(key)
                .or_insert_with(|| {
                    let (size, n) = (64, f64::from(1u32 << z));
                    let heights = (0..size * size)
                        .map(|i| {
                            let u = (f64::from(tx) + ((i % size) as f64 + 0.5) / size as f64) / n;
                            if u * 360.0 - 180.0 > 80.0 {
                                metres
                            } else {
                                0.0
                            }
                        })
                        .collect();
                    Arc::new(Tile::from_elevation(Elevation::from_heights(size, heights)))
                })
                .clone();
            Lookup::Ready(tile)
        }
    }

    #[test]
    fn a_cliff_shades_the_ground_before_it_from_a_sun_behind_it() {
        let geometry = EarthModel::PerfectSphere.geometry();
        let lookup = cliff_world(300.0);
        // 200 m west of the cliff's foot, seen from 5 km straight above.
        let (lat, lon) = (30.0, 80.0 - 0.2 / (111.32 * 30f64.to_radians().cos()));
        let up = globecraft_core::math::ll_to_v3(lat, lon);
        let origin = geometry.position(lat, lon) + up * (5.0 / globecraft_core::EARTH_RADIUS_KM);
        let east = DVec3::Z.cross(up).normalize();
        let light = |sun: DVec3| {
            let mut loc = Local {
                lookup: &lookup,
                map: HashMap::<_, _, KeyBuild>::default(),
                dem: Vec::new(),
                dem_placed: Vec::new(),
                dem_index: HashMap::default(),
                now: std::time::Instant::now(),
            };
            let (ray, hits) = ray_from(origin, -up, geometry);
            let pixel_angle = 1e-3;
            let terrain = TerrainCfg::new(1.0, 1.0);
            let hit = match march(
                &ray,
                0.0,
                hits,
                origin,
                geometry,
                terrain,
                pixel_angle,
                0.0,
                &mut loc,
            ) {
                March::Hit(hit, _) => hit,
                March::Floor => ray,
                March::Miss => panic!("no ground"),
            };
            sun_light(
                &hit,
                sun.normalize(),
                geometry,
                terrain,
                pixel_angle,
                &mut loc,
            )
        };
        let low = 10f64.to_radians();
        // Low in the east, behind the cliff: in its shadow.
        assert_eq!(
            light(up * low.sin() + east * low.cos()),
            TERRAIN_SHADOW_LIGHT
        );
        // Low in the west, or high: in the sun; below the horizon: left to the night.
        assert_eq!(light(up * low.sin() - east * low.cos()), 1.0);
        assert_eq!(light(up), 1.0);
        assert_eq!(light(-up), 1.0);
    }

    #[test]
    fn shading_comes_in_with_the_zoom_instead_of_at_the_level_change() {
        // Level 5 slopes steeply east; the coarser levels are flat.
        let ramp: Vec<f32> = (0..512 * 512).map(|i| (i % 16) as f32 * 400.0).collect();
        let fine = Arc::new(Tile::from_elevation(Elevation::from_heights(512, ramp)));
        let flat = Arc::new(Tile::from_elevation(Elevation::from_heights(
            512,
            vec![0.0; 512 * 512],
        )));
        let lookup = move |key| match crate::unkey(key) {
            (Provider::TerrainDem, 5, ..) => Lookup::Ready(fine.clone()),
            (Provider::TerrainDem, 0..5, ..) => Lookup::Ready(flat.clone()),
            _ => Lookup::Unavailable,
        };
        let geometry = EarthModel::PerfectSphere.geometry();
        let up = globecraft_core::math::ll_to_v3(30.0, 80.0);
        let origin = geometry.position(30.0, 80.0) + up * (60.0 / globecraft_core::EARTH_RADIUS_KM);
        let shade_at = |zoom: f64| {
            let mut loc = Local {
                lookup: &lookup,
                map: HashMap::<_, _, KeyBuild>::default(),
                dem: Vec::new(),
                dem_placed: Vec::new(),
                dem_index: HashMap::default(),
                now: std::time::Instant::now(),
            };
            let (ray, hits) = ray_from(origin, -up, geometry);
            // The pixel angle that puts the ground at `zoom`.
            let t = 60.0 / globecraft_core::EARTH_RADIUS_KM;
            let latitude_cosine = 30f64.to_radians().cos();
            let pixel_angle = 2.0 * PI * latitude_cosine
                / (DEM_TILE_TEXELS * t * DEM_PIXELS_PER_TEXEL * zoom.exp2());
            let March::Hit(_, shade) = march(
                &ray,
                0.0,
                hits,
                origin,
                geometry,
                TerrainCfg::new(1.0, 1.0),
                pixel_angle,
                0.0,
                &mut loc,
            ) else {
                panic!("no hit");
            };
            shade
        };
        let flat_shade = SHADE_BASE as f32;
        let full = shade_at(6.5);
        let difference = (full - flat_shade).abs();
        assert!(difference > 0.04, "level 5 shades differently: {full}");
        // Just past the change to level 5: still nearly the flat level's shading
        // (it was level 5's at once).
        let start = shade_at(5.02);
        assert!((start - flat_shade).abs() < 0.2 * difference, "{start}");
        // Half a level on: half of the way to level 5's.
        let half = shade_at(5.5);
        assert!(
            (half - (flat_shade + full) / 2.0).abs() < 0.2 * difference,
            "{half} {full}"
        );
    }

    #[test]
    fn heights_come_in_with_the_zoom_instead_of_at_the_level_change() {
        // Level 5 stands 2,000 m high; the coarser levels lie at sea level.
        let high = Arc::new(Tile::from_elevation(Elevation::from_heights(
            512,
            vec![2000.0; 512 * 512],
        )));
        let flat = Arc::new(Tile::from_elevation(Elevation::from_heights(
            512,
            vec![0.0; 512 * 512],
        )));
        let lookup = move |key| match crate::unkey(key) {
            (Provider::TerrainDem, 5, ..) => Lookup::Ready(high.clone()),
            (Provider::TerrainDem, 0..5, ..) => Lookup::Ready(flat.clone()),
            _ => Lookup::Unavailable,
        };
        let geometry = EarthModel::PerfectSphere.geometry();
        let up = globecraft_core::math::ll_to_v3(30.0, 80.0);
        let origin = geometry.position(30.0, 80.0) + up * (60.0 / globecraft_core::EARTH_RADIUS_KM);
        let ground_at = |zoom: f64, relief: TerrainCfg| {
            let mut loc = Local {
                lookup: &lookup,
                map: HashMap::<_, _, KeyBuild>::default(),
                dem: Vec::new(),
                dem_placed: Vec::new(),
                dem_index: HashMap::default(),
                now: std::time::Instant::now(),
            };
            let (ray, hits) = ray_from(origin, -up, geometry);
            // The pixel angle that puts the ground (about 60 km away) at `zoom`.
            let t = 60.0 / globecraft_core::EARTH_RADIUS_KM;
            let latitude_cosine = 30f64.to_radians().cos();
            let pixel_angle = 2.0 * PI * latitude_cosine
                / (DEM_TILE_TEXELS * t * DEM_PIXELS_PER_TEXEL * zoom.exp2());
            match march(
                &ray,
                0.0,
                hits,
                origin,
                geometry,
                relief,
                pixel_angle,
                0.0,
                &mut loc,
            ) {
                March::Hit(hit, _) => 60_000.0 - hit.distance * globecraft_core::EARTH_RADIUS_M,
                March::Floor => 0.0,
                March::Miss => panic!("the ray misses the globe"),
            }
        };
        let ground_m_at = |zoom: f64| ground_at(zoom, TerrainCfg::new(1.0, 1.0));
        // Without smoothing, level 5 stands at once.
        let sharp = ground_at(5.01, TerrainCfg::new(1.0, 1.0).without_smoothing());
        assert!((sharp - 2000.0).abs() < 20.0, "{sharp}");
        // Just past the change to level 5: nearly sea level still (it was 2,000 m at
        // once); half way through the blend, half of it; past it, all of it.
        let half = HEIGHT_BLEND_LEVELS / 2.0;
        assert!(ground_m_at(4.98) < 20.0, "{}", ground_m_at(4.98));
        assert!(ground_m_at(5.01) < 150.0, "{}", ground_m_at(5.01));
        let middle = ground_m_at(5.0 + half);
        // Well between: the hit is a little nearer than 60 km, where the zoom is
        // higher, so somewhat over half.
        assert!((500.0..1600.0).contains(&middle), "{middle}");
        assert!(
            (ground_m_at(5.5) - 2000.0).abs() < 20.0,
            "{}",
            ground_m_at(5.5)
        );
    }

    #[test]
    fn arriving_relief_grows_from_what_stood_there() {
        let geometry = EarthModel::PerfectSphere.geometry();
        let now = std::time::Instant::now();
        let up = globecraft_core::math::ll_to_v3(30.0, 80.0);
        let origin = geometry.position(30.0, 80.0) + up * (10.0 / globecraft_core::EARTH_RADIUS_KM);
        // Half grown: halfway from the stand-in's 400 m (or sea level) to 2,000 m.
        for (with_stand_in, ground_km) in [(true, 1.2), (false, 1.0)] {
            let lookup = growing_world(now, with_stand_in);
            let mut loc = Local {
                lookup: &lookup,
                map: HashMap::<_, _, KeyBuild>::default(),
                dem: Vec::new(),
                dem_placed: Vec::new(),
                dem_index: HashMap::default(),
                now,
            };
            let side = up.cross(DVec3::Z).normalize();
            // Straight down, and grazing (where the blocks' tops decide what is skipped).
            for direction in [-up, (side - up * 0.1).normalize()] {
                let (ray, hits) = ray_from(origin, direction, geometry);
                let March::Hit(ground, _) = march(
                    &ray,
                    0.0,
                    hits,
                    origin,
                    geometry,
                    TerrainCfg::new(1.0, 1.0),
                    1e-3,
                    0.0,
                    &mut loc,
                ) else {
                    panic!("no hit on the growing ground");
                };
                let km = (ground.point.length() - 1.0) * globecraft_core::EARTH_RADIUS_KM;
                assert!((km - ground_km).abs() < 0.01, "{with_stand_in}: {km}");
            }
        }
    }

    #[test]
    fn a_ray_stops_on_the_ground_and_mountains_hide_the_floor() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let geometry = model.geometry();
            let lookup = flat_world(2_000.0);
            let mut loc = Local {
                lookup: &lookup,
                map: HashMap::<_, _, KeyBuild>::default(),
                dem: Vec::new(),
                dem_placed: Vec::new(),
                dem_index: HashMap::default(),
                now: std::time::Instant::now(),
            };
            let up = globecraft_core::math::ll_to_v3(30.0, 80.0);
            let origin =
                geometry.position(30.0, 80.0) + up * (10.0 / globecraft_core::EARTH_RADIUS_KM);
            let (ray, hits) = ray_from(origin, -up, geometry);
            let March::Hit(ground, _) = march(
                &ray,
                0.0,
                hits,
                origin,
                geometry,
                TerrainCfg::new(1.0, 1.0),
                1e-3,
                0.0,
                &mut loc,
            ) else {
                panic!("no hit on flat 2 km ground");
            };
            // 8 km below a camera 10 km up.
            let km = ground.distance * globecraft_core::EARTH_RADIUS_KM;
            assert!((km - 8.0).abs() < 0.01, "{model:?}: {km}");
            // A grazing ray that would reach the floor far away meets the plateau first.
            let side = up.cross(DVec3::Z).normalize();
            let grazing = (side - up * 0.1).normalize();
            let (ray, hits) = ray_from(origin, grazing, geometry);
            assert!(hits);
            assert!(matches!(
                march(&ray, 0.0, hits, origin, geometry, TerrainCfg::new(1.0, 1.0), 1e-3, 0.0, &mut loc),
                March::Hit(ground, _) if ground.distance < ray.distance * 0.9
            ));
            // One that clears the plateau over the curve sees past it: sky.
            let (ray, hits) = ray_from(origin, (side - up * 0.04).normalize(), geometry);
            assert!(!hits);
            assert!(matches!(
                march(
                    &ray,
                    0.0,
                    hits,
                    origin,
                    geometry,
                    TerrainCfg::new(1.0, 1.0),
                    1e-3,
                    0.0,
                    &mut loc
                ),
                March::Miss
            ));
            // Looking up: sky.
            let (ray, hits) = ray_from(origin, up, geometry);
            assert!(matches!(
                march(
                    &ray,
                    0.0,
                    hits,
                    origin,
                    geometry,
                    TerrainCfg::new(1.0, 1.0),
                    1e-3,
                    0.0,
                    &mut loc
                ),
                March::Miss
            ));
        }
    }
}
