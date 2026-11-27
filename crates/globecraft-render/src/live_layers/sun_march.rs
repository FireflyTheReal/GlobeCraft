//! Which part of a wall or roof the sun reaches past the other blocks (roadmap C3b,
//! Julien 2026-11-19: "the light must climb the walls at sunset"; 2026-11-22: "the
//! first light arriving on building tops and wandering down"). From a spot, the march
//! goes toward the sun over the blocks' surface model (`Heights`, one per tile) and
//! keeps the highest line of sight any block cuts: the spot is lit from that height
//! up. As the sun rises the line drops down the walls.
//!
//! A block's results depend on the sun and the tiles only, not on the camera: they are
//! kept (`KEPT`) while the sun and the tiles held stay put, so a moving camera marches
//! only for blocks it has not seen yet (Julien, 2026-11-22: "pretty laggy").
use glam::DVec3;
use globecraft_data::buildings::{Building, BuildingTile, Heights};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

/// The march's first step, about the surface model's cell; each step is longer by
/// this share of the distance gone, so far blocks are passed in fewer steps.
const STEP_M: f64 = 6.0;
const STEP_GROWTH: f64 = 0.02;
/// Blocks further than this toward the sun are not looked at: with the sun low a
/// shadow would reach for kilometres, past the tiles held.
const REACH_M: f64 = 1_500.0;
/// The sun counted at least this high (the slope of its line, about 0.3°): lower,
/// every line toward it is cut.
const MIN_RISE: f64 = 0.005;
/// A wall's light is found this far out from it (about a cell of `Heights`), clear of
/// its own block.
const WALL_OUT_M: f64 = 6.0;
/// The length of a wall's pieces (as the drawing splits them).
pub(super) const WALL_PIECE_M: f64 = 15.0;
/// Blocks further than this from the camera are marched once, from their middle:
/// their pieces are too small to tell apart.
pub(super) const ONE_MARCH_BEYOND_M: f64 = 1_200.0;
/// Results are kept for the sun's direction to about this (radians, ~0.03°); at most
/// this many blocks.
const SUN_STEP: f64 = 5e-4;
const MAX_KEPT: usize = 200_000;
pub(super) const METRES_PER_DEGREE: f64 =
    globecraft_core::EARTH_RADIUS_M * std::f64::consts::PI / 180.0;

/// Blocks' results for one sun direction and set of tiles.
static KEPT: Mutex<Option<Kept>> = Mutex::new(None);
/// The sun's direction and the tiles held, and the blocks' results for them.
type Kept = (SunKey, HashMap<u64, Arc<BlockLight>>);
type SunKey = ([i64; 3], u64);

/// Where the sun reaches a block.
#[derive(Debug)]
pub(super) struct BlockLight {
    /// For each wall piece, outline then courtyards, edge by edge: the height above
    /// the ground (metres) the sun reaches from; 0 for walls turned away.
    pub(super) walls: Vec<f64>,
    /// Whether the sun reaches the roof.
    pub(super) roof: bool,
}

/// The march over the tiles of a frame's blocks toward `sun` (a unit vector).
/// shortcut: every block stands on the same level ground as the spot marched from,
/// so a block on a hill shades as if it stood in the valley; a terrain-aware model
/// needs the relief's heights here.
pub(super) struct SunMarch<'a> {
    tiles: &'a [Arc<BuildingTile>],
    sun: DVec3,
    tallest_m: f64,
    /// Which tiles are held: a tile come in toward the sun shades blocks lit before.
    tiles_key: u64,
}

/// A line toward the sun from a spot, in degrees per metre and metres up per metre.
struct Toward {
    lat_per_m: f64,
    lon_per_m: f64,
    rise: f64,
}

impl<'a> SunMarch<'a> {
    pub(super) fn new(tiles: &'a [Arc<BuildingTile>], sun: DVec3) -> Self {
        let tallest_m = tiles
            .iter()
            .map(|tile| f64::from(tile.heights.tallest_m))
            .fold(0.0, f64::max);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for tile in tiles {
            Arc::as_ptr(tile).hash(&mut hasher);
        }
        Self {
            tiles,
            sun,
            tallest_m,
            tiles_key: hasher.finish(),
        }
    }

    /// Where the sun reaches `building` (its middle at (`lat`, `lon`), `up` there),
    /// from the kept results when the sun has not moved; `far` marches it once.
    pub(super) fn block(
        &self,
        building: &Building,
        (lat, lon): (f64, f64),
        up: DVec3,
        far: bool,
    ) -> Arc<BlockLight> {
        let key = (
            (self.sun / SUN_STEP)
                .round()
                .to_array()
                .map(|axis| axis as i64),
            self.tiles_key,
        );
        let id = block_id(building, far);
        let lock = || KEPT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((kept_key, blocks)) = lock().as_ref()
            && *kept_key == key
            && let Some(light) = blocks.get(&id)
        {
            return Arc::clone(light);
        }
        let light = Arc::new(self.march_block(building, (lat, lon), up, far));
        let mut kept = lock();
        match kept.as_mut() {
            Some((kept_key, blocks)) if *kept_key == key && blocks.len() < MAX_KEPT => {
                blocks.insert(id, Arc::clone(&light));
            }
            _ => *kept = Some((key, HashMap::from([(id, Arc::clone(&light))]))),
        }
        light
    }

    fn march_block(
        &self,
        building: &Building,
        (lat, lon): (f64, f64),
        up: DVec3,
        far: bool,
    ) -> BlockLight {
        let toward = self.toward(lat, up);
        let top_m = f64::from(building.top_m);
        let mut walls = Vec::new();
        // Far off, one march from just past the block's own footprint stands for all.
        let whole = far.then(|| {
            let start = block_reach_m(building, lat) + WALL_OUT_M;
            self.lit_from(lat, lon, &toward, start, top_m)
        });
        let roof =
            whole.unwrap_or_else(|| self.lit_from(lat, lon, &toward, STEP_M, top_m)) <= top_m;
        let mut first = true;
        for (ring, ring_walls) in building.rings.iter().zip(&building.walls) {
            let n = ring.len();
            let turn = wall_turn(ring, first);
            first = false;
            for (index, &is_wall) in ring_walls.iter().enumerate() {
                if !is_wall {
                    continue;
                }
                let (start, end) = (ring[index], ring[(index + 1) % n]);
                let pieces = wall_pieces(start, end);
                let outward = outward(start, end, turn);
                let facing = toward.lat_per_m * outward.1 * METRES_PER_DEGREE
                    + toward.lon_per_m * outward.0 * METRES_PER_DEGREE * start.0.to_radians().cos()
                    > 0.0;
                for piece in 0..pieces {
                    let lit = match (facing, whole) {
                        // Turned away: no direct light to split, as in the open.
                        (false, _) => 0.0,
                        (true, Some(whole)) => whole,
                        (true, None) => {
                            let share = (piece as f64 + 0.5) / pieces as f64;
                            let lat = start.0 + (end.0 - start.0) * share;
                            let lon = start.1 + (end.1 - start.1) * share;
                            let (lat, lon) = (
                                lat + outward.1 * WALL_OUT_M / METRES_PER_DEGREE,
                                lon + outward.0 * WALL_OUT_M
                                    / (METRES_PER_DEGREE * lat.to_radians().cos()),
                            );
                            self.lit_from(lat, lon, &toward, STEP_M, top_m)
                        }
                    };
                    walls.push(lit);
                }
            }
        }
        BlockLight { walls, roof }
    }

    /// The line toward the sun from latitude `lat` where `up` is the vertical.
    fn toward(&self, lat: f64, up: DVec3) -> Toward {
        let east = DVec3::Z.cross(up).normalize_or(DVec3::X);
        let north = up.cross(east);
        let (toward_east, toward_north) = (self.sun.dot(east), self.sun.dot(north));
        let across = toward_east.hypot(toward_north).max(1e-9);
        Toward {
            lat_per_m: toward_north / across / METRES_PER_DEGREE,
            lon_per_m: toward_east / across / (METRES_PER_DEGREE * lat.to_radians().cos()),
            rise: (self.sun.dot(up) / across).max(MIN_RISE),
        }
    }

    /// The lowest height (metres above the ground) the sun reaches at (`lat`, `lon`),
    /// marching from `start_m` away; it stops once that height passes `ceiling_m` (the
    /// spot is in the shade throughout) or the tiles held end.
    fn lit_from(&self, lat: f64, lon: f64, toward: &Toward, start_m: f64, ceiling_m: f64) -> f64 {
        let reach = (self.tallest_m / toward.rise).min(REACH_M);
        let mut lit = 0.0f64;
        let mut heights: Option<&Heights> = None;
        let mut distance = start_m;
        while distance <= reach && self.tallest_m - distance * toward.rise > lit && lit < ceiling_m
        {
            let (lat, lon) = (
                lat + toward.lat_per_m * distance,
                lon + toward.lon_per_m * distance,
            );
            let height = heights.and_then(|grid| grid.at(lat, lon)).or_else(|| {
                heights = self
                    .tiles
                    .iter()
                    .map(|tile| &tile.heights)
                    .find(|grid| grid.at(lat, lon).is_some());
                heights.and_then(|grid| grid.at(lat, lon))
            });
            // Past the tiles held nothing more is known.
            let Some(height) = height else {
                break;
            };
            lit = lit.max(f64::from(height) - distance * toward.rise);
            distance += STEP_M + distance * STEP_GROWTH;
        }
        lit
    }
}

/// How many pieces a wall from `start` to `end` (latitude, longitude) is drawn in.
pub(super) fn wall_pieces(start: (f64, f64), end: (f64, f64)) -> usize {
    let north = (end.0 - start.0) * METRES_PER_DEGREE;
    let east = (end.1 - start.1) * METRES_PER_DEGREE * start.0.to_radians().cos();
    (north.hypot(east) / WALL_PIECE_M).ceil().max(1.0) as usize
}

/// Which side of a ring's edges is outside: 1 for the right-hand side, −1 for the left
/// (outlines run anticlockwise, courtyards clockwise, seen from above).
pub(super) fn wall_turn(ring: &[(f64, f64)], outline: bool) -> f64 {
    if super::buildings::anticlockwise(ring) == outline {
        1.0
    } else {
        -1.0
    }
}

/// A wall's outward direction on the ground (east, north), unit.
fn outward(start: (f64, f64), end: (f64, f64), turn: f64) -> (f64, f64) {
    let east = (end.1 - start.1) * start.0.to_radians().cos();
    let north = end.0 - start.0;
    let length = east.hypot(north).max(f64::MIN_POSITIVE);
    (north / length * turn, -east / length * turn)
}

/// How far a block's footprint reaches from its middle (metres, about).
fn block_reach_m(building: &Building, lat: f64) -> f64 {
    let Some(outline) = building.rings.first() else {
        return 0.0;
    };
    let (mut south, mut north, mut west, mut east) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(lat, lon) in outline {
        (south, north) = (south.min(lat), north.max(lat));
        (west, east) = (west.min(lon), east.max(lon));
    }
    0.5 * ((north - south) * METRES_PER_DEGREE)
        .hypot((east - west) * METRES_PER_DEGREE * lat.to_radians().cos())
}

/// A block's key among the kept results.
fn block_id(building: &Building, far: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for ring in &building.rings {
        for &(lat, lon) in ring.iter().take(3) {
            lat.to_bits().hash(&mut hasher);
            lon.to_bits().hash(&mut hasher);
        }
        ring.len().hash(&mut hasher);
    }
    building.top_m.to_bits().hash(&mut hasher);
    building.base_m.to_bits().hash(&mut hasher);
    far.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            shaped: None,
            open_roof: 2,
        }
    }

    /// A tower east of a low block shades the low block's east wall up to a height
    /// that drops as the sun climbs (the light wanders down the wall), and its roof
    /// with the sun low; with the sun in the west nothing does.
    #[test]
    fn a_tower_toward_the_sun_shades_up_to_a_height_that_drops_as_it_climbs() {
        let (lat, lon) = (40.75, -73.99);
        // A 30 m block, and a 60 m tower about 50 m east of it (27 m from its east wall).
        let low = block(lat, lon, 0.000_09, 30.0);
        let tower = block(lat, lon + 0.000_6, 0.000_12, 60.0);
        let buildings = vec![low.clone(), tower];
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
        let light = |sun, far| SunMarch::new(&tiles, sun).march_block(&low, (lat, lon), up, far);
        // The square's second edge (south-east to north-east) is its east wall.
        let east_wall = |light: &BlockLight| light.walls[light.walls.len() / 4];
        let low_sun = light(sun_at(east, 5.0), false);
        assert!(east_wall(&low_sun) >= 30.0, "{low_sun:?}");
        assert!(!low_sun.roof);
        let higher = light(sun_at(east, 60.0), false);
        assert!(
            east_wall(&higher) < 30.0 && east_wall(&higher) > 0.0,
            "{higher:?}"
        );
        assert!(higher.roof);
        let west = light(sun_at(-east, 5.0), false);
        assert!(west.roof && west.walls.iter().all(|&lit| lit == 0.0));
        // Marched once from afar, the low sun still finds the tower.
        assert!(!light(sun_at(east, 5.0), true).roof);
    }
}
