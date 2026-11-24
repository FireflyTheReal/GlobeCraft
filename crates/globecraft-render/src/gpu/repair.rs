//! Precise shared CPU sky, horizon and tile-seam repair after GPU readback.
use crate::{tuning, worker::RenderRequest};
use globecraft_data::tiles::{Lookup, TileKey};
use rayon::prelude::*;
use std::collections::HashMap;

/// Draws the pixels the shader left (alpha below 255) as the CPU does; with the
/// relief, from the GPU's walk (`walk`, `relief::PLANES` planes): marched where the
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
    let batches: Vec<_> = pixels
        .par_chunks_mut(request.width * 4 * tuning::ROWS_PER_TILE_BATCH)
        .enumerate()
        .map(|(batch, rows)| {
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
                // The relief: every pixel left as `render_row` draws it.
                if let (Some(walk), Some(_)) = (walk, request.cfg.terrain) {
                    let starts = crate::MarchStarts::walked(walk, request.width, request.height, y);
                    let mut sun_seen = crate::SunSeen::default();
                    for (pack, group) in pixels.chunks_mut(crate::rays::RAY_LANES).enumerate() {
                        if group.iter().all(|pixel| pixel[3] == 255) {
                            continue;
                        }
                        let first_x = pack * crate::rays::RAY_LANES;
                        let samples = if rays.uses_unit_sphere() {
                            rays.cast_model::<false>(first_x, y)
                        } else {
                            rays.cast_model::<true>(first_x, y)
                        };
                        for (lane, (pixel, ray)) in group.iter_mut().zip(samples).enumerate() {
                            if pixel[3] == 255 {
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
                    continue;
                }
                for (pack, group) in pixels.chunks_mut(crate::rays::RAY_LANES).enumerate() {
                    // Most surface packs need no CPU work. Sky/horizon packs retain
                    // f64 classification and the deterministic 64-bit stars.
                    if group.iter().all(|pixel| pixel[3] == 255) {
                        continue;
                    }
                    for (lane, (pixel, ray)) in group
                        .iter_mut()
                        .zip(rays.cast(pack * crate::rays::RAY_LANES, y))
                        .enumerate()
                    {
                        if pixel[3] == 255 {
                            continue;
                        }
                        let color = if ray.discriminant >= 0.0 && ray.camera_dot < 0.0 {
                            let (x, geometry) =
                                (pack * crate::rays::RAY_LANES + lane, rays.model_geometry());
                            let (w, h) = (request.width, request.height);
                            let surface = if rays.uses_unit_sphere() {
                                crate::surface::<false>(x, y, w, h, &ray, &request.cfg, geometry)
                            } else {
                                crate::surface::<true>(x, y, w, h, &ray, &request.cfg, geometry)
                            };
                            crate::shade_surface(
                                crate::with_air(surface, &ray, air.as_ref()),
                                &request.cfg,
                                request.cfg.rel,
                                &mut local,
                                crate::Relief::FLAT,
                            )
                            .with_w(1.0)
                        } else {
                            crate::sky(&ray, &rays, air.as_ref(), &request.cfg)
                        };
                        let color = color.clamp(glam::Vec4::ZERO, glam::Vec4::ONE) * 255.0;
                        *pixel = [color.x as u8, color.y as u8, color.z as u8, color.w as u8];
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
