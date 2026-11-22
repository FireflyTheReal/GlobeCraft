//! Which part of a wall or roof the sun reaches past the other blocks (roadmap C3b,
//! Julien 2026-11-19: "the light must climb the walls at sunset"; 2026-11-22: "the
//! first light arriving on building tops and wandering down"). From a spot, the march
//! goes toward the sun over the blocks' surface model (`Heights`, one per tile) and
//! keeps the highest line of sight any block cuts: the spot is lit from that height
//! up. As the sun rises the line drops down the walls.
use glam::DVec3;
use globecraft_data::buildings::{BuildingTile, Heights};
use std::sync::Arc;

/// The march's step: about the surface model's cell.
const STEP_M: f64 = 6.0;
/// Blocks further than this toward the sun are not looked at: with the sun low a
/// shadow would reach for kilometres, past the tiles held.
const REACH_M: f64 = 1_500.0;
/// The sun counted at least this high (the slope of its line, about 0.3°): lower,
/// every line toward it is cut.
const MIN_RISE: f64 = 0.005;
const METRES_PER_DEGREE: f64 = globecraft_core::EARTH_RADIUS_M * std::f64::consts::PI / 180.0;

/// The march over the tiles of a frame's blocks toward `sun` (a unit vector).
/// shortcut: every block stands on the same level ground as the spot marched from,
/// so a block on a hill shades as if it stood in the valley; a terrain-aware model
/// needs the relief's heights here.
pub(super) struct SunMarch<'a> {
    tiles: &'a [Arc<BuildingTile>],
    sun: DVec3,
    tallest_m: f64,
}

impl<'a> SunMarch<'a> {
    pub(super) fn new(tiles: &'a [Arc<BuildingTile>], sun: DVec3) -> Self {
        let tallest_m = tiles
            .iter()
            .map(|tile| f64::from(tile.heights.tallest_m))
            .fold(0.0, f64::max);
        Self {
            tiles,
            sun,
            tallest_m,
        }
    }

    /// The lowest height (metres above the ground) the sun reaches at (`lat`, `lon`),
    /// where `up` is the vertical: 0 where nothing stands toward it.
    pub(super) fn lit_from(&self, lat: f64, lon: f64, up: DVec3) -> f64 {
        let east = DVec3::Z.cross(up).normalize_or(DVec3::X);
        let north = up.cross(east);
        let (toward_east, toward_north) = (self.sun.dot(east), self.sun.dot(north));
        let across = toward_east.hypot(toward_north);
        if across < 1e-9 {
            return 0.0;
        }
        let rise = (self.sun.dot(up) / across).max(MIN_RISE);
        let lat_per_m = toward_north / across / METRES_PER_DEGREE;
        let lon_per_m = toward_east / across / (METRES_PER_DEGREE * lat.to_radians().cos());
        let reach = (self.tallest_m / rise).min(REACH_M);
        let mut lit = 0.0f64;
        let mut heights: Option<&Heights> = None;
        let mut distance = STEP_M;
        while distance <= reach && self.tallest_m - distance * rise > lit {
            let (lat, lon) = (lat + lat_per_m * distance, lon + lon_per_m * distance);
            let height = heights.and_then(|grid| grid.at(lat, lon)).or_else(|| {
                heights = self
                    .tiles
                    .iter()
                    .map(|tile| &tile.heights)
                    .find(|grid| grid.at(lat, lon).is_some());
                heights.and_then(|grid| grid.at(lat, lon))
            });
            if let Some(height) = height {
                lit = lit.max(f64::from(height) - distance * rise);
            }
            distance += STEP_M;
        }
        lit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_data::buildings::Building;

    fn block(lat: f64, lon: f64, half_deg: f64, top_m: f32) -> Building {
        Building {
            rings: vec![vec![
                (lat - half_deg, lon - half_deg),
                (lat - half_deg, lon + half_deg),
                (lat + half_deg, lon + half_deg),
                (lat + half_deg, lon - half_deg),
            ]],
            walls: vec![vec![true; 4]],
            roof: vec![0, 1, 2, 0, 2, 3],
            base_m: 0.0,
            top_m,
            roof_colour: None,
            wall_colour: None,
        }
    }

    /// A tower east of a spot shades it from the low eastern sun up to a height that
    /// drops as the sun climbs (the light wanders down the wall); with the sun in the
    /// west nothing does.
    #[test]
    fn a_tower_toward_the_sun_shades_up_to_a_height_that_drops_as_it_climbs() {
        let (lat, lon) = (40.75, -73.99);
        // A 100 m tower about 40 m east (its near face about 30 m away).
        let buildings = vec![block(lat, lon + 0.000_47, 0.000_12, 100.0)];
        let heights = Heights::of(
            (lat + 0.01, lon - 0.01),
            (lat - 0.01, lon + 0.01),
            &buildings,
        );
        let tiles = vec![Arc::new(BuildingTile { buildings, heights })];
        let up = globecraft_core::math::ll_to_v3(lat, lon);
        let east = DVec3::Z.cross(up).normalize();
        let sun_at = |toward: DVec3, degrees: f64| {
            let height = degrees.to_radians();
            (up * height.sin() + toward * height.cos()).normalize()
        };
        let lit_from = |sun| SunMarch::new(&tiles, sun).lit_from(lat, lon, up);
        let low = lit_from(sun_at(east, 5.0));
        let higher = lit_from(sun_at(east, 30.0));
        assert!(low > 90.0 && low < 100.0, "{low}");
        assert!(higher > 70.0 && higher < low, "{higher}");
        assert_eq!(lit_from(sun_at(-east, 5.0)), 0.0);
    }
}
