//! Elevation tiles prepared for ray marching: heights in metres, decoded once, and a
//! pyramid of block maxima. A ray above a block's maximum cannot hit anything inside
//! it, so it skips the whole block in one step (the maximum-mipmap traversal).

/// Heights below sea level are drawn at sea level: the globe's surface is the floor
/// the ray marcher stops at (bathymetry and the Dead Sea shore read as flat water).
const SEA_LEVEL_M: f32 = 0.0;

/// A square grid of heights and its block maxima.
pub struct Elevation {
    size: usize,
    heights: Vec<f32>,
    /// `maxima[k]` holds one value per block of `2^k` × `2^k` texels (row-major,
    /// `size >> k` per side): the highest height a bilinear sample anywhere in that
    /// block can return. Level 0 is each texel with its neighbours (bilinear taps
    /// reach one texel across a block edge); the last level is the whole tile.
    maxima: Vec<Vec<f32>>,
}

impl Elevation {
    /// Decodes Terrarium RGB, `(R * 256 + G + B / 256) - 32768` metres.
    pub fn from_terrarium(width: usize, height: usize, rgba: &[u8]) -> Option<Self> {
        if width != height || !width.is_power_of_two() || rgba.len() != width * height * 4 {
            return None;
        }
        let heights = rgba
            .chunks_exact(4)
            .map(|p| {
                let metres =
                    f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32768.0;
                metres.max(SEA_LEVEL_M)
            })
            .collect();
        Some(Self::from_heights(width, heights))
    }

    /// A tile from heights in metres (row-major, `size` × `size`, a power of two).
    pub fn from_heights(size: usize, heights: Vec<f32>) -> Self {
        assert!(size.is_power_of_two() && heights.len() == size * size);
        // Level 0: each texel with its 3×3 neighbourhood, clamped at the tile edge.
        let at = |x: isize, y: isize| {
            let clamp = |c: isize| c.clamp(0, size as isize - 1) as usize;
            heights[clamp(y) * size + clamp(x)]
        };
        let mut level: Vec<f32> = (0..size * size)
            .map(|i| {
                let (x, y) = ((i % size) as isize, (i / size) as isize);
                let mut m = f32::MIN;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        m = m.max(at(x + dx, y + dy));
                    }
                }
                m
            })
            .collect();
        let mut maxima = Vec::with_capacity(size.trailing_zeros() as usize + 1);
        let mut side = size;
        loop {
            let next_side = side / 2;
            let next = (next_side > 0).then(|| {
                (0..next_side * next_side)
                    .map(|i| {
                        let (x, y) = (2 * (i % next_side), 2 * (i / next_side));
                        level[y * side + x]
                            .max(level[y * side + x + 1])
                            .max(level[(y + 1) * side + x])
                            .max(level[(y + 1) * side + x + 1])
                    })
                    .collect::<Vec<f32>>()
            });
            maxima.push(level);
            match next {
                Some(next) => {
                    level = next;
                    side = next_side;
                }
                None => break,
            }
        }
        Self {
            size,
            heights,
            maxima,
        }
    }

    /// Texels per side.
    pub fn size(&self) -> usize {
        self.size
    }
    /// The coarsest level: one block for the whole tile.
    pub fn top_level(&self) -> usize {
        self.maxima.len() - 1
    }
    /// The highest point of the tile, in metres.
    pub fn max_m(&self) -> f32 {
        self.maxima[self.top_level()][0]
    }
    /// The highest bilinear height inside block `(bx, by)` of `level` (blocks of
    /// `2^level` texels), in metres.
    #[inline]
    pub fn block_max_m(&self, level: usize, bx: usize, by: usize) -> f32 {
        let side = self.size >> level;
        self.maxima[level][by.min(side - 1) * side + bx.min(side - 1)]
    }
    /// Bilinear height at texel coordinates `(x, y)` (texel centres at `i + 0.5`).
    #[inline]
    pub fn height_m(&self, x: f64, y: f64) -> f32 {
        let last = self.size - 1;
        let x = (x - 0.5).clamp(0.0, last as f64);
        let y = (y - 0.5).clamp(0.0, last as f64);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(last), (y0 + 1).min(last));
        let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        let h = |xx: usize, yy: usize| self.heights[yy * self.size + xx];
        let top = h(x0, y0) + (h(x1, y0) - h(x0, y0)) * fx;
        let bottom = h(x0, y1) + (h(x1, y1) - h(x0, y1)) * fx;
        top + (bottom - top) * fy
    }
    /// Memory held, for the cache budget.
    pub fn bytes(&self) -> usize {
        (self.heights.len() + self.maxima.iter().map(Vec::len).sum::<usize>())
            * std::mem::size_of::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_maxima_bound_every_bilinear_sample_inside_the_block() {
        let size = 16;
        let heights: Vec<f32> = (0..size * size)
            .map(|i| (((i * 7919) % 101) as f32) * 10.0)
            .collect();
        let tile = Elevation::from_heights(size, heights);
        assert_eq!(tile.top_level(), 4);
        for level in 0..=tile.top_level() {
            let block = 1 << level;
            for step in 0..400 {
                let x = (step % 20) as f64 * size as f64 / 20.0 + 0.013;
                let y = (step / 20) as f64 * size as f64 / 20.0 + 0.007;
                let (bx, by) = (x as usize / block, y as usize / block);
                assert!(tile.height_m(x, y) <= tile.block_max_m(level, bx, by) + 1e-3);
            }
        }
        assert_eq!(tile.max_m(), 1000.0);
    }

    #[test]
    fn terrarium_decodes_metres_and_floors_the_sea() {
        // 1000 m = [131, 232, 0]; -50 m = [127, 206, 0].
        let mut rgba = Vec::new();
        for _ in 0..2 {
            rgba.extend_from_slice(&[131, 232, 0, 255, 127, 206, 0, 255]);
        }
        let tile = Elevation::from_terrarium(2, 2, &rgba).unwrap();
        assert_eq!(tile.height_m(0.5, 0.5), 1000.0);
        assert_eq!(tile.height_m(1.5, 0.5), 0.0);
        assert!(Elevation::from_terrarium(3, 3, &[0; 36]).is_none());
    }
}
