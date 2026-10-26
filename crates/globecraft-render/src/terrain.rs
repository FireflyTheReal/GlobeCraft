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

/// Terrain is drawn below this camera altitude; above it relief is smaller than a pixel.
pub const TERRAIN_MAX_ALTITUDE_KM: f64 = 150.0;
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
/// The elevation zoom is chosen again once the distance grew by this factor.
const ZOOM_RECHECK_RATIO: f64 = 1.2;
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
}

impl TerrainCfg {
    /// Relief with the given vertical exaggeration, clamped to the accepted range.
    pub fn new(exaggeration: f32) -> Self {
        Self {
            exaggeration: f64::from(exaggeration.clamp(MIN_EXAGGERATION, MAX_EXAGGERATION)),
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

/// A loaded elevation tile (by its index in the batch's `Local::dem`) and where it sits.
#[derive(Clone, Copy)]
struct DemTile {
    index: usize,
    /// Tiles per side at the tile's level.
    n: f64,
    tx: f64,
    ty: f64,
    /// Texels per side.
    size: f64,
}

impl DemTile {
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
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        (0.0..self.size).contains(&x) && (0.0..self.size).contains(&y)
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
    for level in (0..=zoom.clamp(0, DEM_MAX_ZOOM)).rev() {
        let n = (1u64 << level) as f64;
        let tx = (u * n).floor().min(n - 1.0);
        let ty = (v * n).floor().min(n - 1.0);
        let key = make_key(Provider::TerrainDem, level as u8, tx as u32, ty as u32, 0);
        let tile = loc
            .map
            .entry(key)
            .or_insert_with(|| match (loc.lookup)(key) {
                Lookup::Ready(tile) => LocalTile::Ready(tile),
                Lookup::Pending => LocalTile::Pending,
                Lookup::Absent => LocalTile::Absent(dist),
                Lookup::Unavailable => LocalTile::Unavailable,
            });
        match tile {
            LocalTile::Ready(tile) if tile.elevation().is_some() => {
                let size = tile.elevation().map_or(0, Elevation::size) as f64;
                let index = match loc.dem.iter().position(|held| Arc::ptr_eq(held, tile)) {
                    Some(index) => index,
                    None => {
                        loc.dem.push(tile.clone());
                        loc.dem.len() - 1
                    }
                };
                return Some(DemTile {
                    index,
                    n,
                    tx,
                    ty,
                    size,
                });
            }
            LocalTile::Ready(_) | LocalTile::Pending | LocalTile::Unavailable => {}
            LocalTile::Absent(nearest) => *nearest = nearest.min(dist),
        }
    }
    None
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

/// The elevation zoom for a point `t` away: texels of about `DEM_PIXELS_PER_TEXEL`
/// pixels, given the angle per pixel `pixel_angle`.
fn dem_zoom(t: f64, pixel_angle: f64, latitude_cosine: f64, tile_texels: f64) -> i32 {
    let metres_per_pixel = (t * EARTH_RADIUS_M * pixel_angle).max(0.01);
    let texel_m = metres_per_pixel * DEM_PIXELS_PER_TEXEL;
    let zoom = (2.0 * PI * EARTH_RADIUS_M * latitude_cosine.max(MIN_LATITUDE_COSINE)
        / (tile_texels * texel_m))
        .log2();
    (zoom.floor() as i32).clamp(0, DEM_MAX_ZOOM)
}

/// Marches `ray` from the camera at `origin` through the relief. `hits_surface` says
/// whether the ray meets the ellipsoid (at `ray.distance`); `pixel_angle` is the view
/// angle per displayed pixel; `dist` the pixel's screen priority for tile requests.
#[allow(clippy::too_many_arguments)]
pub(super) fn march(
    ray: &rays::RaySample,
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
    let mut t = enter;
    // The last point known to be above the ground and its clearance, for the hit.
    let mut above: Option<(f64, f64)> = None;
    let mut current: Option<DemTile> = None;
    let mut looked_up: Option<i32> = None;
    let mut anchor = Anchor::at(geometry, origin, direction, t);
    let (mut zoom, mut zoom_until) = (0, f64::NEG_INFINITY);
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
        if t > zoom_until {
            zoom = dem_zoom(t, pixel_angle, anchor.latitude_cosine, DEM_TILE_TEXELS);
            zoom_until = t * ZOOM_RECHECK_RATIO;
        }
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
        }
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
        let elevation = tile.elevation(&loc.dem);
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
            let top = f64::from(elevation.block_max_m(level, bx, by)) * to_world;
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
                let ground = f64::from(elevation.height_m(x, y)) * to_world;
                if height <= ground {
                    let t = refine(above, t, height - ground, |t| {
                        let (x, y) = tile.texel(anchor.uv(t));
                        height_above(origin + direction * t, q)
                            - f64::from(elevation.height_m(x, y)) * to_world
                    });
                    let point = origin + direction * t;
                    let normal = geometry.normal(point).normalize();
                    let shade = hill_shade(&tile, elevation, anchor.uv(t), normal, to_world);
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
fn hill_shade(
    tile: &DemTile,
    elevation: &Elevation,
    uv: (f64, f64),
    up: DVec3,
    to_world: f64,
) -> f32 {
    let (x, y) = tile.texel(uv);
    let height = |x: f64, y: f64| f64::from(elevation.height_m(x, y));
    let latitude_cosine = up.x.hypot(up.y).max(MIN_LATITUDE_COSINE);
    // One texel on the ground (Mercator is conformal: the same both ways).
    let run_m = 2.0 * PI * EARTH_RADIUS_M * latitude_cosine / (tile.n * elevation.size() as f64);
    let exaggeration = to_world * EARTH_RADIUS_M;
    let slope_east = ((height(x + 1.0, y) - height(x - 1.0, y)) / (2.0 * run_m) * exaggeration)
        .clamp(-MAX_SLOPE, MAX_SLOPE);
    let slope_north = ((height(x, y - 1.0) - height(x, y + 1.0)) / (2.0 * run_m) * exaggeration)
        .clamp(-MAX_SLOPE, MAX_SLOPE);
    let east = DVec3::new(-up.y, up.x, 0.0).normalize_or_zero();
    let north = up.cross(east).normalize_or_zero();
    let relief_normal = (up - east * slope_east - north * slope_north).normalize_or_zero();
    let light = (up * LIGHT_UP + north * LIGHT_NORTH - east * LIGHT_WEST).normalize();
    let flat = up.dot(light);
    let relief = relief_normal.dot(light).max(0.0);
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
        assert_eq!(TerrainCfg::new(10.0), TerrainCfg::new(MAX_EXAGGERATION));
        assert_eq!(TerrainCfg::new(0.0), TerrainCfg::new(MIN_EXAGGERATION));
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

    #[test]
    fn a_ray_stops_on_the_ground_and_mountains_hide_the_floor() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let geometry = model.geometry();
            let lookup = flat_world(2_000.0);
            let mut loc = Local {
                lookup: &lookup,
                map: HashMap::<_, _, KeyBuild>::default(),
                dem: Vec::new(),
            };
            let up = globecraft_core::math::ll_to_v3(30.0, 80.0);
            let origin =
                geometry.position(30.0, 80.0) + up * (10.0 / globecraft_core::EARTH_RADIUS_KM);
            let (ray, hits) = ray_from(origin, -up, geometry);
            let March::Hit(ground, _) = march(
                &ray,
                hits,
                origin,
                geometry,
                TerrainCfg::new(1.0),
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
                march(&ray, hits, origin, geometry, TerrainCfg::new(1.0), 1e-3, 0.0, &mut loc),
                March::Hit(ground, _) if ground.distance < ray.distance * 0.9
            ));
            // One that clears the plateau over the curve sees past it: sky.
            let (ray, hits) = ray_from(origin, (side - up * 0.04).normalize(), geometry);
            assert!(!hits);
            assert!(matches!(
                march(
                    &ray,
                    hits,
                    origin,
                    geometry,
                    TerrainCfg::new(1.0),
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
                    hits,
                    origin,
                    geometry,
                    TerrainCfg::new(1.0),
                    1e-3,
                    0.0,
                    &mut loc
                ),
                March::Miss
            ));
        }
    }
}
