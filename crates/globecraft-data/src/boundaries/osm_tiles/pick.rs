//! Which tiles the view needs: a zoom level for the altitude, and the tiles of that
//! level around the view, nearest first.
use super::content::{MAX_MERCATOR_LAT, TileKey};

/// The deepest level fetched. At zoom 12 OpenMapTiles' boundary layer already holds
/// every `admin_level` up to 10; deeper tiles add only buildings and roads (hundreds of
/// kilobytes the border layer throws away), so closer views draw zoom 12's lines.
const MAX_ZOOM: u8 = 12;
const EARTH_CIRCUMFERENCE_KM: f64 = 40_075.0;
const EARTH_RADIUS_KM: f64 = 6_371.0;
const KM_PER_DEGREE: f64 = 111.32;
/// Ground width shown per kilometre of camera altitude, for the default field of view
/// (as in `municipal::topology`).
const VIEW_WIDTH_PER_ALTITUDE: f64 = 1.5;
/// Tiles across the view. A tile's 4,096 units still give a line point every pixel or
/// two at two tiles across a 3,000-pixel window, and each OpenMapTiles tile carries
/// every layer (roads, buildings: often 100–500 KB), so one level coarser than
/// imagery-like detail (four across) takes a quarter of the downloads (user,
/// 2026-10-25: zooming in loaded slower than before).
const TILES_ACROSS_VIEW: f64 = 2.0;
/// The finer choice, four tiles across, that decides where municipal lines start: they
/// exist only from `MUNICIPAL_MIN_ZOOM`, and the coarser choice would reach that level
/// only at half the altitude.
const DETAIL_TILES_ACROSS_VIEW: f64 = 4.0;
/// Ground radius around the view centre whose tiles are wanted, per km of altitude:
/// a tilted camera sees farther than straight down.
const RADIUS_PER_ALTITUDE: f64 = 1.0;
/// More tiles than this go one level up instead.
const MAX_TILES: usize = 96;

/// The zoom level and the tiles around the view, nearest first.
pub(super) fn wanted(lat: f64, lon: f64, altitude_km: f64) -> (u8, Vec<TileKey>) {
    let altitude_km = altitude_km.max(0.05);
    let lat = lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT);
    // Mercator tiles shrink with cos(latitude) on the ground.
    let ground_scale = lat.to_radians().cos().max(0.05);
    let level = |tiles_across: f64| {
        let tile_km_wanted = altitude_km * VIEW_WIDTH_PER_ALTITUDE / tiles_across;
        let ideal = (EARTH_CIRCUMFERENCE_KM * ground_scale / tile_km_wanted).log2();
        ideal.round().clamp(0.0, f64::from(MAX_ZOOM)) as u8
    };
    let mut zoom = level(TILES_ACROSS_VIEW);
    if level(DETAIL_TILES_ACROSS_VIEW) >= super::MUNICIPAL_MIN_ZOOM {
        zoom = zoom.max(super::MUNICIPAL_MIN_ZOOM);
    }
    // Never beyond the horizon.
    let horizon_deg = (EARTH_RADIUS_KM / (EARTH_RADIUS_KM + altitude_km))
        .acos()
        .to_degrees();
    let radius_deg = (altitude_km * RADIUS_PER_ALTITUDE / KM_PER_DEGREE).min(horizon_deg);
    loop {
        let tiles = around(zoom, lat, lon, radius_deg);
        if tiles.len() <= MAX_TILES || zoom == 0 {
            return (zoom, tiles);
        }
        zoom -= 1;
    }
}

/// The highest camera altitude at `lat` whose tiles hold municipal lines (the
/// inverse of the detail choice in [`wanted`], which rounds the ideal zoom).
pub(super) fn max_altitude_km_for_municipal(lat: f64) -> f64 {
    let z = super::MUNICIPAL_MIN_ZOOM;
    let lat = lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT);
    let ground_scale = lat.to_radians().cos().max(0.05);
    EARTH_CIRCUMFERENCE_KM * ground_scale * DETAIL_TILES_ACROSS_VIEW
        / (VIEW_WIDTH_PER_ALTITUDE * (f64::from(z) - 0.5).exp2())
}

fn around(zoom: u8, lat: f64, lon: f64, radius_deg: f64) -> Vec<TileKey> {
    let n = TileKey::tiles_per_side(zoom) as i64;
    let lon_radius = radius_deg / lat.to_radians().cos().max(0.05);
    let north = (lat + radius_deg).min(MAX_MERCATOR_LAT);
    let south = (lat - radius_deg).max(-MAX_MERCATOR_LAT);
    let top = TileKey::at(zoom, north, lon).y as i64;
    let bottom = TileKey::at(zoom, south, lon).y as i64;
    let (west_x, east_x) = if lon_radius >= 180.0 {
        (0, n - 1)
    } else {
        let column = |lon: f64| ((lon + 180.0) / 360.0 * n as f64).floor() as i64;
        (column(lon - lon_radius), column(lon + lon_radius))
    };
    let centre = TileKey::at(zoom, lat, lon);
    let (cx, cy) = (centre.x as i64, centre.y as i64);
    let mut tiles: Vec<(i64, TileKey)> = Vec::new();
    for y in top..=bottom {
        for x in west_x..=east_x.min(west_x + n - 1) {
            let wrapped = x.rem_euclid(n);
            let dx = (x - cx).abs().min(n - (x - cx).abs());
            tiles.push((
                dx * dx + (y - cy).pow(2),
                TileKey {
                    z: zoom,
                    x: wrapped as u32,
                    y: y as u32,
                },
            ));
        }
    }
    tiles.sort_by_key(|&(distance, key)| (distance, key));
    tiles.dedup_by_key(|(_, key)| *key);
    tiles.into_iter().map(|(_, key)| key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_follows_altitude_and_the_tile_count_stays_bounded() {
        let zoom_at = |alt| wanted(49.0, 8.4, alt).0;
        assert!((1..=2).contains(&zoom_at(20_000.0)));
        assert!((5..=7).contains(&zoom_at(400.0)), "{}", zoom_at(400.0));
        assert!((10..=11).contains(&zoom_at(40.0)), "{}", zoom_at(40.0));
        assert_eq!(zoom_at(0.5), MAX_ZOOM);
        for alt in [0.5, 5.0, 40.0, 400.0, 4_000.0, 40_000.0] {
            let (zoom, tiles) = wanted(49.0, 8.4, alt);
            assert!(
                !tiles.is_empty() && tiles.len() <= MAX_TILES,
                "{alt} km: {}",
                tiles.len()
            );
            assert_eq!(tiles[0], TileKey::at(zoom, 49.0, 8.4), "nearest first");
        }
    }

    #[test]
    fn municipal_lines_start_where_the_detail_level_reaches_them() {
        for lat in [0.0, 47.5, 64.0] {
            let top = max_altitude_km_for_municipal(lat);
            assert!(wanted(lat, 8.0, top * 0.99).0 >= super::super::MUNICIPAL_MIN_ZOOM);
            assert!(wanted(lat, 8.0, top * 1.01).0 < super::super::MUNICIPAL_MIN_ZOOM);
        }
    }

    #[test]
    fn border_tiles_are_a_level_coarser_than_detail_above_the_municipal_range() {
        // 400 km: four across would be zoom 7 or so; two across is one level up and
        // a quarter of the tiles.
        let (zoom, tiles) = wanted(49.0, 8.4, 400.0);
        assert!((5..=7).contains(&zoom), "{zoom}");
        assert!(tiles.len() <= 16, "{}", tiles.len());
    }

    #[test]
    fn tiles_wrap_across_the_date_line() {
        let (zoom, tiles) = wanted(-17.0, 179.9, 300.0);
        let n = TileKey::tiles_per_side(zoom);
        assert!(tiles.iter().any(|t| t.x == 0) && tiles.iter().any(|t| t.x == n - 1));
    }
}
