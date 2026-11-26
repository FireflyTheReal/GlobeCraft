//! Matching imagery levels' colours (roadmap 8.7, C6). Esri's imagery changes its
//! source between levels 11 and 12 (mosaics to 11, aerial and satellite photographs
//! from 12), and the two differ by 15 to 45 % in brightness over the same ground
//! (measured 2026-11-18 over New York, the Alps, the Sahara, the Amazon and Berlin;
//! the other level changes stay within a few per cent). That showed as a band across
//! tilted views and a jump of colour while zooming in. Level 12 is brought to level
//! 11's colours over blocks of about 2 km, and the finer levels come back to their
//! own over the next few.
use crate::{Local, pixel::Surface};
use glam::Vec4;
use globecraft_data::tiles::{Lookup, MEAN_CELLS, Provider, TileKey, make_key};

/// The last level of the one source and the first of the other.
pub(crate) const COARSE_LEVEL: i32 = 11;
pub(crate) const FINE_LEVEL: i32 = 12;
/// From this level on the imagery keeps its own colours.
pub(crate) const OWN_COLOURS_LEVEL: i32 = 16;
/// Each colour is changed by no more than this factor either way.
pub(crate) const MAX_GAIN: f32 = 1.7;
/// Keeps a nearly black block from asking for a huge gain (colour, 0 to 1).
pub(crate) const DARK_FLOOR: f32 = 0.02;

/// How much of level 11's colours a level takes on: all at 12, none from 16.
fn weight(level: i32) -> f32 {
    if level < FINE_LEVEL {
        return 0.0;
    }
    ((OWN_COLOURS_LEVEL - level) as f32 / (OWN_COLOURS_LEVEL - FINE_LEVEL) as f32).clamp(0.0, 1.0)
}

/// `colour`, sampled at `level` of `provider`'s imagery where `surface` is, brought
/// toward level 11's colours there. Unchanged until both level 11's and level 12's
/// tiles there are loaded, and for other maps than Esri's imagery.
pub(crate) fn matched(
    loc: &mut Local<impl Fn(TileKey) -> Lookup>,
    provider: Provider,
    release: u16,
    surface: &Surface,
    colour: Vec4,
    level: i32,
) -> Vec4 {
    let weight = weight(level);
    if weight <= 0.0 || !matches!(provider, Provider::Satellite | Provider::Wayback) {
        return colour;
    }
    let mut mean = |level: i32, cells: usize| {
        let n = (1u64 << level) as f64;
        let tx = (surface.u * n).floor().min(n - 1.0);
        let ty = (surface.v * n).floor().min(n - 1.0);
        let key = make_key(provider, level as u8, tx as u32, ty as u32, release);
        loc.mean_colour(
            key,
            surface.dist,
            surface.u * n - tx,
            surface.v * n - ty,
            cells,
        )
    };
    // Over the same ground: level 11's blocks are an eighth of its tile, level 12's
    // a quarter of its own.
    let Some(coarse) = mean(COARSE_LEVEL, MEAN_CELLS) else {
        return colour;
    };
    let Some(fine) = mean(FINE_LEVEL, MEAN_CELLS / 2) else {
        return colour;
    };
    let floor = Vec4::splat(DARK_FLOOR);
    let gain = ((coarse + floor) / (fine + floor))
        .clamp(Vec4::splat(1.0 / MAX_GAIN), Vec4::splat(MAX_GAIN));
    let gain = Vec4::ONE + (gain - Vec4::ONE) * weight;
    (colour * gain.truncate().extend(1.0)).min(Vec4::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_twelve_takes_on_eleven_and_sixteen_keeps_its_own() {
        assert_eq!(weight(11), 0.0);
        assert_eq!(weight(12), 1.0);
        assert!(weight(13) > weight(14) && weight(14) > 0.0);
        assert_eq!(weight(16), 0.0);
        assert_eq!(weight(19), 0.0);
    }
}
