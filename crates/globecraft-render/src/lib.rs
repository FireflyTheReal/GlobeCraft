//! Globe rasterization (CPU/GPU) and background frame coordination.
#![forbid(unsafe_code)]
use glam::{DVec3, Vec2, Vec4};
pub mod backend;
pub mod celestial;
#[cfg(feature = "gpu")]
mod gpu;
pub mod overlays;
mod pixel;
mod rays;
mod terrain;
use pixel::{Surface, sky, surface};
pub use terrain::{MAX_EXAGGERATION, MIN_EXAGGERATION, TERRAIN_MAX_ALTITUDE_KM, TerrainCfg};
mod tuning;
pub mod worker;
use globecraft_core::{EARTH_RADIUS_M, VERTICAL_FOV_HALF_TAN, camera::Basis, math::v3_to_ll};
use globecraft_core::{METERS_PER_KM, METERS_PER_LATITUDE_DEGREE};
use globecraft_data::tiles::{
    KeyBuild, Lookup, Provider, Tile, TileCache, TileKey, make_key, unkey,
};
use rayon::prelude::*;
use rays::{RAY_LANES, RayCaster};
use std::{collections::HashMap, f64::consts::PI, sync::Arc, sync::LazyLock};
use tuning::{
    ATMOSPHERE_BLEND, ATMOSPHERE_COLOR, ATMOSPHERE_HEIGHT_RATIO, ATMOSPHERE_RADIUS_RATIO,
    COLOR_CHANNEL_MAX, DAY_BRIGHTNESS_RANGE, GRID_BLEND, GRID_COLOR, GRID_LINE_WIDTH_PX,
    IMAGERY_ZOOM_FADE_START, LOD_SHARPNESS_BIAS, MIN_LOD_INCIDENCE_COSINE, MIN_LOD_LATITUDE_COSINE,
    MIN_ZOOM_BLEND_FRACTION, MISSING_TILE_COLOR, NIGHT_BLUE_TINT, NIGHT_BRIGHTNESS, OPAQUE_ALPHA,
    RGBA_CHANNELS, RIM_BLEND, RIM_COLOR, RIM_FALLOFF_POWER, ROWS_PER_TILE_BATCH, SPACE_COLOR,
    STAR_BRIGHTNESS_DIVISOR, STAR_BRIGHTNESS_MASK, STAR_BRIGHTNESS_SHIFT, STAR_GRID_RESOLUTION,
    STAR_MIN_BRIGHTNESS, STAR_OCCURRENCE_MODULUS, TEXT_ZOOM_FADE_START, TILE_EXP2_LUT_STEPS,
    TILE_FILTER_RATIO_EXONENT_MAX, TILE_FILTER_RATIO_EXONENT_MIN, TILE_SIZE_PX, TWILIGHT_OFFSET,
    TWILIGHT_WIDTH, WEB_MERCATOR_MAX_LATITUDE_DEG,
};
const GRID_STEPS_DEG: [f64; 12] = [
    30.0, 10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05, 0.01, 0.005, 0.001,
];
const TARGET_GRID_DIVISIONS: f64 = 5.0;
const MIN_GRID_STEP_DEG: f64 = 0.001;

/// Geographic grid spacing for the raster's projected pixel footprint.
pub fn grid_step(altitude_km: f64, width: usize, height: usize) -> f64 {
    let mpp = altitude_km * METERS_PER_KM * 2.0 * VERTICAL_FOV_HALF_TAN / height as f64;
    let span = (mpp * width as f64 / METERS_PER_LATITUDE_DEGREE).min(180.0);

    GRID_STEPS_DEG
        .iter()
        .copied()
        .find(|s| *s <= span / TARGET_GRID_DIVISIONS)
        .unwrap_or(MIN_GRID_STEP_DEG)
}

// ───────────────────────────── Rendering ─────────────────────────────

pub struct RenderCfg {
    pub sky: Option<celestial::SkyBackdrop>,
    pub provider: Provider,
    pub terrain: Option<TerrainCfg>,
    pub rel: u16,
    pub grid: bool,
    pub grid_step: f64,
    pub atmo: bool,
    pub sun: Option<DVec3>,
    /// Displayed-pixel density factor used for the level of detail: the
    /// per-pixel angle of a low-resolution interactive raster is divided by
    /// this scale so motion frames pick the same zoom band as idle frames.
    /// 1.0 for full-resolution renders.
    pub lod_scale: f32,
}

struct Local<'a, F> {
    lookup: &'a F,
    map: HashMap<TileKey, LocalTile, KeyBuild>,
}
enum LocalTile {
    Ready(Arc<Tile>),
    Pending,
    Absent(f32),
}
impl<F: Fn(TileKey) -> Lookup> Local<'_, F> {
    fn tile_color(&mut self, k: TileKey, dist: f32, u: f64, v: f64, stride: f32) -> Option<Vec4> {
        match self.map.entry(k).or_insert_with(|| match (self.lookup)(k) {
            Lookup::Ready(tile) => LocalTile::Ready(tile),
            Lookup::Pending => LocalTile::Pending,
            Lookup::Absent => LocalTile::Absent(dist),
        }) {
            LocalTile::Ready(tile) => Some(tile.sample(u, v, stride)),
            LocalTile::Pending => None,
            LocalTile::Absent(nearest) => {
                *nearest = nearest.min(dist);
                None
            }
        }
    }
    #[cfg(test)]
    fn misses(self) -> impl Iterator<Item = (TileKey, f32)> {
        self.map.into_iter().filter_map(|(key, tile)| match tile {
            LocalTile::Absent(dist) => Some((key, dist)),
            _ => None,
        })
    }
}

/// Fractional `2^x` table for per-pixel stride sizing: `EXP2_LUT[i] = 2^(i/TILE_EXP2_LUT_STEPS)`
/// for i ≤ 256, built once, so the per-pixel cost is an interpolated lookup
/// instead of a libm `powf` call.
static EXP2_LUT: LazyLock<[f64; 257]> = LazyLock::new(|| {
    let mut lut = [0.0; 257];
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = 2f64.powf(i as f64 / TILE_EXP2_LUT_STEPS);
    }
    lut
});

/// Samples the tile at level `z`; if it has not loaded yet, falls back to coarser
/// (parent) levels. Returns the color and the level it actually came from.
fn sample_chain(
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    prov: Provider,
    rel: u16,
    start: i32,
    s: &Surface,
) -> (Vec4, i32) {
    // Texels per raster pixel at level zz: the level of detail is
    // display-referenced, so reduced-scale motion frames and the oversampled
    // band base hold several texels per pixel. Past the filter-start ratio
    // those pixels area-average their texels instead of aliasing on four
    // bilinear taps; coarser fallback levels magnify and stay bilinear.
    //
    // The ratio is `2^exponent` and only the base level (and the blend walk
    // one level up) can pass the threshold: one level below the base the
    // ratio is ≤ 2^-frac < 1.25, so those levels skip the exponentiation
    // entirely, and the positive exponents resolve through a small
    // interpolated exp2 table instead of a libm call per pixel.
    let stride = |zz: i32| {
        let exponent = (zz - s.z0) as f64 + LOD_SHARPNESS_BIAS - s.frac as f64;
        if exponent <= TILE_FILTER_RATIO_EXONENT_MIN {
            return 1.0;
        }
        // The base sits at exponent 1 − frac and the blend walk one level up;
        // anything beyond reaches only the sampler's largest stride anyway.
        if exponent >= TILE_FILTER_RATIO_EXONENT_MAX {
            return Tile::MAX_SAMPLE_STRIDE as f32;
        }
        let whole = exponent.floor();
        let index = (exponent - whole) * TILE_EXP2_LUT_STEPS;
        let i = index as usize;
        let step = index - i as f64;
        let table = &*EXP2_LUT;
        // 2^whole is exact by construction (unit mantissa), and the fractional
        // part interpolates a 2^(i/steps) table; interpolation error stays
        // below ~3e-6 relative, invisible at 8-bit colors.
        let two = f64::from_bits((1023 + whole as u64) << 52);
        (table[i] + (table[i + 1] - table[i]) * step) as f32 * two as f32
    };
    let mut zz = start;
    while zz >= 0 {
        let n = (1u64 << zz) as f64;
        let tx = (s.u * n).floor().min(n - 1.0);
        let ty = (s.v * n).floor().min(n - 1.0);
        let k = make_key(prov, zz as u8, tx as u32, ty as u32, rel);
        if let Some(c) = loc.tile_color(k, s.dist, s.u * n - tx, s.v * n - ty, stride(zz)) {
            return (c, zz);
        }
        zz -= 1;
    }
    (MISSING_TILE_COLOR, -1)
}

fn shade_surface(
    surface: Surface,
    cfg: &RenderCfg,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    terrain_light: f32,
) -> Vec4 {
    let (mut col, _) = sample_chain(loc, cfg.provider, cfg.rel, surface.z0, &surface);
    let blend = if cfg.provider.is_imagery() {
        // Short fade at the end of the level band: with an oversampled base,
        // imagery stays sharp until the next level is almost fully in place.
        ((surface.frac - IMAGERY_ZOOM_FADE_START) / (1.0 - IMAGERY_ZOOM_FADE_START)).clamp(0.0, 1.0)
    } else {
        // Short fade at the end of the level band: text stays sharp until the
        // next level is almost fully in place.
        ((surface.frac - TEXT_ZOOM_FADE_START) / (1.0 - TEXT_ZOOM_FADE_START)).clamp(0.0, 1.0)
    };
    if blend > MIN_ZOOM_BLEND_FRACTION {
        let (c2, level) = sample_chain(loc, cfg.provider, cfg.rel, surface.z0 + 1, &surface);
        // Only blend when the finer level actually resolved. Blending a coarser
        // fallback back over the current image makes levels flash while tiles
        // load during zoom changes.
        if level == surface.z0 + 1 {
            col = col.lerp_monotonic(c2, blend);
        }
    }
    if surface.grid {
        col = col.lerp_monotonic(GRID_COLOR, GRID_BLEND);
    }
    col = col * (surface.shade * terrain_light) + Vec4::new(0.0, 0.0, surface.night, 0.0);
    col = col.lerp_monotonic(RIM_COLOR, surface.rim);
    col
}

fn render_row<const ELLIPSOID: bool>(
    y: usize,
    w: usize,
    h: usize,
    rays: &RayCaster,
    cfg: &RenderCfg,
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    row: &mut [u8],
) {
    let cc = rays.camera_len_squared;
    let pixels: &mut [[u8; RGBA_CHANNELS]] = bytemuck::cast_slice_mut(row);
    for (pack, pixels) in pixels.chunks_mut(RAY_LANES).enumerate() {
        let first_x = pack * RAY_LANES;
        let samples = rays.cast_model::<ELLIPSOID>(first_x, y);
        // A short final pack only shades real pixels; padded lanes make no lookups.
        for (lane, (pixel, ray)) in pixels.iter_mut().zip(samples).enumerate() {
            let x = first_x + lane;
            let bb = ray.camera_dot;
            let disc = ray.discriminant;
            let col = if disc >= 0.0 && bb < 0.0 {
                let geometry = rays.model_geometry();
                let hit = surface::<ELLIPSOID>(x, y, w, h, &ray, cfg, geometry);
                // Relief moves the hit along its ray; the imagery is then sampled there.
                match terrain::displace(&ray, (hit.u, hit.v), hit.z0, hit.dist, geometry, cfg, loc)
                {
                    Some((raised, light)) => shade_surface(
                        surface::<ELLIPSOID>(x, y, w, h, &raised, cfg, geometry),
                        cfg,
                        loc,
                        light,
                    ),
                    None => shade_surface(hit, cfg, loc, 1.0),
                }
            } else {
                sky(&ray, cc, cfg)
            };
            let channels = col.clamp(Vec4::ZERO, Vec4::ONE) * COLOR_CHANNEL_MAX;
            pixel[0] = channels.x as u8;
            pixel[1] = channels.y as u8;
            pixel[2] = channels.z as u8;
            pixel[3] = OPAQUE_ALPHA;
        }
    }
}

/// Renders an RGBA frame and queues missing tiles, coarse zoom levels first.
pub fn render_frame(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    cache: &TileCache,
) -> Vec<u8> {
    let frame = render_view(w, h, basis, cfg, cache);
    cache.prioritize_view(&frame.required_tiles());
    frame.pixels
}

pub struct RenderedView {
    pub pixels: Vec<u8>,
    /// Tiles that were unavailable when sampled, including work already pending.
    pub waiting: Vec<TileKey>,
    pub requests: Vec<TileKey>,
}

impl RenderedView {
    /// New misses and already-pending dependencies, in coarse-first order.
    pub fn required_tiles(&self) -> Vec<TileKey> {
        let mut keys = self.requests.clone();
        let admitted: std::collections::HashSet<_> = keys.iter().copied().collect();
        keys.extend(
            self.waiting
                .iter()
                .copied()
                .filter(|key| !admitted.contains(key)),
        );
        keys.sort_by_key(|&key| unkey(key).1);
        keys
    }
}

pub fn render_view(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    cache: &TileCache,
) -> RenderedView {
    render_view_into(w, h, basis, cfg, cache, Vec::new())
}

pub fn render_view_into(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    cache: &TileCache,
    buffer: Vec<u8>,
) -> RenderedView {
    let (buf, mut list, waiting) =
        render_pixels_into(w, h, basis, cfg, &|key| cache.lookup(key), buffer);

    // Request missing tiles: lowest zoom first, then closest to the center.
    list.sort_by(|a, b| {
        unkey(a.0)
            .1
            .cmp(&unkey(b.0).1)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let requests = list.into_iter().map(|(key, _)| key).collect();
    RenderedView {
        pixels: buf,
        waiting,
        requests,
    }
}

#[cfg(test)]
fn render_pixels(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    lookup: &(impl Fn(TileKey) -> Lookup + Sync),
) -> (Vec<u8>, Vec<(TileKey, f32)>) {
    let (pixels, misses, _) = render_pixels_tracked(w, h, basis, cfg, lookup);
    (pixels, misses)
}

#[cfg(test)]
fn render_pixels_tracked(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    lookup: &(impl Fn(TileKey) -> Lookup + Sync),
) -> (Vec<u8>, Vec<(TileKey, f32)>, Vec<TileKey>) {
    render_pixels_into(w, h, basis, cfg, lookup, Vec::new())
}

fn render_pixels_into(
    w: usize,
    h: usize,
    basis: &Basis,
    cfg: &RenderCfg,
    lookup: &(impl Fn(TileKey) -> Lookup + Sync),
    mut buf: Vec<u8>,
) -> (Vec<u8>, Vec<(TileKey, f32)>, Vec<TileKey>) {
    buf.resize(w * h * RGBA_CHANNELS, 0);
    let row_bytes = w * RGBA_CHANNELS;
    let rays = RayCaster::new(w, h, basis);
    let unit_sphere = rays.uses_unit_sphere();
    let per_batch: Vec<Vec<(TileKey, Option<f32>)>> = buf
        .par_chunks_mut(row_bytes * ROWS_PER_TILE_BATCH)
        .enumerate()
        .map(|(batch, rows)| {
            // Neighboring rows share tiles. Keep handles only for this small batch,
            // avoiding repeated Moka reads without retaining tiles between frames.
            let mut loc = Local {
                lookup,
                map: HashMap::default(),
            };
            for (offset, row) in rows.chunks_mut(row_bytes).enumerate() {
                if unit_sphere {
                    render_row::<false>(
                        batch * ROWS_PER_TILE_BATCH + offset,
                        w,
                        h,
                        &rays,
                        cfg,
                        &mut loc,
                        row,
                    );
                } else {
                    render_row::<true>(
                        batch * ROWS_PER_TILE_BATCH + offset,
                        w,
                        h,
                        &rays,
                        cfg,
                        &mut loc,
                        row,
                    );
                }
            }
            loc.map
                .into_iter()
                .filter_map(|(key, tile)| match tile {
                    LocalTile::Ready(_) => None,
                    LocalTile::Pending => Some((key, None)),
                    LocalTile::Absent(distance) => Some((key, Some(distance))),
                })
                .collect()
        })
        .collect();

    let mut misses: HashMap<TileKey, f32> = HashMap::new();
    let mut waiting = std::collections::HashSet::new();
    for (k, distance) in per_batch.into_iter().flatten() {
        waiting.insert(k);
        if let Some(d) = distance {
            let e = misses.entry(k).or_insert(f32::MAX);
            *e = e.min(d);
        }
    }
    (
        buf,
        misses.into_iter().collect(),
        waiting.into_iter().collect(),
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod benchmarks;

pub mod aircraft;
pub mod planet;
pub mod scale_bar;
pub mod weather;

pub mod view;

pub mod live_layers;
