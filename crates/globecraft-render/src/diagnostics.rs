//! The streaming diagnostics overlay's geometry (roadmap 1.11): which imagery tiles a
//! view wants, found the way the renderer chooses them, and their outlines on screen.
//! The app colours them by state and draws them.
use super::{RayCaster, RenderCfg, natural_level, pixel::surface};
use emath::{Pos2, Rect};
use globecraft_core::camera::Basis;
use globecraft_data::tiles::{TileKey, make_key};
use std::collections::BTreeSet;

/// Screen samples per side when collecting the view's tiles.
const SAMPLES_PER_SIDE: usize = 48;
/// Points per tile edge: edges are curves on the globe.
const EDGE_POINTS: usize = 8;
/// At most this many tiles are outlined, the nearest levels first.
const MAX_TILES: usize = 400;

/// A tile the view wants and its outline as screen polylines (split where an edge
/// passes behind the horizon).
pub struct TileOutline {
    pub key: TileKey,
    pub z: u8,
    pub x: u32,
    pub y: u32,
    pub outline: Vec<Vec<Pos2>>,
    /// Where its label goes, when its centre is on screen.
    pub centre: Option<Pos2>,
}

/// The imagery tiles a `w`×`h` render of `basis` with `cfg` wants, each at the level a
/// pixel over it would use (a tilted view's far field is coarser), with outlines in
/// `rect`.
pub fn view_tiles(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    rect: Rect,
) -> Vec<TileOutline> {
    let rays = RayCaster::new(w, h, basis);
    let geometry = rays.model_geometry();
    let step = (w.max(h) / SAMPLES_PER_SIDE).max(1);
    let max_zoom = i32::from(cfg.provider.max_zoom());
    let mut tiles = BTreeSet::new();
    for y in (0..h).step_by(step) {
        for x in (0..w).step_by(step) {
            let ray = if rays.uses_unit_sphere() {
                rays.cast_model::<false>(x, y)[0]
            } else {
                rays.cast_model::<true>(x, y)[0]
            };
            if ray.discriminant < 0.0 || ray.camera_dot >= 0.0 {
                continue;
            }
            let surface = if rays.uses_unit_sphere() {
                surface::<false>(x, y, w, h, &ray, cfg, geometry)
            } else {
                surface::<true>(x, y, w, h, &ray, cfg, geometry)
            };
            let z = natural_level(&surface).clamp(0, max_zoom);
            let n = (1u64 << z) as f64;
            let tx = (surface.u * n).floor().min(n - 1.0) as u32;
            let ty = (surface.v * n).floor().min(n - 1.0) as u32;
            tiles.insert((std::cmp::Reverse(z as u8), tx, ty));
        }
    }
    tiles
        .into_iter()
        .take(MAX_TILES)
        .map(|(std::cmp::Reverse(z), x, y)| {
            let at = |fx: f64, fy: f64| {
                let n = f64::from(1u32 << z);
                let lon = (f64::from(x) + fx) / n * 360.0 - 180.0;
                let lat = (std::f64::consts::PI * (1.0 - 2.0 * (f64::from(y) + fy) / n))
                    .sinh()
                    .atan()
                    .to_degrees();
                basis.project(geometry.position(lat, lon), rect)
            };
            let mut outline = Vec::new();
            let mut run = Vec::new();
            // Round the tile: top, right, bottom, left edges.
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)];
            for edge in corners.windows(2) {
                let ((x0, y0), (x1, y1)) = (edge[0], edge[1]);
                for i in 0..=EDGE_POINTS {
                    let t = i as f64 / EDGE_POINTS as f64;
                    match at(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t) {
                        Some(p) => run.push(p),
                        None if run.len() > 1 => outline.push(std::mem::take(&mut run)),
                        None => run.clear(),
                    }
                }
            }
            if run.len() > 1 {
                outline.push(run);
            }
            TileOutline {
                key: make_key(cfg.provider, z, x, y, cfg.rel),
                z,
                x,
                y,
                outline,
                centre: at(0.5, 0.5),
            }
        })
        .collect()
}
