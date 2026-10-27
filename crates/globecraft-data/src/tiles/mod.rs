use crate::wayback::wayback_tile_url;
use glam::Vec4;
use globecraft_core::language::Lang;
use std::hash::{BuildHasherDefault, Hasher};
// Tile-key bit offsets/masks are part of the packed-key contract.
const PROVIDER_SHIFT: u32 = 59;
const ZOOM_SHIFT: u32 = 54;
const RELEASE_SHIFT: u32 = 38;
const X_SHIFT: u32 = 19;
const PROVIDER_MASK: u64 = 31;
const ZOOM_MASK: u64 = 31;
const COORDINATE_MASK: u64 = 0x7FFFF;
const RELEASE_MASK: u64 = 0xFFFF;
// Cache/network policy: preserve both the thresholds and the eviction strategy.

// ───────────────────────────── Tiles ─────────────────────────────

/// A tile source, by its id (see `source::TileSource` and `sources.json`). The named
/// constants are the built-in sources the code refers to directly.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Provider(u8);

#[allow(non_upper_case_globals)]
impl Provider {
    pub const Satellite: Provider = Provider(0);
    pub const Streets: Provider = Provider(1);
    pub const Topo: Provider = Provider(2);
    pub const Wayback: Provider = Provider(3);
    /// Mapterhorn Terrarium WebP elevation, for the native globe only.
    pub const TerrainDem: Provider = Provider(4);
    /// NASA Black Marble night lights, drawn on the night side.
    pub const NightLights: Provider = Provider(5);
}

impl Provider {
    /// The source's description.
    pub fn source(self) -> &'static TileSource {
        source::REGISTRY.by_id[usize::from(self.0)]
            .as_ref()
            .expect("tile keys only name registered sources")
    }
    /// Every registered source, in listing order.
    pub fn all() -> impl Iterator<Item = Provider> {
        source::REGISTRY.order.iter().map(|&id| Provider(id))
    }
    /// The sources offered as basemaps, in listing order.
    pub fn basemaps() -> impl Iterator<Item = Provider> {
        Self::all().filter(|provider| provider.source().basemap)
    }
    /// Stable identifier for settings and commands.
    pub fn code(self) -> &'static str {
        &self.source().code
    }
    /// The basemap with this code; elevation tiles and overlays are not a basemap.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::basemaps().find(|provider| provider.code() == code)
    }
    /// Any source with this code.
    pub fn from_any_code(code: &str) -> Option<Self> {
        Self::all().find(|provider| provider.code() == code)
    }
    pub fn id(self) -> u64 {
        u64::from(self.0)
    }
    fn from_id(i: u64) -> Provider {
        let id = (i as usize).min(source::MAX_SOURCES - 1) as u8;
        if source::REGISTRY.by_id[usize::from(id)].is_some() {
            Provider(id)
        } else {
            Provider::Topo
        }
    }
    /// Imagery providers get smooth blending between zoom levels.
    pub fn is_imagery(self) -> bool {
        self.source().kind == SourceKind::Imagery
    }
    pub fn name(self, lg: Lang) -> &'static str {
        let source = self.source();
        source
            .name_key
            .as_deref()
            .map_or(source.name.as_str(), |key| lg.text(key))
    }
    fn dir(self) -> &'static str {
        &self.source().cache_dir
    }
    pub fn max_zoom(self) -> u8 {
        self.source().max_zoom
    }
    fn url(self, z: u8, x: u32, y: u32, rel: u16) -> String {
        let source = self.source();
        match source.access {
            Access::Wayback => wayback_tile_url(rel, z, x, y),
            Access::Xyz { .. } => source.tile_url(z, x, y).unwrap_or_default(),
        }
    }
    pub fn attribution(self, lg: Lang) -> &'static str {
        let source = self.source();
        source
            .attribution_key
            .as_deref()
            .map_or(source.attribution.as_str(), |key| lg.text(key))
    }
}

pub type TileKey = u64;
// Key layout: y 19 bits | x 19 bits | release index 16 bits | zoom 5 bits | source 5 bits.
// `rel` is the index of the Wayback release (0 for every other provider).
pub fn make_key(p: Provider, z: u8, x: u32, y: u32, rel: u16) -> TileKey {
    (p.id() << PROVIDER_SHIFT)
        | ((z as u64) << ZOOM_SHIFT)
        | ((rel as u64) << RELEASE_SHIFT)
        | ((x as u64) << X_SHIFT)
        | (y as u64)
}
pub fn unkey(k: TileKey) -> (Provider, u8, u32, u32, u16) {
    (
        Provider::from_id((k >> PROVIDER_SHIFT) & PROVIDER_MASK),
        ((k >> ZOOM_SHIFT) & ZOOM_MASK) as u8,
        ((k >> X_SHIFT) & COORDINATE_MASK) as u32,
        (k & COORDINATE_MASK) as u32,
        ((k >> RELEASE_SHIFT) & RELEASE_MASK) as u16,
    )
}

#[derive(Default)]
pub struct KeyHasher(u64);
impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 29)
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }
    fn write_u64(&mut self, k: u64) {
        self.0 = k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}
pub type KeyBuild = BuildHasherDefault<KeyHasher>;

/// Most texels one axis of an area sample can touch: the stride cap plus a partial texel at
/// each edge.
const MAX_AREA_SPAN_TEXELS: usize = 11;

pub struct Tile {
    w: usize,
    h: usize,
    px: Vec<u8>, // RGBA
    /// Elevation tiles keep heights and block maxima instead of colour (`px` empty).
    elevation: Option<Elevation>,
}
impl Tile {
    /// Decodes a Terrarium elevation tile into heights and block maxima.
    pub fn decode_elevation(bytes: &[u8]) -> Result<Self, crate::Error> {
        let img = image::load_from_memory(bytes)
            .map_err(crate::Error::from)?
            .to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let elevation = Elevation::from_terrarium(w, h, img.as_raw())
            .ok_or("elevation tiles must be square with a power-of-two size")?;
        Ok(Self {
            w,
            h,
            px: Vec::new(),
            elevation: Some(elevation),
        })
    }
    /// An elevation tile from prepared heights (fixtures and benchmarks).
    pub fn from_elevation(elevation: Elevation) -> Self {
        Self {
            w: elevation.size(),
            h: elevation.size(),
            px: Vec::new(),
            elevation: Some(elevation),
        }
    }
    /// Heights and block maxima, for elevation tiles.
    pub fn elevation(&self) -> Option<&Elevation> {
        self.elevation.as_ref()
    }
    /// Decoded memory held, for the cache budget.
    pub fn bytes(&self) -> usize {
        self.px.len() + self.elevation.as_ref().map_or(0, Elevation::bytes)
    }
    /// Decodes the same PNG/JPEG tile bytes used by the disk and network loaders.
    pub fn decode(bytes: &[u8]) -> Result<Self, crate::Error> {
        let img = image::load_from_memory(bytes)
            .map_err(crate::Error::from)?
            .to_rgba8();
        Ok(Self {
            w: img.width() as usize,
            h: img.height() as usize,
            px: img.into_raw(),
            elevation: None,
        })
    }

    /// Decoded RGBA layout for backend texture upload; storage remains tile-owned.
    pub fn rgba(&self) -> (usize, usize, &[u8]) {
        (self.w, self.h, &self.px)
    }

    /// RGB in SIMD lanes, with the unused fourth lane set to zero. A `stride`
    /// above one area-averages roughly that many texels per axis instead:
    /// minified tiles (an oversampled band, or a reduced-scale motion raster)
    /// otherwise alias into a low-bitrate look, since bilinear only ever sees
    /// two texels per axis regardless of how many fall under the pixel.
    pub fn sample(&self, u: f64, v: f64, stride: f32) -> Vec4 {
        if stride <= 1.0 {
            return self.sample_bilinear(u, v);
        }
        self.sample_area(u, v, (stride as f64).min(Self::MAX_SAMPLE_STRIDE))
    }

    /// Bilinear RGB in SIMD lanes, with the unused fourth lane set to zero.
    fn sample_bilinear(&self, u: f64, v: f64) -> Vec4 {
        let x = (u * self.w as f64 - 0.5).max(0.0);
        let y = (v * self.h as f64 - 0.5).max(0.0);
        let x0 = (x as usize).min(self.w - 1);
        let y0 = (y as usize).min(self.h - 1);
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let fx = ((x - x0 as f64) as f32).min(1.0);
        let fy = ((y - y0 as f64) as f32).min(1.0);
        let pixels: &[[u8; 4]] = bytemuck::cast_slice(&self.px);
        let pixel = |xx: usize, yy: usize| {
            let rgba = pixels[yy * self.w + xx];
            Vec4::new(rgba[0] as f32, rgba[1] as f32, rgba[2] as f32, 0.0)
        };
        let (a, b, c, d) = (pixel(x0, y0), pixel(x1, y0), pixel(x0, y1), pixel(x1, y1));
        let top = a.lerp_monotonic(b, fx);
        let bottom = c.lerp_monotonic(d, fx);
        top.lerp_monotonic(bottom, fy) / 255.0
    }

    /// Largest supported area-average stride. The renderer's level of detail
    /// keeps the real stride at 4 texels per axis or below; the cap only
    /// bounds the work an out-of-range caller could request per sample.
    pub const MAX_SAMPLE_STRIDE: f64 = 8.0;

    /// Covers of each texel under a `stride`-wide box centered on the sample
    /// point, accumulated with partial-overlap weights. Border clamping keeps
    /// edge texels reachable so tiles meet their neighbors the same way the
    /// bilinear path does.
    fn sample_area(&self, u: f64, v: f64, stride: f64) -> Vec4 {
        let pixels: &[[u8; 4]] = bytemuck::cast_slice(&self.px);
        let center_x = u * self.w as f64 - 0.5;
        let center_y = v * self.h as f64 - 0.5;
        // Half-open texel index ranges whose cells can overlap the box,
        // clamped into the tile. A texel at index t covers [t-0.5, t+0.5], so
        // the edges convert to indices by folding the half-texel offset in.
        let span = |center: f64, size: i64| {
            let lower_edge = (center - stride * 0.5).max(0.0);
            let upper_edge = (center + stride * 0.5).min(size as f64);
            (
                (lower_edge + 0.5).floor().max(0.0) as i64,
                ((upper_edge + 0.5).floor() as i64 + 1).min(size),
            )
        };
        let (tx_lo, tx_hi) = span(center_x, self.w as i64);
        let (ty_lo, ty_hi) = span(center_y, self.h as i64);
        // Overlap of texel t's cell with the box, clamped to the tile bounds.
        let weight = |t: i64, center: f64, size: i64| {
            let lo = (t as f64 - 0.5).max(center - stride * 0.5).max(0.0);
            let hi = (t as f64 + 0.5).min(center + stride * 0.5).min(size as f64);
            (hi - lo).max(0.0)
        };
        // The box weight is separable: each texel's weight is its column overlap times its
        // row overlap. Column weights are computed once (the box spans at most
        // `MAX_AREA_SPAN_TEXELS` per axis), each row is summed with them, and the row sum is
        // scaled by the row weight, instead of weighting every texel twice.
        let mut column_weights = [0.0f32; MAX_AREA_SPAN_TEXELS];
        let columns =
            &mut column_weights[..(tx_hi - tx_lo).clamp(0, MAX_AREA_SPAN_TEXELS as i64) as usize];
        for (offset, wx) in columns.iter_mut().enumerate() {
            *wx = weight(tx_lo + offset as i64, center_x, self.w as i64) as f32;
        }
        let column_total: f32 = columns.iter().sum();
        let mut sum = Vec4::ZERO;
        let mut total = 0.0f32;
        for ty in ty_lo..ty_hi {
            let wy = weight(ty, center_y, self.h as i64) as f32;
            if wy <= 0.0 {
                continue;
            }
            let row = &pixels[ty as usize * self.w + tx_lo as usize..];
            let row_sum = columns
                .iter()
                .zip(row)
                .fold(Vec4::ZERO, |acc, (&wx, rgba)| {
                    acc + Vec4::new(rgba[0] as f32, rgba[1] as f32, rgba[2] as f32, 0.0) * wx
                });
            sum += row_sum * wy;
            total += column_total * wy;
        }
        if total <= 0.0 {
            // Degenerate clamped corner: fall back to the nearest texel.
            let tx = (center_x.round().max(0.0) as usize).min(self.w - 1);
            let ty = (center_y.round().max(0.0) as usize).min(self.h - 1);
            let rgba = pixels[ty * self.w + tx];
            return Vec4::new(rgba[0] as f32, rgba[1] as f32, rgba[2] as f32, 0.0) / 255.0;
        }
        sum / total / 255.0
    }
}

mod cache;
mod disk;
pub mod source;
pub use source::{Access, Prefetch, SourceKind, TileSource};
mod elevation;
pub use cache::{
    CacheBudgets, DEFAULT_TILE_CACHE_BUDGET_BYTES, Lookup, MAX_INFLIGHT, SourceQueue, TileCache,
};
pub use elevation::Elevation;
mod warmup;
pub use warmup::TileWarmup;

#[cfg(test)]
mod sampling_tests {
    use super::*;
    #[test]
    fn simd_sampling_preserves_pixel_centers_bilinear_weights_and_unused_lane() {
        let tile = Tile {
            w: 2,
            h: 2,
            px: vec![
                255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 128, 255, 255, 255, 64,
            ],
            elevation: None,
        };
        assert_eq!(tile.sample(0.25, 0.25, 1.0), Vec4::new(1.0, 0.0, 0.0, 0.0));
        assert_eq!(tile.sample(0.75, 0.25, 1.0), Vec4::new(0.0, 1.0, 0.0, 0.0));
        assert_eq!(tile.sample(0.5, 0.5, 1.0), Vec4::new(0.5, 0.5, 0.5, 0.0));
        assert_eq!(tile.sample(-1.0, -1.0, 1.0), tile.sample(0.25, 0.25, 1.0));
        assert_eq!(tile.sample(2.0, 2.0, 1.0), Vec4::new(1.0, 1.0, 1.0, 0.0));
    }

    #[test]
    fn bilinear_remains_exactly_the_legacy_two_tap_path_at_unit_stride() {
        let tile = Tile {
            w: 4,
            h: 1,
            px: vec![
                0, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 10, 10, 10, 255,
            ],
            elevation: None,
        };
        // Between texel 1 (red) and texel 2 (blue), level weights/lane zero.
        assert_eq!(tile.sample(0.5, 0.5, 1.0), Vec4::new(0.5, 0.0, 0.5, 0.0));
    }

    #[test]
    fn area_average_matches_the_mean_of_fully_covered_texels() {
        // Stride-2 box centered between texels 1 and 2 covers exactly those
        // two cells, so the result must be their exact mean.
        let tile = Tile {
            w: 4,
            h: 1,
            px: vec![
                0, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 10, 10, 10, 255,
            ],
            elevation: None,
        };
        assert_eq!(tile.sample(0.5, 0.5, 2.0), Vec4::new(0.5, 0.0, 0.5, 0.0));
    }

    #[test]
    fn area_average_flattens_a_checkerboard_while_bilinear_snaps_to_a_phase() {
        // A one-texel checker sampled with several texels under each pixel:
        // bilinear lands between two texels and reports whichever phase it
        // hit, while the box average converges on the constant mean.
        let px: Vec<u8> = (0..16)
            .flat_map(|i| {
                if i % 2 == 0 {
                    [0, 0, 0, 255]
                } else {
                    [255u8; 4]
                }
            })
            .collect();
        let tile = Tile {
            w: 16,
            h: 1,
            px,
            elevation: None,
        };
        let bilinear = tile.sample(0.53, 0.5, 1.0);
        let area = tile.sample(0.53, 0.5, 4.0);
        assert!(
            (bilinear.x - 0.5).abs() > 0.4,
            "bilinear minification aliases to a phase color, got {bilinear:?}"
        );
        assert!(
            (area.x - 0.5).abs() < 0.02 && (area.y - 0.5).abs() < 0.02,
            "area average must return the texel mean, got {area:?}"
        );
    }
}
