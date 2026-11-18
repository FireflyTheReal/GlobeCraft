//! Motion frames are drawn at a fraction of the screen's size and stretched to it,
//! which softens them. They are sharpened as AMD's contrast-adaptive sharpening (CAS,
//! FidelityFX) does: each pixel is pushed away from its four neighbours, less where
//! the contrast is already high, so edges grow crisper without halos (roadmap B2).
use rayon::prelude::*;

/// How strongly (0 to 1): CAS's "sharpness", its neighbours' weight going from −1/8
/// to −1/5.
const MOTION_SHARPNESS: f32 = 0.6;
const BYTES_PER_PIXEL: usize = 4;
const OPAQUE: u8 = u8::MAX;

/// Sharpens a `width`×`height` RGBA frame that is shown larger than it was drawn
/// (`lod_scale` below one). Pixels that are not opaque, or beside one that is not
/// (the sky, the Earth's limb), are left as they are.
pub(crate) fn motion_frame(pixels: &mut [u8], width: usize, height: usize, lod_scale: f32) {
    if lod_scale >= 1.0 || width < 3 || height < 3 {
        return;
    }
    let row = width * BYTES_PER_PIXEL;
    if pixels.len() < row * height {
        return;
    }
    let source = pixels[..row * height].to_vec();
    let peak = -1.0 / (8.0 + (5.0 - 8.0) * MOTION_SHARPNESS);
    pixels[row..row * (height - 1)]
        .par_chunks_mut(row)
        .enumerate()
        .for_each(|(index, out)| {
            let up = &source[index * row..][..row];
            let middle = &source[(index + 1) * row..][..row];
            let down = &source[(index + 2) * row..][..row];
            for x in 1..width - 1 {
                let at = x * BYTES_PER_PIXEL;
                let (left, right) = (at - BYTES_PER_PIXEL, at + BYTES_PER_PIXEL);
                let alpha = middle[at + 3] & up[at + 3] & down[at + 3];
                if alpha & middle[left + 3] & middle[right + 3] != OPAQUE {
                    continue;
                }
                // How much, from the green channel's contrast (CAS's cheaper form):
                // one weight for all three colours.
                let green = |row: &[u8], at: usize| f32::from(row[at + 1]);
                let cross = [
                    green(middle, at),
                    green(up, at),
                    green(down, at),
                    green(middle, left),
                    green(middle, right),
                ];
                let low = cross.iter().copied().fold(f32::MAX, f32::min);
                let high = cross.iter().copied().fold(0.0, f32::max);
                if high <= 0.0 {
                    continue;
                }
                let amount = (low.min(255.0 - high) / high).clamp(0.0, 1.0).sqrt();
                let weight = amount * peak;
                let scale = 1.0 / (1.0 + 4.0 * weight);
                for channel in 0..3 {
                    let around = f32::from(up[at + channel])
                        + f32::from(down[at + channel])
                        + f32::from(middle[left + channel])
                        + f32::from(middle[right + channel]);
                    let value = (f32::from(middle[at + channel]) + weight * around) * scale;
                    out[at + channel] = value.clamp(0.0, 255.0).round() as u8;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of `width`×3 grey pixels from `values`, repeated in every row.
    fn frame(values: &[u8], alpha: u8) -> Vec<u8> {
        (0..3)
            .flat_map(|_| values.iter().flat_map(|&v| [v, v, v, alpha]))
            .collect()
    }

    #[test]
    fn an_edge_grows_steeper_and_flat_ground_stays() {
        let values = [100, 100, 100, 160, 160, 160];
        let mut pixels = frame(&values, OPAQUE);
        motion_frame(&mut pixels, values.len(), 3, 0.4);
        let middle: Vec<u8> = (0..values.len()).map(|x| pixels[(6 + x) * 4]).collect();
        assert_eq!(middle[1], 100);
        assert!(middle[2] < 100 && middle[3] > 160, "{middle:?}");
        assert_eq!(middle[4], 160);
        // The edge rows and columns, and the alpha, are untouched.
        assert_eq!(&pixels[..24], &frame(&values, OPAQUE)[..24]);
        assert!(pixels.chunks(4).all(|pixel| pixel[3] == OPAQUE));
    }

    #[test]
    fn full_frames_and_the_sky_are_left_alone() {
        let values = [100, 100, 100, 160, 160, 160];
        for (alpha, lod_scale) in [(OPAQUE, 1.0), (200, 0.4)] {
            let mut pixels = frame(&values, alpha);
            motion_frame(&mut pixels, values.len(), 3, lod_scale);
            assert_eq!(pixels, frame(&values, alpha));
        }
    }
}
