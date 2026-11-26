//! Precise shared CPU sky, horizon and tile-seam repair after GPU readback.
use crate::{tuning, worker::RenderRequest};
use globecraft_data::tiles::{Lookup, TileKey};
use rayon::prelude::*;
use std::collections::HashMap;

/// Draws the pixels the shader left (`overview::UNDRAWN`) as the CPU does; with the
/// relief, from the GPU's walk (`walk`, `relief::READ_PLANES` planes): marched where the
/// shader did not take the walk's ground. The tiles missed, and the elevation tiles
/// held.
pub(super) fn pixels(
    request: &RenderRequest,
    pixels: &mut [u8],
    lookup: &(impl Fn(TileKey) -> Lookup + Sync),
    walk: Option<&[f32]>,
) -> (Vec<(TileKey, Option<f32>)>, Vec<TileKey>) {
    let rays = crate::rays::RayCaster::new(request.width, request.height, &request.basis);
    let air = crate::frame_air(&rays, &request.cfg);
    let now = std::time::Instant::now();
    let cancel = crate::cancel::flag();
    let batches: Vec<_> = pixels
        .par_chunks_mut(request.width * 4 * tuning::ROWS_PER_TILE_BATCH)
        .enumerate()
        .map(|(batch, rows)| {
            if crate::cancel::set(&cancel) {
                return (Vec::new(), Vec::new());
            }
            let mut local = crate::Local {
                lookup,
                map: HashMap::default(),
                dem: Vec::new(),
                dem_placed: Vec::new(),
                dem_index: HashMap::default(),
                now,
            };
            for (offset, row) in rows.chunks_mut(request.width * 4).enumerate() {
                let y = batch * tuning::ROWS_PER_TILE_BATCH + offset;
                let pixels: &mut [[u8; 4]] = bytemuck::cast_slice_mut(row);
                // Every pixel left as `render_row` draws it; with the relief, marched
                // from the walk's starts.
                let starts = match (walk, request.cfg.terrain) {
                    (Some(walk), Some(_)) => {
                        crate::MarchStarts::walked(walk, request.width, request.height, y)
                    }
                    _ => crate::MarchStarts::default(),
                };
                let mut sun_seen = crate::SunSeen::default();
                for (pack, group) in pixels.chunks_mut(crate::rays::RAY_LANES).enumerate() {
                    // Most packs need no CPU work. Sky/horizon packs retain f64
                    // classification.
                    if group.iter().all(|pixel| *pixel != super::overview::UNDRAWN) {
                        continue;
                    }
                    let first_x = pack * crate::rays::RAY_LANES;
                    for (lane, (pixel, ray)) in
                        group.iter_mut().zip(rays.cast(first_x, y)).enumerate()
                    {
                        if *pixel != super::overview::UNDRAWN {
                            continue;
                        }
                        let x = first_x + lane;
                        *pixel = if rays.uses_unit_sphere() {
                            crate::pixel_colour::<false>(
                                x,
                                y,
                                &ray,
                                &rays,
                                &request.cfg,
                                &mut local,
                                &starts,
                                air.as_ref(),
                                &mut sun_seen,
                            )
                        } else {
                            crate::pixel_colour::<true>(
                                x,
                                y,
                                &ray,
                                &rays,
                                &request.cfg,
                                &mut local,
                                &starts,
                                air.as_ref(),
                                &mut sun_seen,
                            )
                        };
                    }
                }
            }
            let held: Vec<TileKey> = local.dem_index.keys().copied().collect();
            let missed = local
                .map
                .into_iter()
                .filter_map(|(key, tile)| match tile {
                    crate::LocalTile::Absent(distance) => Some((key, Some(distance))),
                    crate::LocalTile::Pending => Some((key, None)),
                    crate::LocalTile::Ready(_) | crate::LocalTile::Unavailable => None,
                })
                .collect::<Vec<_>>();
            (missed, held)
        })
        .collect();
    let mut held = std::collections::HashSet::new();
    let mut missed = Vec::new();
    for (batch_missed, batch_held) in batches {
        missed.extend(batch_missed);
        held.extend(batch_held);
    }
    (missed, held.into_iter().collect())
}
