//! Roofs drawn as imagery tiles (D3). The frame on screen showed roofs as the ground
//! looks from the camera: stretched at the frame's edges and blurred in steep views
//! (Julien's recording, 2026-11-27). Each photographed roof is now laid over a block
//! of 2×2 imagery tiles at the finest zoom its distance on screen asks for, up to the
//! provider's finest (taken from nearly straight above), so a roof keeps its detail
//! however the view leans. A tile not loaded yet is stood in for by its finest loaded
//! ancestor, and the nearest missing ones are asked for.
use globecraft_data::tiles::{Lookup, Provider, Tile, TileCache, TileKey, make_key};
use std::collections::HashMap;
use std::sync::Arc;

/// A corner's `roof` where it takes its colour alone: walls, shadows, and roofs
/// without imagery.
pub const NO_ROOF: u32 = u32::MAX;
/// `BuildingVertex::roof_at` across a whole block of two tiles.
pub const ROOF_AT_SCALE: f32 = u16::MAX as f32;
/// The tiles roofs are drawn from are this many pixels square.
pub const ROOF_TILE_PX: usize = 256;
/// How far up from a missing tile its stand-in is looked for (levels).
const MAX_COARSER_LEVELS: u8 = 8;
/// Tiles a roof's corners are asked for per mesh, nearest first: in background slots,
/// so the view's own tiles keep a quarter of the downloads.
const MAX_REQUESTS_PER_MESH: usize = 24;
/// Texels per pixel of the roof on screen: a little finer than the screen, so a roof
/// stays sharp while the camera closes in before the next level arrives.
const TEXELS_PER_PIXEL: f64 = 1.5;
const EARTH_CIRCUMFERENCE_M: f64 = std::f64::consts::TAU * globecraft_core::EARTH_RADIUS_M;

/// One of a block's tiles: the tile drawn, its own or an ancestor `coarser` levels
/// up (drawn over the part of it the block's tile covers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoofCell {
    pub tile: TileKey,
    pub coarser: u8,
}

/// 2×2 tiles a roof is laid over, from tile `(x, y)` at their zoom; cells in the
/// order (x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1), none where no roof reaches
/// or nothing is loaded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoofBlock {
    pub x: u32,
    pub y: u32,
    pub cells: [Option<RoofCell>; 4],
}

/// What a mesh's roofs are drawn from: the blocks (`BuildingVertex::roof` counts
/// them) and every tile they draw, held while the mesh is shown.
#[derive(Clone, Default)]
pub struct RoofImagery {
    pub blocks: Vec<RoofBlock>,
    pub tiles: Vec<(TileKey, Arc<Tile>)>,
}

impl std::fmt::Debug for RoofImagery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoofImagery")
            .field("blocks", &self.blocks)
            .field(
                "tiles",
                &self.tiles.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Where a roof lies in its block: tiles from the block's first one.
#[derive(Clone, Copy, Debug)]
pub(super) struct RoofPlace {
    pub(super) block: u32,
    scale: f64,
    origin: (f64, f64),
}

impl RoofPlace {
    /// `(lat, lon)` in tiles from the block's corner, 0 to 2 across it.
    pub(super) fn at(&self, lat: f64, lon: f64) -> [f64; 2] {
        let (u, v) = mercator(lat, lon);
        [
            u * self.scale - self.origin.0,
            v * self.scale - self.origin.1,
        ]
    }
}

/// One projection job's blocks and tiles, kept from block to block.
#[derive(Default)]
pub(super) struct RoofRun {
    blocks: Vec<RoofBlock>,
    block_index: HashMap<(u8, u32, u32), u32>,
    cells: HashMap<(u8, u32, u32), Option<RoofCell>>,
    tiles: HashMap<TileKey, Arc<Tile>>,
    /// Tiles a roof wanted that are not loaded, with the roof's distance (metres).
    wanted: Vec<(f64, TileKey)>,
}

/// Which imagery the roofs are drawn from, and how fine a roof needs it.
#[derive(Clone, Copy)]
pub(super) struct RoofSource<'a> {
    /// A loaded tile (`TileCache::ready_tile`).
    pub(super) ready: &'a (dyn Fn(TileKey) -> Option<Arc<Tile>> + Sync),
    pub(super) provider: Provider,
    pub(super) release: u16,
    /// The ground one pixel covers per metre of distance.
    pub(super) pixel_per_m: f64,
}

impl RoofRun {
    /// The block a roof over `outline` lies in, `distance_m` from the camera.
    pub(super) fn place(
        &mut self,
        source: RoofSource<'_>,
        outline: &[(f64, f64)],
        distance_m: f64,
    ) -> Option<RoofPlace> {
        let (mut low, mut high) = ((f64::INFINITY, f64::INFINITY), (0.0f64, 0.0f64));
        let mut lat_sum = 0.0;
        for &(lat, lon) in outline {
            let (u, v) = mercator(lat, lon);
            (low, high) = ((low.0.min(u), low.1.min(v)), (high.0.max(u), high.1.max(v)));
            lat_sum += lat;
        }
        let lat = lat_sum / outline.len().max(1) as f64;
        // Fine enough for its pixels on screen, coarse enough that the roof fits a
        // block (its width at most a tile's).
        let pixel_m = (distance_m * source.pixel_per_m).max(f64::MIN_POSITIVE);
        let detail = (EARTH_CIRCUMFERENCE_M * lat.to_radians().cos() * TEXELS_PER_PIXEL
            / (ROOF_TILE_PX as f64 * pixel_m))
            .log2()
            .ceil();
        let span = (high.0 - low.0).max(high.1 - low.1).max(f64::MIN_POSITIVE);
        let fit = (-span.log2()).floor();
        let zoom = detail
            .min(fit)
            .clamp(0.0, f64::from(source.provider.max_zoom())) as u8;
        let scale = f64::from(1u32 << zoom);
        let last = scale - 1.0;
        let (x, y) = (
            (low.0 * scale).floor().min(last),
            (low.1 * scale).floor().min(last),
        );
        let (far_x, far_y) = (
            (high.0 * scale).floor().min(last),
            (high.1 * scale).floor().min(last),
        );
        let (x, y) = (x as u32, y as u32);
        let block = match self.block_index.get(&(zoom, x, y)) {
            Some(&index) => index,
            None => {
                let cells = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| {
                    // Only the tiles the roof reaches.
                    (f64::from(x + dx) <= far_x && f64::from(y + dy) <= far_y)
                        .then(|| self.cell(source, zoom, x + dx, y + dy, distance_m))
                        .flatten()
                });
                if cells.iter().all(Option::is_none) {
                    return None;
                }
                let index = self.blocks.len() as u32;
                self.blocks.push(RoofBlock { x, y, cells });
                self.block_index.insert((zoom, x, y), index);
                index
            }
        };
        Some(RoofPlace {
            block,
            scale,
            origin: (f64::from(x), f64::from(y)),
        })
    }

    /// The tile drawn for tile `(x, y)` at `zoom`: its own when loaded, else its
    /// finest loaded ancestor (and the tile itself is wanted).
    fn cell(
        &mut self,
        source: RoofSource<'_>,
        zoom: u8,
        x: u32,
        y: u32,
        distance_m: f64,
    ) -> Option<RoofCell> {
        if let Some(&cell) = self.cells.get(&(zoom, x, y)) {
            return cell;
        }
        let key = |coarser: u8| {
            make_key(
                source.provider,
                zoom - coarser,
                x >> coarser,
                y >> coarser,
                source.release,
            )
        };
        let found = (0..=MAX_COARSER_LEVELS.min(zoom)).find_map(|coarser| {
            let tile = (source.ready)(key(coarser))?;
            let (width, height, _) = tile.rgba();
            (width == ROOF_TILE_PX && height == ROOF_TILE_PX).then_some((coarser, tile))
        });
        if found.as_ref().is_none_or(|&(coarser, _)| coarser > 0) {
            self.wanted.push((distance_m, key(0)));
        }
        let cell = found.map(|(coarser, tile)| {
            let tile_key = key(coarser);
            self.tiles.entry(tile_key).or_insert(tile);
            RoofCell {
                tile: tile_key,
                coarser,
            }
        });
        self.cells.insert((zoom, x, y), cell);
        cell
    }

    pub(super) fn block_count(&self) -> u32 {
        self.blocks.len() as u32
    }
}

/// The runs' blocks one after another (each run's `BuildingVertex::roof` counts
/// from its first), every tile once; the nearest missing tiles are asked for.
pub(super) fn joined(runs: &[&RoofRun], cache: Option<&TileCache>) -> RoofImagery {
    let mut imagery = RoofImagery::default();
    let mut tiles: HashMap<TileKey, Arc<Tile>> = HashMap::new();
    let mut wanted: Vec<(f64, TileKey)> = Vec::new();
    for run in runs {
        imagery.blocks.extend_from_slice(&run.blocks);
        for (&key, tile) in &run.tiles {
            tiles.entry(key).or_insert_with(|| tile.clone());
        }
        wanted.extend_from_slice(&run.wanted);
    }
    imagery.tiles = tiles.into_iter().collect();
    if let Some(cache) = cache {
        wanted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut asked = std::collections::HashSet::new();
        for (_, key) in wanted {
            if asked.len() >= MAX_REQUESTS_PER_MESH {
                break;
            }
            // A tile the source does not have is not asked for again.
            if asked.insert(key) && matches!(cache.lookup(key), Lookup::Absent) {
                cache.request_in_background(key);
            }
        }
    }
    imagery
}

/// Web Mercator position, 0 to 1 east and south.
fn mercator(lat: f64, lon: f64) -> (f64, f64) {
    let sine = lat.clamp(-85.05, 85.05).to_radians().sin();
    (
        ((lon + 180.0) / 360.0).rem_euclid(1.0),
        0.5 - ((1.0 + sine) / (1.0 - sine)).ln() / (4.0 * std::f64::consts::PI),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 256-pixel imagery tile of one colour.
    fn tile() -> Arc<Tile> {
        let picture = image::RgbaImage::from_pixel(256, 256, image::Rgba([90, 120, 150, 255]));
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Arc::new(Tile::decode(&png).unwrap())
    }

    /// A house 12 m square (one ring) at (lat, lon).
    fn house(lat: f64, lon: f64) -> Vec<(f64, f64)> {
        let (dlat, dlon) = (6.0 / 111_195.0, 6.0 / (111_195.0 * lat.to_radians().cos()));
        vec![
            (lat - dlat, lon - dlon),
            (lat - dlat, lon + dlon),
            (lat + dlat, lon + dlon),
            (lat + dlat, lon - dlon),
        ]
    }

    fn source<'a>(ready: &'a (dyn Fn(TileKey) -> Option<Arc<Tile>> + Sync)) -> RoofSource<'a> {
        RoofSource {
            ready,
            provider: Provider::Satellite,
            release: 0,
            // A 1,000-pixel-high view.
            pixel_per_m: 2.0 * globecraft_core::VERTICAL_FOV_HALF_TAN / 1000.0,
        }
    }

    #[test]
    fn a_near_roof_takes_the_finest_tiles_and_a_far_one_coarser() {
        let (lat, lon) = (50.976, 11.023);
        let all = |_: TileKey| Some(tile());
        let mut run = RoofRun::default();
        let near = run.place(source(&all), &house(lat, lon), 100.0).unwrap();
        // Close by, the provider's finest level (20): a 12 m house fits a block of
        // two 24 m tiles.
        assert_eq!(near.scale, f64::from(1u32 << 20));
        for &(lat, lon) in &house(lat, lon) {
            let at = near.at(lat, lon);
            assert!(at.iter().all(|value| (0.0..=2.0).contains(value)), "{at:?}");
        }
        let far = run.place(source(&all), &house(lat, lon), 5_000.0).unwrap();
        assert!(far.scale < near.scale);
        // Every cell found its own tile.
        let block = run.blocks[near.block as usize];
        assert!(
            block
                .cells
                .iter()
                .flatten()
                .all(|cell| cell.coarser == 0 && globecraft_data::tiles::unkey(cell.tile).1 == 20)
        );
        assert!(run.wanted.is_empty());
    }

    #[test]
    fn a_missing_tile_is_drawn_from_its_ancestor_and_wanted() {
        let (lat, lon) = (50.976, 11.023);
        // Only zoom 17 is loaded.
        let coarse = |key: TileKey| (globecraft_data::tiles::unkey(key).1 == 17).then(tile);
        let mut run = RoofRun::default();
        let place = run.place(source(&coarse), &house(lat, lon), 100.0).unwrap();
        let block = run.blocks[place.block as usize];
        let cells: Vec<RoofCell> = block.cells.iter().flatten().copied().collect();
        assert!(!cells.is_empty());
        for cell in &cells {
            let (_, zoom, ..) = globecraft_data::tiles::unkey(cell.tile);
            assert_eq!((zoom, cell.coarser), (17, 3));
        }
        // The level the roof wanted is asked for, each tile once per job.
        assert_eq!(run.wanted.len(), cells.len());
        assert!(
            run.wanted
                .iter()
                .all(|&(_, key)| globecraft_data::tiles::unkey(key).1 == 20)
        );
        // Nothing loaded at all: no block, the roof keeps its colour.
        let none = |_: TileKey| None;
        assert!(
            RoofRun::default()
                .place(source(&none), &house(lat, lon), 100.0)
                .is_none()
        );
    }

    #[test]
    fn joined_runs_count_their_blocks_on_and_share_tiles() {
        let all = |_: TileKey| Some(tile());
        let (mut first, mut second) = (RoofRun::default(), RoofRun::default());
        first
            .place(source(&all), &house(50.976, 11.023), 100.0)
            .unwrap();
        let place = second
            .place(source(&all), &house(50.976, 11.023), 100.0)
            .unwrap();
        assert_eq!(place.block, 0);
        let joined = joined(&[&first, &second], None);
        // The second run's block follows the first's; the same tiles are held once.
        assert_eq!(joined.blocks.len(), 2);
        assert_eq!(joined.blocks[0], joined.blocks[1]);
        assert_eq!(joined.tiles.len(), first.tiles.len());
    }
}
