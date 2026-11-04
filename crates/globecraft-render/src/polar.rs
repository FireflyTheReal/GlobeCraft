//! The polar caps: past the Web Mercator limit (85.05°) the imagery tiles end, and
//! the last row of tiles used to be stretched to the pole. There the globe samples
//! NASA's Blue Marble in polar stereographic tiles instead (GIBS EPSG:3413 and
//! EPSG:3031), fading in over the two degrees before the limit so no seam shows.
//! The caps render on the CPU: a view that shows one is not given to the GPU path.
use crate::{Local, pixel::Surface, tuning::LOD_SHARPNESS_BIAS};
use emath::{Pos2, Rect};
use glam::Vec4;
use globecraft_core::{camera::Basis, polar::Pole};
use globecraft_data::tiles::{Lookup, Provider, TileKey, make_key};

/// The polar imagery fades in from this latitude (over about 230 km, so the change
/// of tone between the two imageries is gradual)…
pub const CAP_START_DEG: f64 = 83.0;
/// …and replaces the Mercator imagery from here on (its limit).
const CAP_FULL_DEG: f64 = crate::tuning::WEB_MERCATOR_MAX_LATITUDE_DEG;
/// The GIBS polar grid: ±4,194,304 m on both axes, 2 × 2 tiles of 512 px at level 0.
const GRID_HALF_EXTENT_M: f64 = 4_194_304.0;
const LEVEL0_TILES: f64 = 2.0;
const TILE_PX: f64 = 512.0;
/// Rays per side of the coarse grid that finds whether a view shows a cap.
const VIEW_PROBES: usize = 24;

/// How much of the polar imagery shows at a latitude: none before the cap, all past
/// the Mercator limit, smoothly in between.
pub(crate) fn cap_weight(lat_deg: f64) -> f32 {
    let t = ((lat_deg.abs() - CAP_START_DEG) / (CAP_FULL_DEG - CAP_START_DEG)).clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

fn provider(pole: Pole) -> Provider {
    match pole {
        Pole::North => Provider::PolarNorth,
        Pole::South => Provider::PolarSouth,
    }
}

/// Where a projected point falls in the grid: (0, 0) top left, (1, 1) bottom right.
fn grid_position(x_m: f64, y_m: f64) -> (f64, f64) {
    let side = 2.0 * GRID_HALF_EXTENT_M;
    (
        (x_m + GRID_HALF_EXTENT_M) / side,
        (GRID_HALF_EXTENT_M - y_m) / side,
    )
}

/// The tile holding a grid position at `level`, and the position inside it.
fn tile_at(provider: Provider, level: i32, (u, v): (f64, f64)) -> (TileKey, f64, f64) {
    let n = LEVEL0_TILES * f64::from(1u32 << level);
    let (tx, ty) = (
        (u * n).floor().clamp(0.0, n - 1.0),
        (v * n).floor().clamp(0.0, n - 1.0),
    );
    (
        make_key(provider, level as u8, tx as u32, ty as u32, 0),
        u * n - tx,
        v * n - ty,
    )
}

/// The level whose pixels are about as large as the screen's on the ground.
fn level_for(metres_per_pixel: f64, max_level: i32) -> i32 {
    let level0 = 2.0 * GRID_HALF_EXTENT_M / (LEVEL0_TILES * TILE_PX);
    let level = (level0 / metres_per_pixel.max(1e-3)).log2() + LOD_SHARPNESS_BIAS;
    (level.floor() as i32).clamp(0, max_level)
}

/// The polar imagery under a surface pixel, from the finest loaded level at or
/// above its own (missing tiles are asked for); `None` while nothing has loaded.
pub(crate) fn sample(
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    surface: &Surface,
) -> Option<Vec4> {
    let pole = Pole::of(surface.lat);
    let provider = provider(pole);
    let (x, y) = pole.project(surface.lat, surface.lon);
    let position = grid_position(x, y);
    let mut level = level_for(surface.metres_per_pixel, i32::from(provider.max_zoom()));
    while level >= 0 {
        let (key, u, v) = tile_at(provider, level, position);
        if let Some(colour) = loc.tile_color(key, surface.dist, u, v, 1.0) {
            return Some(colour);
        }
        level -= 1;
    }
    None
}

/// Whether a `width` × `height` view shows any of a polar cap.
pub fn shows_polar_cap(basis: &Basis, width: usize, height: usize) -> bool {
    let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(width as f32, height as f32));
    // A pole on screen: the cap may be smaller than the probes' spacing.
    for z in [1.0, -1.0] {
        if basis
            .project(basis.surface_position(90.0 * z, 0.0), rect)
            .is_some_and(|at| rect.contains(at))
        {
            return true;
        }
    }
    (0..VIEW_PROBES).any(|row| {
        (0..VIEW_PROBES).any(|column| {
            let at = Pos2::new(
                (column as f32 + 0.5) / VIEW_PROBES as f32 * rect.width(),
                (row as f32 + 0.5) / VIEW_PROBES as f32 * rect.height(),
            );
            basis
                .pick(rect, at)
                .is_some_and(|(lat, _)| lat.abs() >= CAP_START_DEG)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;

    #[test]
    fn the_cap_fades_in_before_the_mercator_limit() {
        assert_eq!(cap_weight(80.0), 0.0);
        assert_eq!(cap_weight(-83.0), 0.0);
        let middle = cap_weight(84.0);
        assert!(middle > 0.0 && middle < 1.0);
        assert_eq!(cap_weight(85.1), 1.0);
        assert_eq!(cap_weight(-90.0), 1.0);
    }

    #[test]
    fn the_poles_sit_where_the_four_level0_tiles_meet() {
        // Both poles are the grid's centre: the corner of tiles (0,0)…(1,1).
        assert_eq!(grid_position(0.0, 0.0), (0.5, 0.5));
        let (key, u, v) = tile_at(Provider::PolarSouth, 0, grid_position(1.0, -1.0));
        assert_eq!(globecraft_data::tiles::unkey(key).2, 1, "x");
        assert_eq!(globecraft_data::tiles::unkey(key).3, 1, "y");
        assert!(u < 1e-3 && v < 1e-3);
        // Greenland (72° N, 40° W) lies straight below the North Pole in EPSG:3413,
        // so in the lower half; the Antarctic Peninsula (65° S, 60° W) upper left.
        let (u, v) = {
            let (x, y) = Pole::North.project(72.0, -40.0);
            grid_position(x, y)
        };
        assert!(v > 0.5 && (u - 0.5).abs() < 0.05, "({u}, {v})");
        let (u, v) = {
            let (x, y) = Pole::South.project(-65.0, -60.0);
            grid_position(x, y)
        };
        assert!(u < 0.5 && v < 0.5, "({u}, {v})");
        // Level 4 is the finest (512 m a pixel); a close view asks for it.
        assert_eq!(level_for(400.0, 4), 4);
        assert_eq!(level_for(20_000.0, 4), 0);
    }

    #[test]
    fn a_view_of_a_pole_shows_its_cap_and_a_view_of_europe_does_not() {
        let view = |lat: f64, alt: f64| {
            Cam {
                lat,
                lon: 10.0,
                heading: 0.0,
                alt,
                tilt: 0.0,
            }
            .basis(4.0 / 3.0)
        };
        assert!(shows_polar_cap(&view(-89.0, 2_000.0), 800, 600));
        assert!(shows_polar_cap(&view(70.0, 9_000.0), 800, 600));
        assert!(!shows_polar_cap(&view(50.0, 2_000.0), 800, 600));
    }
}
