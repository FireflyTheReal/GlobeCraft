//! The naked-eye stars: the Yale Bright Star Catalogue (`assets/stars`), placed in
//! the sky for a moment by the same celestial frame as the Milky Way. Until
//! 2026-11-18 the stars were made up (one in 1,400 cells of a grid) and fixed to the
//! Earth, so they turned with it instead of with the sky (user: "are the stars
//! accurate?").
//!
//! Stars are drawn as dots behind the frame, at the picture's full resolution: the
//! app's sky layer draws them on screen, `paint_behind` into screenshots. The sun is
//! drawn in the frame itself, through the air (`sun`).
use emath::{Pos2, Rect};
use glam::{DVec3, Vec3};
use globecraft_core::{VERTICAL_FOV_HALF_TAN, camera::Basis, celestial::CelestialFrame};
use std::sync::LazyLock;

/// One star of the catalogue.
struct Star {
    /// J2000 (ICRS) unit direction.
    direction: DVec3,
    /// Visual magnitude.
    magnitude: f32,
    /// Its colour (0 to 1), from the spectral class.
    colour: Vec3,
}

/// Bytes per star in `bsc5.bin` (see `assets/stars/convert_bsc5.py`).
const STAR_BYTES: usize = 8;

static CATALOGUE: LazyLock<Vec<Star>> = LazyLock::new(|| {
    include_bytes!("../assets/stars/bsc5.bin")
        .chunks_exact(STAR_BYTES)
        .map(|entry| {
            let ra = f64::from(u16::from_le_bytes([entry[0], entry[1]])) / 65_536.0
                * std::f64::consts::TAU;
            let dec = f64::from(i16::from_le_bytes([entry[2], entry[3]])) / 32_767.0
                * std::f64::consts::FRAC_PI_2;
            Star {
                direction: DVec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()),
                magnitude: f32::from(i16::from_le_bytes([entry[4], entry[5]])) / 100.0,
                colour: class_colour(entry[6]),
            }
        })
        .collect()
});

/// Stars' colours by spectral class, hot blue to cool red (approximate sRGB of
/// black bodies at each class's typical temperature, scaled to the brightest
/// channel).
fn class_colour(class: u8) -> Vec3 {
    match class {
        b'O' => Vec3::new(0.66, 0.75, 1.0),
        b'B' => Vec3::new(0.74, 0.82, 1.0),
        b'A' => Vec3::new(0.93, 0.95, 1.0),
        b'F' => Vec3::new(1.0, 0.98, 0.93),
        b'G' => Vec3::new(1.0, 0.93, 0.82),
        b'K' => Vec3::new(1.0, 0.83, 0.64),
        b'M' | b'C' | b'S' | b'N' => Vec3::new(1.0, 0.74, 0.52),
        _ => Vec3::ONE,
    }
}

/// Stars this bright (magnitude) or brighter show at full light; fainter ones dim
/// with their flux to the power `LIGHT_GAMMA` (roughly as the eye sees it), down to
/// `MIN_LIGHT` at the catalogue's end (about 6.5).
const FULL_LIGHT_MAGNITUDE: f32 = 1.0;
const LIGHT_GAMMA: f32 = 0.45;
const MIN_LIGHT: f32 = 0.08;
/// Stars brighter than these magnitudes are drawn half again and twice as wide.
const BRIGHT_MAGNITUDE: f32 = 1.5;
const BRIGHTEST_MAGNITUDE: f32 = 0.0;

/// A star where it shows in a picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarDot {
    /// Its centre, in pixels from the picture's top left.
    pub centre: Pos2,
    /// Its width, in pixels.
    pub size: f32,
    /// The light it adds (0 to 1 per channel, as drawn).
    pub light: [f32; 3],
}

/// The stars a camera at `basis` sees at `frame`'s moment, in a `width` × `height`
/// pixel picture. `pixel_scale` is how many of its pixels make a screen point: the
/// dots grow with it.
pub fn seen(
    basis: &Basis,
    frame: CelestialFrame,
    width: f64,
    height: f64,
    pixel_scale: f64,
) -> Vec<StarDot> {
    let view_from_world = basis.rotation_from_view().transpose();
    let base = pixel_scale.max(1.0) as f32;
    let margin = 2.0 * base as f64;
    CATALOGUE
        .iter()
        .filter_map(|star| {
            let local = view_from_world * frame.earth_fixed(star.direction);
            let at = project(basis, local, width, height)?;
            let (x, y) = (f64::from(at.x), f64::from(at.y));
            if !(-margin..width + margin).contains(&x) || !(-margin..height + margin).contains(&y) {
                return None;
            }
            let size = base
                * if star.magnitude < BRIGHTEST_MAGNITUDE {
                    2.0
                } else if star.magnitude < BRIGHT_MAGNITUDE {
                    1.5
                } else {
                    1.0
                };
            Some(StarDot {
                centre: at,
                size,
                light: (star.colour * light(star.magnitude)).to_array(),
            })
        })
        .collect()
}

/// Where a direction `local` in the camera's frame lands in a `width` × `height`
/// pixel picture; `None` behind the camera.
fn project(basis: &Basis, local: DVec3, width: f64, height: f64) -> Option<Pos2> {
    if local.z >= 0.0 {
        return None;
    }
    let (tan_x, tan_y) = (
        VERTICAL_FOV_HALF_TAN * basis.aspect_ratio(),
        VERTICAL_FOV_HALF_TAN,
    );
    Some(Pos2::new(
        ((1.0 + local.x / -local.z / tan_x) * 0.5 * width) as f32,
        ((1.0 - local.y / -local.z / tan_y) * 0.5 * height) as f32,
    ))
}

/// How bright a star of `magnitude` is drawn, 0 to 1.
fn light(magnitude: f32) -> f32 {
    let flux = 10f32.powf(-0.4 * (magnitude - FULL_LIGHT_MAGNITUDE));
    flux.powf(LIGHT_GAMMA).clamp(MIN_LIGHT, 1.0)
}

/// Paints the sky behind rows `rows` of a `width` × `height` picture rendered with
/// `RenderCfg::sky_behind` (premultiplied RGBA, the strip's rows only): the Milky
/// Way or empty space, and the stars, under what the frame left open. Screenshots
/// use it; on screen the app's sky layer does the same.
pub(crate) fn paint_behind(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    rows: std::ops::Range<usize>,
    basis: &Basis,
    cfg: &crate::RenderCfg,
) {
    let picture = Rect::from_min_size(Pos2::ZERO, emath::vec2(width as f32, height as f32));
    let space = Vec3::from(crate::space_color());
    // The stars' light over the strip, then the sky under each open pixel.
    let mut light = vec![Vec3::ZERO; width * rows.len()];
    if let Some(frame) = cfg.stars {
        let scale = height as f64 / STILL_POINTS_HIGH;
        for dot in seen(basis, frame, width as f64, height as f64, scale) {
            add_dot(&mut light, width, rows.clone(), dot);
        }
    }
    for (index, pixel) in pixels.chunks_exact_mut(crate::RGBA_CHANNELS).enumerate() {
        let open = 1.0 - f32::from(pixel[3]) / 255.0;
        if open <= 0.0 {
            continue;
        }
        let (x, y) = (index % width, rows.start + index / width);
        let ray = basis.ray(picture, Pos2::new(x as f32 + 0.5, y as f32 + 0.5));
        let behind = match &cfg.sky {
            Some(sky) => sky.sample(ray).truncate(),
            None => space,
        } + light[index];
        for (channel, value) in pixel[..3].iter_mut().zip(behind.to_array()) {
            let under = value.clamp(0.0, 1.0) * open * 255.0;
            *channel = (f32::from(*channel) + under).min(255.0) as u8;
        }
        pixel[3] = 255;
    }
}

/// A screenshot's star dots are sized as on a screen this many points high.
const STILL_POINTS_HIGH: f64 = 900.0;

/// Adds `dot`'s light to the strip's light buffer: a disc, its edge pixels by how
/// much of them it covers (sampled 4 × 4).
fn add_dot(light: &mut [Vec3], width: usize, rows: std::ops::Range<usize>, dot: StarDot) {
    const SAMPLES: usize = 4;
    let radius = dot.size * 0.5;
    let colour = Vec3::from(dot.light);
    let x0 = (dot.centre.x - radius).floor().max(0.0) as usize;
    let x1 = ((dot.centre.x + radius).ceil() as usize).min(width);
    let y0 = ((dot.centre.y - radius).floor().max(0.0) as usize).max(rows.start);
    let y1 = ((dot.centre.y + radius).ceil() as usize).min(rows.end);
    for y in y0..y1 {
        for x in x0..x1 {
            let mut covered = 0;
            for s in 0..SAMPLES * SAMPLES {
                let px = x as f32 + ((s % SAMPLES) as f32 + 0.5) / SAMPLES as f32;
                let py = y as f32 + ((s / SAMPLES) as f32 + 0.5) / SAMPLES as f32;
                covered += usize::from(dot.centre.distance(Pos2::new(px, py)) <= radius);
            }
            light[(y - rows.start) * width + x] +=
                colour * (covered as f32 / (SAMPLES * SAMPLES) as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;

    #[test]
    fn the_catalogue_holds_the_naked_eye_sky() {
        assert_eq!(CATALOGUE.len(), 9_096);
        // Sirius: RA 6h45m09s, Dec −16°42′58″, V −1.46, class A.
        let sirius = CATALOGUE
            .iter()
            .min_by(|a, b| a.magnitude.total_cmp(&b.magnitude))
            .unwrap();
        assert_eq!(sirius.magnitude, -1.46);
        let (ra, dec) = (6.7525f64 * 15.0, -16.7161f64);
        let expected = globecraft_core::math::ll_to_v3(dec, ra);
        assert!(sirius.direction.angle_between(expected).to_degrees() < 0.02);
        assert_eq!(light(-1.46), 1.0);
        assert!(light(6.5) < 0.11);
    }

    #[test]
    fn polaris_stands_over_the_north_pole_at_any_hour() {
        let polaris = CATALOGUE
            .iter()
            .filter(|star| star.magnitude < 2.1)
            .max_by(|a, b| a.direction.z.total_cmp(&b.direction.z))
            .unwrap();
        for hour in 0..4 {
            let frame =
                CelestialFrame::at_unix(1_763_000_000.0 + f64::from(hour) * 21_600.0).unwrap();
            // Within a degree of the Earth's axis, whatever the Earth's turn.
            assert!(frame.earth_fixed(polaris.direction).z > 0.9998);
        }
    }

    #[test]
    fn screenshots_paint_space_and_stars_only_where_the_frame_is_open() {
        let cam = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 30_000.0,
            tilt: 0.0,
            look_up: 0.0,
        };
        let (width, height) = (300, 200);
        let basis = cam.basis(1.5);
        let mut cfg =
            crate::tests::batch_reference_config(globecraft_data::tiles::Provider::Satellite);
        cfg.stars = Some(CelestialFrame::at_unix(1_763_000_000.0).unwrap());
        // The top half open, the bottom half covered by the frame.
        let mut pixels = vec![0u8; width * height * crate::RGBA_CHANNELS];
        for pixel in pixels[width * height * 2..].chunks_exact_mut(crate::RGBA_CHANNELS) {
            pixel.copy_from_slice(&[10, 20, 30, 255]);
        }
        let covered = pixels[width * height * 2..].to_vec();
        for rows in [0..120, 120..200] {
            let range = rows.start * width * 4..rows.end * width * 4;
            paint_behind(&mut pixels[range], width, height, rows, &basis, &cfg);
        }
        assert_eq!(&pixels[width * height * 2..], &covered[..]);
        let top = &pixels[..width * height * 2];
        assert!(top.chunks_exact(4).all(|pixel| pixel[3] == 255));
        // Space is near black; a few pixels carry stars.
        let lit = top.chunks_exact(4).filter(|pixel| pixel[0] > 40).count();
        assert!((1..500).contains(&lit), "{lit}");
    }

    #[test]
    fn stars_land_on_the_picture_where_the_camera_looks() {
        // From space looking straight at the north celestial pole region.
        let cam = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 30_000.0,
            tilt: 0.0,
            look_up: 0.0,
        };
        let basis = cam.basis(1.5);
        let frame = CelestialFrame::at_unix(1_763_000_000.0).unwrap();
        let (width, height) = (1500.0, 1000.0);
        let dots = seen(&basis, frame, width, height, 1.0);
        // Looking down at the Earth from 30,000 km, the sky around it fills the
        // edges: some hundreds of stars, every one on its ray.
        assert!(dots.len() > 50, "{}", dots.len());
        let picture = Rect::from_min_size(Pos2::ZERO, emath::vec2(width as f32, height as f32));
        for dot in dots.iter().take(40) {
            let ray = basis.ray(picture, dot.centre);
            let nearest = CATALOGUE
                .iter()
                .map(|star| frame.earth_fixed(star.direction).angle_between(ray))
                .fold(f64::MAX, f64::min);
            assert!(nearest < 2e-3, "{nearest}");
        }
    }
}
