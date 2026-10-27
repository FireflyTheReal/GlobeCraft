//! Beam pre-pass: one cone per block of pixels walks the open air before the pixels'
//! own rays do (roadmap 8.1 speed-up, 2026-10-27).
//!
//! Most of a ray's steps are spent descending the block-maxima pyramid toward the
//! ground, and neighbouring pixels repeat nearly the same descent. A cone that holds
//! every ray of an 8×8 pixel block takes those steps once: it may skip a block only
//! when its whole footprint lies inside the block and its lowest point is above the
//! block's maximum, so no ray in it can meet the ground there. Each pixel then starts
//! its own march where the cone stopped, a few texels above the ground.
//!
//! The cone picks its elevation level with the same rule as the pixels' rays (from the
//! distance), so the relief it clears is the relief they would have marched through.
use super::{Anchor, Local, Lookup, TileKey};
use super::{
    BLOCK_EXIT_TEXELS, DemTile, MAX_MARCH_STEPS, MAX_NUDGE, MAX_TERRAIN_M, TerrainCfg, dem_tile,
    height_above, shell_span, zoom_band,
};
use glam::DVec3;
use globecraft_core::{EARTH_RADIUS_M, earth_model::SurfaceGeometry};
use std::f64::consts::PI;

/// Pixels per side of the block one cone covers.
pub(in crate::terrain) const BEAM_PX: usize = 8;
/// Footprint margin in texels, for the linear Mercator steps between exact
/// measurements (of the cone and of the pixels' rays) and the texel rounding.
const FOOTPRINT_MARGIN_TEXELS: f64 = 1.0;
/// A cone step may grow the distance by at most this fraction (of at least
/// `MIN_STEP_BASE_M`), so the footprint at the step's end, which the tests use, stays
/// close to the one at its start.
const MAX_STEP_GROWTH: f64 = 0.5;
/// A cone stops once its step is shorter than this part of its footprint radius
/// (0.03–0.25 measured the same; 1 left a fifth more steps to the pixels).
const MIN_PROGRESS: f64 = 0.25;
/// The step limit grows from this distance when the camera is among the summits
/// (metres along the ray).
const MIN_STEP_BASE_M: f64 = 20.0;
/// Keeps the cone's half angle above zero for a block of one pixel.
const MIN_SPREAD_RAD: f64 = 1e-9;

/// The cone of a pixel block: its axis and the largest angle (radians) between the axis
/// and any ray of the block.
#[derive(Clone, Copy)]
pub(in crate::terrain) struct Cone {
    pub(in crate::terrain) axis: DVec3,
    pub(in crate::terrain) spread: f64,
}

impl Cone {
    /// The cone around unit ray directions (the block's corner rays: every ray of the
    /// block lies between them).
    pub(in crate::terrain) fn around(corners: [DVec3; 4]) -> Self {
        let axis = (corners[0] + corners[1] + corners[2] + corners[3]).normalize();
        let spread = corners
            .iter()
            .map(|corner| corner.dot(axis).clamp(-1.0, 1.0).acos())
            .fold(MIN_SPREAD_RAD, f64::max);
        Self { axis, spread }
    }
}

/// How far along their rays (in the rays' own distance) every ray of `cone` stays above
/// the relief: the pixels' marches may start there. Zero when nothing can be skipped.
#[allow(clippy::too_many_arguments)]
pub(in crate::terrain) fn clear_distance(
    cone: Cone,
    origin: DVec3,
    geometry: SurfaceGeometry,
    terrain: TerrainCfg,
    pixel_angle: f64,
    dist: f32,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
) -> f64 {
    let Cone { axis, spread } = cone;
    let to_world = terrain.exaggeration / EARTH_RADIUS_M;
    let ceiling = MAX_TERRAIN_M * to_world;
    // Start where the axis is so high that no ray of the cone can be below the highest
    // summit yet: above the ceiling by more than the footprint there.
    let Some((enter, leave)) = shell_span(geometry, origin, axis, ceiling) else {
        return 0.0;
    };
    let Some((start, _)) = shell_span(geometry, origin, axis, ceiling + 2.0 * enter * spread)
    else {
        return 0.0;
    };
    let q = geometry.inverse_axes_squared();
    let mut t = start;
    let mut anchor = Anchor::at(geometry, origin, axis, t);
    let (mut zoom, mut zoom_until) = (0, f64::NEG_INFINITY);
    let mut current: Option<DemTile> = None;
    let mut looked_up: Option<i32> = None;
    let mut start_level = usize::MAX;
    for _ in 0..MAX_MARCH_STEPS {
        if t >= leave {
            break;
        }
        let height = height_above(origin + axis * t, q);
        // The footprint radius at the end of the longest step allowed from here.
        let longest = MAX_STEP_GROWTH * t.max(MIN_STEP_BASE_M / EARTH_RADIUS_M);
        let reach = (t + longest) * spread;
        if height - reach <= 0.0 {
            break;
        }
        if t > zoom_until {
            (zoom, zoom_until) = zoom_band(t, pixel_angle, anchor.latitude_cosine);
        }
        let mut uv = anchor.uv(t);
        let reuse = looked_up == Some(zoom)
            && current
                .as_ref()
                .is_some_and(|tile| tile.contains(tile.texel(uv)));
        if !reuse {
            current = dem_tile(loc, zoom, uv, dist);
            looked_up = current.is_some().then_some(zoom);
            start_level = usize::MAX;
        }
        // No relief loaded here: the pixels go on alone.
        let Some(tile) = current else {
            break;
        };
        let (vx, vy) = (
            anchor.duv.0 * tile.n * tile.size,
            anchor.duv.1 * tile.n * tile.size,
        );
        let speed = (vx * vx + vy * vy).sqrt().max(1e-12);
        if (t - anchor.t) * speed > super::REANCHOR_TEXELS {
            anchor = Anchor::at(geometry, origin, axis, t);
            uv = anchor.uv;
        }
        // Texel coordinates across the whole level, so the footprint may reach into
        // the neighbouring tiles.
        let (x, y) = tile.texel(uv);
        let (x, y) = (x + tile.tx * tile.size, y + tile.ty * tile.size);
        // One texel on the ground, in world units (Mercator: the same both ways).
        let texel_world = 2.0 * PI * anchor.latitude_cosine / (tile.n * tile.size);
        let margin = reach / texel_world + FOOTPRINT_MARGIN_TEXELS;
        let side = tile.n * tile.size;
        let (low, high) = ((x - margin, y - margin), (x + margin, y + margin));
        if low.0 < 0.0 || low.1 < 0.0 || high.0 >= side || high.1 >= side || margin >= tile.size {
            break;
        }
        // The tiles under the footprint (at most two by two), all at this level.
        let tiles_x = ((low.0 / tile.size) as usize, (high.0 / tile.size) as usize);
        let tiles_y = ((low.1 / tile.size) as usize, (high.1 / tile.size) as usize);
        let mut under = [None; 4];
        let mut complete = true;
        for (i, slot) in under.iter_mut().enumerate() {
            let (tx, ty) = (tiles_x.0 + i % 2, tiles_y.0 + i / 2);
            if tx > tiles_x.1 || ty > tiles_y.1 {
                continue;
            }
            let found = if (tx as f64, ty as f64) == (tile.tx, tile.ty) {
                Some(tile)
            } else {
                let centre = ((tx as f64 + 0.5) / tile.n, (ty as f64 + 0.5) / tile.n);
                dem_tile(loc, tile.n.log2().round() as i32, centre, dist)
            };
            match found {
                Some(found) if found.n == tile.n && found.size == tile.size => *slot = Some(found),
                _ => complete = false,
            }
        }
        if !complete {
            break;
        }
        let block_max = |level: usize, bx: usize, by: usize| {
            let per_tile = (tile.size as usize) >> level;
            let (tx, ty) = (bx / per_tile, by / per_tile);
            let slot = (tx - tiles_x.0) + 2 * (ty - tiles_y.0);
            under[slot].map(|held| {
                held.elevation(&loc.dem)
                    .block_max_m(level, bx % per_tile, by % per_tile)
            })
        };
        // The coarsest blocks (at most two by two) that hold the whole footprint and
        // all lie below it.
        let mut clear = None;
        let top_level = tile.size.log2() as usize;
        for level in (0..=top_level.min(start_level)).rev() {
            let block = (1usize << level) as f64;
            let bx = ((low.0 / block) as usize, (high.0 / block) as usize);
            let by = ((low.1 / block) as usize, (high.1 / block) as usize);
            if bx.1 > bx.0 + 1 || by.1 > by.0 + 1 {
                break;
            }
            let mut top = f32::MIN;
            for yy in by.0..=by.1 {
                for xx in bx.0..=bx.1 {
                    top = top.max(block_max(level, xx, yy).unwrap_or(f32::MAX));
                }
            }
            let top = f64::from(top) * to_world;
            if height - reach > top {
                clear = Some((level, block, bx, by, top));
                break;
            }
        }
        let Some((level, block, bx, by, top)) = clear else {
            break;
        };
        // Until the footprint leaves those blocks, or the cone's lowest ray could come
        // down to their top (heights along a straight line rise faster than linearly
        // over the curved Earth, so the linear bound is safe).
        let region = (bx.0 as f64 * block, by.0 as f64 * block);
        let extent = (
            (bx.1 - bx.0 + 1) as f64 * block,
            (by.1 - by.0 + 1) as f64 * block,
        );
        let mut step = block_exit_rect(
            (x - margin - region.0, y - margin - region.1),
            (extent.0 - 2.0 * margin, extent.1 - 2.0 * margin),
            vx,
            vy,
        );
        let sink = anchor.climb - spread;
        if sink < 0.0 {
            step = step.min((height - reach - top) / -sink);
        }
        step = step.min(longest);
        if step <= 0.0 {
            break;
        }
        t += step + (BLOCK_EXIT_TEXELS / speed).min(MAX_NUDGE);
        // Closing in on the relief by a small part of the footprint: the pixels' finer
        // steps take over (the cone would only creep closer, step after shorter step).
        if step < reach * MIN_PROGRESS {
            break;
        }
        start_level = level + 1;
    }
    t
}

/// Distance along the ray to leave a rectangle `extent` texels wide, from `position`
/// inside it, at texel velocity `(vx, vy)` per unit of distance.
fn block_exit_rect(position: (f64, f64), extent: (f64, f64), vx: f64, vy: f64) -> f64 {
    let axis = |position: f64, extent: f64, velocity: f64| {
        if velocity > 0.0 {
            (extent - position) / velocity
        } else if velocity < 0.0 {
            -position / velocity
        } else {
            f64::INFINITY
        }
    };
    axis(position.0, extent.0, vx).min(axis(position.1, extent.1, vy))
}
