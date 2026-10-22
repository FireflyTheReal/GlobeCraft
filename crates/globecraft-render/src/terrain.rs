//! Relief for the CPU ray caster: each surface hit is moved along its view ray to the height of
//! a Terrarium elevation model (Mapterhorn), and shaded by the local slope.
//!
//! Elevation tiles come from the same asynchronous memory/disk tile cache as imagery, through the
//! batch-local lookup, so missing tiles are requested with their screen priority. This is a 2.5D
//! correction (parallax and hill shading without occlusion), not a terrain mesh.
use super::{Local, LocalTile, Lookup, Provider, RenderCfg, TileKey, make_key, rays};
use crate::tuning::WEB_MERCATOR_MAX_LATITUDE_DEG;
use glam::DVec3;
use globecraft_core::{EARTH_RADIUS_M, earth_model::SurfaceGeometry};
use std::f64::consts::PI;

/// Terrain is drawn below this camera altitude; above it relief is smaller than a pixel.
pub const TERRAIN_MAX_ALTITUDE_KM: f64 = 150.0;
/// Accepted vertical exaggeration.
pub const MIN_EXAGGERATION: f32 = 0.5;
pub const MAX_EXAGGERATION: f32 = 3.0;

/// Finest elevation zoom requested; finer imagery reuses its relief.
const DEM_MAX_ZOOM: i32 = 15;
/// Elevation is sampled one zoom coarser than imagery: relief varies slowly, tiles are 512 px.
const DEM_ZOOM_OFFSET: i32 = 1;
const DEM_TILE_EDGE_PX: f64 = 512.0;
/// Rays this close to grazing (cosine of incidence) keep the ellipsoid: the correction diverges.
const MIN_INCIDENCE_COSINE: f64 = 0.18;
/// Fixed-point iterations of the height along the ray; two settle within a DEM pixel.
const HEIGHT_ITERATIONS: usize = 2;
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

/// A loaded elevation tile and where it sits, so nearby samples skip the batch's tile map.
struct DemTile {
    tile: std::sync::Arc<globecraft_data::tiles::Tile>,
    level: i32,
    /// Tiles per side at `level`.
    n: f64,
    tx: f64,
    ty: f64,
}

impl DemTile {
    /// The height at `(u, v)` when it falls inside this tile.
    fn height(&self, (u, v): (f64, f64)) -> Option<f64> {
        let (x, y) = (u.rem_euclid(1.0) * self.n - self.tx, v * self.n - self.ty);
        ((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y))
            .then(|| self.tile.sample_terrarium_m(x, y))
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
    if !(0.0..=1.0).contains(&v) {
        return None;
    }
    let u = u.rem_euclid(1.0);
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
            LocalTile::Ready(tile) => {
                return Some(DemTile {
                    tile: tile.clone(),
                    level,
                    n,
                    tx,
                    ty,
                });
            }
            LocalTile::Pending | LocalTile::Unavailable => {}
            LocalTile::Absent(nearest) => *nearest = nearest.min(dist),
        }
    }
    None
}

/// The height at `uv` from `current` when it covers it at the wanted zoom, else from the batch's
/// tiles (which may change `current`).
fn height_near(
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    current: &mut DemTile,
    zoom: i32,
    uv: (f64, f64),
    dist: f32,
) -> Option<f64> {
    if let Some(h) = current.height(uv).filter(|_| current.level == zoom) {
        return Some(h);
    }
    let found = dem_tile(loc, zoom, uv, dist)?;
    let h = found.height(uv);
    *current = found;
    h
}

/// Web Mercator texture coordinates of a surface normal, as `pixel::surface` computes them.
fn mercator_uv(normal: DVec3) -> (f64, f64) {
    let lon = normal.y.atan2(normal.x);
    let max_sine = WEB_MERCATOR_MAX_LATITUDE_DEG.to_radians().sin();
    let s = normal.z.clamp(-max_sine, max_sine);
    // Clamped: at the Mercator limit rounding would otherwise leave the tile range.
    let v = (0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI)).clamp(0.0, 1.0);
    ((lon / (2.0 * PI) + 0.5).rem_euclid(1.0), v)
}

/// The ray moved to the relief surface and its hill-shading factor, or `None` where no relief
/// applies (grazing rays, no elevation loaded yet): the caller keeps the ellipsoid hit.
pub(super) fn displace(
    ray: &rays::RaySample,
    surface_uv: (f64, f64),
    image_zoom: i32,
    dist: f32,
    geometry: SurfaceGeometry,
    cfg: &RenderCfg,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
) -> Option<(rays::RaySample, f32)> {
    let terrain = cfg.terrain?;
    if ray.incidence < MIN_INCIDENCE_COSINE {
        return None;
    }
    let dem_zoom = (image_zoom - DEM_ZOOM_OFFSET).clamp(0, DEM_MAX_ZOOM);
    let mut dem = dem_tile(loc, dem_zoom, surface_uv, dist)?;
    let mut elevation_m = dem.height(surface_uv)?;
    let mut adjusted = *ray;
    for _ in 0..HEIGHT_ITERATIONS {
        // Ray distances are in globe radii, so the height is converted from metres. Moving back
        // by height / cos(incidence) reaches that height above the tangent plane.
        let delta = elevation_m * terrain.exaggeration / EARTH_RADIUS_M / ray.incidence;
        let t = ray.distance - delta;
        // Never reach the camera when it is below a summit.
        if t <= 0.0 || !t.is_finite() {
            break;
        }
        adjusted.distance = t;
        adjusted.point = ray.point + ray.direction * (t - ray.distance);
        adjusted.normal = geometry.normal(adjusted.point);
        adjusted.incidence = (-ray.direction.dot(adjusted.normal)).max(0.0);
        match height_near(loc, &mut dem, dem_zoom, mercator_uv(adjusted.normal), dist) {
            Some(h) => elevation_m = h,
            None => break,
        }
    }
    let shade = hill_shade(&adjusted, elevation_m, &mut dem, terrain, dist, loc);
    Some((adjusted, shade))
}

/// Brightness factor from the slope of the elevation grid at the displaced point, so ridges and
/// valleys read without sunlight. 1 on flat ground.
fn hill_shade(
    at: &rays::RaySample,
    elevation_m: f64,
    dem: &mut DemTile,
    terrain: TerrainCfg,
    dist: f32,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
) -> f32 {
    let (u, v) = mercator_uv(at.normal);
    let zoom = dem.level;
    // One DEM pixel in texture units, east and south (Mercator is conformal: same ground step).
    let step = 1.0 / (DEM_TILE_EDGE_PX * dem.n);
    let east_m = height_near(loc, dem, zoom, (u + step, v), dist).unwrap_or(elevation_m);
    let south_m = height_near(loc, dem, zoom, (u, v + step), dist).unwrap_or(elevation_m);
    let latitude_cosine = at.normal.z.mul_add(-at.normal.z, 1.0).max(0.0).sqrt();
    let run_m = 2.0 * PI * EARTH_RADIUS_M * latitude_cosine.max(MIN_LATITUDE_COSINE) * step;
    let slope_east =
        ((east_m - elevation_m) / run_m * terrain.exaggeration).clamp(-MAX_SLOPE, MAX_SLOPE);
    let slope_north =
        ((elevation_m - south_m) / run_m * terrain.exaggeration).clamp(-MAX_SLOPE, MAX_SLOPE);
    let up = at.normal;
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
}
