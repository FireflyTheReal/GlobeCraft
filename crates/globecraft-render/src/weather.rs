//! Georeferenced cloud/radar sampling and compositing without GUI types.
use emath::{Pos2, Rect};
use globecraft_core::camera::Basis;
use image::RgbaImage;
const RADAR_OPACITY: f32 = 0.88;
const RADAR_LATITUDE_LIMIT_DEG: f64 = 85.0;
/// The rain radar seen from `basis`, at its `opacity`, as a `width` × `height`
/// straight-alpha RGBA image of `rect`. (The clouds are a layer above the ground of
/// their own: `clouds`.)
pub fn project_radar(
    basis: &Basis,
    rect: Rect,
    width: usize,
    height: usize,
    radar: (&RgbaImage, f32),
) -> Vec<u8> {
    let mut rgba = vec![0u8; width * height * 4];
    let rsize = rect.size();
    for y in 0..height {
        let sy = rect.top() + (y as f32 + 0.5) / height as f32 * rsize.y;
        for x in 0..width {
            let sx = rect.left() + (x as f32 + 0.5) / width as f32 * rsize.x;
            let Some((lat, lon)) = basis.pick(rect, Pos2::new(sx, sy)) else {
                continue;
            };
            let i = (y * width + x) * 4;
            // At the user's opacity (roadmap 6.1), times its own.
            if lat.abs() < RADAR_LATITUDE_LIMIT_DEG {
                let (img, opacity) = radar;
                let r = mercator_pixel(img, lat, lon);
                over(
                    &mut rgba[i..i + 4],
                    [
                        r[0],
                        r[1],
                        r[2],
                        (r[3] as f32 * RADAR_OPACITY * opacity) as u8,
                    ],
                );
            }
        }
    }
    rgba
}
/// The user's web map layers (Add Data) seen from `basis`, bottom to top, each at its
/// opacity, as a `width` × `height` straight-alpha RGBA image of `rect`.
pub fn project_user_rasters(
    basis: &Basis,
    rect: Rect,
    width: usize,
    height: usize,
    layers: &[(&globecraft_data::user_data::RasterSnapshot, f32)],
) -> Vec<u8> {
    use rayon::prelude::*;
    let mut rgba = vec![0u8; width * height * 4];
    let rsize = rect.size();
    rgba.par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let sy = rect.top() + (y as f32 + 0.5) / height as f32 * rsize.y;
            for x in 0..width {
                let sx = rect.left() + (x as f32 + 0.5) / width as f32 * rsize.x;
                let Some((lat, lon)) = basis.pick(rect, Pos2::new(sx, sy)) else {
                    continue;
                };
                let pixel = &mut row[x * 4..x * 4 + 4];
                for (layer, opacity) in layers {
                    if let Some(c) = layer.sample(lat, lon) {
                        over(pixel, [c[0], c[1], c[2], (f32::from(c[3]) * opacity) as u8]);
                    }
                }
            }
        });
    rgba
}

fn mercator_pixel(img: &RgbaImage, lat: f64, lon: f64) -> [u8; 4] {
    let x = (((lon + 180.0) / 360.0 * img.width() as f64) as u32).min(img.width() - 1);
    let radians = lat.to_radians();
    let y = ((1.0 - radians.tan().asinh() / std::f64::consts::PI) / 2.0 * img.height() as f64)
        .clamp(0.0, (img.height() - 1) as f64) as u32;
    img.get_pixel(x, y).0
}
pub(crate) fn over(dst: &mut [u8], src: [u8; 4]) {
    let a = src[3] as f32 / 255.0;
    if a == 0.0 {
        return;
    }
    let previous = dst[3] as f32 / 255.0;
    let total = a + previous * (1.0 - a);
    if total == 0.0 {
        return;
    }
    for i in 0..3 {
        dst[i] = ((src[i] as f32 * a + dst[i] as f32 * previous * (1.0 - a)) / total) as u8;
    }
    dst[3] = (total * 255.0) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_blend() {
        let mut pixel = [0u8; 4];
        over(&mut pixel, [0, 200, 0, 128]);
        assert!(pixel[1] > 190 && pixel[3] > 120);
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use emath::Vec2;
    use globecraft_core::camera::Cam;
    #[test]
    fn radar_raster_keeps_its_opacity_and_leaves_space_clear() {
        let rect = Rect::from_min_size(Pos2::new(30.0, 50.0), Vec2::splat(800.0));
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            alt: 14_000.0,
            heading: 0.0,
            tilt: 0.0,
        }
        .basis(1.0);
        let radar = RgbaImage::from_pixel(4, 4, image::Rgba([123, 45, 67, 255]));
        let pixels = project_radar(&basis, rect, 9, 9, (&radar, 1.0));
        assert_eq!(
            &pixels[(4 * 9 + 4) * 4..(4 * 9 + 5) * 4],
            &[123, 45, 67, 224]
        );
        assert_eq!(&pixels[..4], &[0, 0, 0, 0]);
    }
}

/// Project provider's transparent 3x3 congestion raster onto the curved globe.
pub fn project_traffic_tiles(
    basis: &Basis,
    rect: Rect,
    width: usize,
    height: usize,
    tiles: &globecraft_data::traffic::TrafficTiles,
) -> Vec<u8> {
    let mut out = vec![0; width * height * 4];
    for py in 0..height {
        let sy = rect.top() + (py as f32 + 0.5) * rect.height() / height as f32;
        for px in 0..width {
            let sx = rect.left() + (px as f32 + 0.5) * rect.width() / width as f32;
            if let Some((lat, lon)) = basis.pick(rect, Pos2::new(sx, sy))
                && let Some(mut rgba) = tiles.sample(lat, lon)
            {
                rgba[3] = (rgba[3] as f32 * 0.90) as u8;
                out[(py * width + px) * 4..(py * width + px) * 4 + 4].copy_from_slice(&rgba);
            }
        }
    }
    out
}
