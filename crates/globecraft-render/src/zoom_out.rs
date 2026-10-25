//! Imagery worth loading ahead of a zoom out.
//!
//! The view requests only what it shows: the level its centre needs, and coarser
//! levels where the ground recedes. Zoomed out, each coarser level covers the same
//! number of tiles around the centre as the current level does now (the view grows as
//! the tiles do), and none of those tiles were needed before, so the coarser imagery
//! faded in tile by tile. This lists them for the levels a zoom out passes through.
use crate::tuning::{LOD_SHARPNESS_BIAS, TILE_SIZE_PX};
use globecraft_core::{EARTH_RADIUS_M, VERTICAL_FOV_HALF_TAN, camera::Cam};
use globecraft_data::tiles::{Provider, TileKey, make_key};
use std::f64::consts::PI;

/// Levels above the view's that are loaded ahead (a zoom out of 8×).
const ZOOM_OUT_LEVELS: i32 = 3;
/// Tiles beyond the view's edge at each level.
const MARGIN_TILES: i64 = 1;
/// Half-width of a level's block, in tiles, at most (bounds memory and downloads).
const MAX_HALF_SPAN_TILES: i64 = 6;
const MAX_MERCATOR_LAT: f64 = 85.051_128_78;

/// The imagery tiles a zoom out from `cam` will show, nearest level first and, within a
/// level, nearest the centre first. `height_px` and `aspect` describe the displayed view.
pub fn zoom_out_tiles(
    cam: Cam,
    height_px: usize,
    aspect: f64,
    provider: Provider,
    releases: &[u16],
) -> Vec<TileKey> {
    if !provider.is_imagery() || height_px == 0 || !cam.alt.is_finite() || cam.alt <= 0.0 {
        return Vec::new();
    }
    let lat = cam.lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT);
    let coslat = lat.to_radians().cos().max(0.05);
    // The view centre's level, as the renderer chooses it straight down.
    let metres_per_px = cam.alt * 1000.0 * 2.0 * VERTICAL_FOV_HALF_TAN / height_px as f64;
    let ideal = ((2.0 * PI * EARTH_RADIUS_M * coslat) / (TILE_SIZE_PX * metres_per_px)).log2()
        + LOD_SHARPNESS_BIAS;
    let zoom = (ideal.floor() as i32).clamp(0, i32::from(provider.max_zoom()));
    // The view's half-size in tiles of its own level; the same at every coarser level
    // once zoomed out to it.
    let tile_km = |z: i32| 40_075.0 * coslat / f64::from(1_u32 << z);
    let half_height_km = cam.alt * VERTICAL_FOV_HALF_TAN;
    let span =
        |km: f64| ((km / tile_km(zoom)).ceil() as i64 + MARGIN_TILES).min(MAX_HALF_SPAN_TILES);
    let (half_x, half_y) = (span(half_height_km * aspect), span(half_height_km));
    let mut levels: Vec<Vec<TileKey>> = Vec::new();
    for level in (zoom - ZOOM_OUT_LEVELS).max(0)..zoom {
        let n = 1_i64 << level;
        let (cx, cy) = tile_of(level, lat, cam.lon);
        let mut block: Vec<(i64, u32, u32)> = Vec::new();
        for dy in -half_y.min(n / 2)..=half_y.min(n / 2) {
            let y = cy + dy;
            if !(0..n).contains(&y) {
                continue;
            }
            for dx in -half_x.min(n / 2)..=half_x.min(n / 2) {
                let x = (cx + dx).rem_euclid(n);
                block.push((dx * dx + dy * dy, x as u32, y as u32));
            }
        }
        block.sort_unstable();
        block.dedup_by_key(|(_, x, y)| (*x, *y));
        let keys = block.iter().flat_map(|&(_, x, y)| {
            releases
                .iter()
                .map(move |&release| make_key(provider, level as u8, x, y, release))
        });
        levels.push(keys.collect::<Vec<_>>());
    }
    // The levels a zoom out reaches first come first.
    levels.into_iter().rev().flatten().collect()
}

fn tile_of(z: i32, lat: f64, lon: f64) -> (i64, i64) {
    let n = (1_i64 << z) as f64;
    let x = ((lon + 180.0) / 360.0 * n).floor().clamp(0.0, n - 1.0);
    let y = ((1.0 - lat.to_radians().tan().asinh() / PI) / 2.0 * n)
        .floor()
        .clamp(0.0, n - 1.0);
    (x as i64, y as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_data::tiles::unkey;

    #[test]
    fn a_close_view_loads_the_next_coarser_levels_around_it() {
        let cam = Cam {
            lat: 48.1,
            lon: 11.6,
            heading: 0.0,
            alt: 5.0,
            tilt: 0.0,
        };
        let keys = zoom_out_tiles(cam, 1600, 1.6, Provider::Satellite, &[0]);
        let levels: Vec<u8> = keys.iter().map(|&k| unkey(k).1).collect();
        let top = levels[0];
        assert_eq!(
            levels.iter().copied().max(),
            Some(top),
            "nearest level first"
        );
        let distinct: std::collections::BTreeSet<u8> = levels.iter().copied().collect();
        assert_eq!(distinct.len(), ZOOM_OUT_LEVELS as usize);
        // Each level is a block around the centre, centre first.
        let first = unkey(keys[0]);
        let (cx, cy) = tile_of(i32::from(first.1), cam.lat, cam.lon);
        assert_eq!((first.2 as i64, first.3 as i64), (cx, cy));
        let per_level = keys.iter().filter(|&&k| unkey(k).1 == top).count();
        assert!((9..=169).contains(&per_level), "{per_level}");
    }

    #[test]
    fn only_imagery_loads_ahead_and_compare_releases_both_load() {
        let cam = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 20.0,
            tilt: 0.0,
        };
        assert!(zoom_out_tiles(cam, 1000, 1.5, Provider::Streets, &[0]).is_empty());
        let both = zoom_out_tiles(cam, 1000, 1.5, Provider::Wayback, &[3, 9]);
        let releases: std::collections::BTreeSet<u16> = both.iter().map(|&k| unkey(k).4).collect();
        assert_eq!(releases, [3, 9].into_iter().collect());
    }
}
