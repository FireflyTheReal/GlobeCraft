//! The shown frame moved to where the camera is now, until the frame drawn for it
//! arrives: input shows on the next screen refresh, not a whole frame later.
//!
//! Between a gesture and its frame on screen lie the request, the drawing, the
//! preparation and the upload: 50–100 ms on an M1, 100–150 ms on a PowerVR phone
//! (Julien, 2026-11-26: "input lag needs to be minimized, especially for phones").
//! Meanwhile the last frame, scaled and shifted so its ground lies under the current
//! camera's, follows the pointer at once. A pan and a zoom are such a move; a turn,
//! a tilt or a look up are not, and leave the frame where it is. With the relief the
//! ground is the drawn one (the walk's, `DepthMap`): the surface below it would move a
//! low camera's frame up to twice as far as its ground.
use emath::{Pos2, Rect, Vec2};
use globecraft_core::camera::Basis;

/// The shown frame's points the move is fitted to: its centre and four around it,
/// as shares of its width and height from the centre.
const SAMPLE_SHARES: [(f32, f32); 5] = [
    (0.0, 0.0),
    (-0.3, -0.3),
    (0.3, -0.3),
    (-0.3, 0.3),
    (0.3, 0.3),
];
/// A fitted move further from a sample's true place than this share of the frame's
/// diagonal is not a pan or a zoom (a turn or a tilt): the frame stays put.
const MAX_MISFIT_SHARE: f32 = 0.01;
/// Zooms between frames past this factor (either way) are left to the next frame.
const MAX_SCALE: f32 = 4.0;

/// A frame's move: each of its points `p` shows at `p * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameShift {
    pub scale: f32,
    pub offset: Vec2,
}

impl FrameShift {
    pub fn apply(self, p: Pos2) -> Pos2 {
        Pos2::new(p.x * self.scale, p.y * self.scale) + self.offset
    }
}

/// How the frame drawn from `shown` (in `rect`) moves to show the ground as `now`
/// sees it, or `None` when the two see the same, or not by a pan and a zoom. With
/// `depth` (the frame's drawn relief) its samples' ground is the drawn one, and a
/// sample without drawn ground is left out.
pub fn frame_shift(
    shown: &Basis,
    now: &Basis,
    rect: Rect,
    depth: Option<&crate::overlays::DepthMap>,
) -> Option<FrameShift> {
    if shown.c == now.c && shown.rotation_from_view() == now.rotation_from_view() {
        return None;
    }
    let geometry = shown.model.geometry();
    let pairs: Vec<(Pos2, Pos2)> = SAMPLE_SHARES
        .iter()
        .filter_map(|&(x, y)| {
            let at = rect.center() + Vec2::new(x * rect.width(), y * rect.height());
            let ray = shown.ray(rect, at);
            let ground = match depth {
                Some(depth) => shown.c + ray * depth.ground_at(rect, at)?,
                None => geometry.intersect(shown.c, ray)?,
            };
            let seen = now.project_precise(ground, rect)?;
            Some((at, Pos2::new(seen.x as f32, seen.y as f32)))
        })
        .collect();
    if pairs.len() < 2 {
        return None;
    }
    let count = pairs.len() as f32;
    let mean = |side: fn(&(Pos2, Pos2)) -> Pos2| {
        pairs
            .iter()
            .fold(Vec2::ZERO, |sum, pair| sum + side(pair).to_vec2())
            / count
    };
    let (from_mean, to_mean) = (mean(|pair| pair.0), mean(|pair| pair.1));
    let (mut along, mut spread) = (0.0, 0.0);
    for (from, to) in &pairs {
        let (from, to) = (from.to_vec2() - from_mean, to.to_vec2() - to_mean);
        along += from.dot(to);
        spread += from.length_sq();
    }
    let scale = along / spread;
    if !scale.is_finite() || !(1.0 / MAX_SCALE..=MAX_SCALE).contains(&scale) {
        return None;
    }
    let shift = FrameShift {
        scale,
        offset: to_mean - from_mean * scale,
    };
    let misfit = MAX_MISFIT_SHARE * rect.size().length();
    pairs
        .iter()
        .all(|(from, to)| shift.apply(*from).distance(*to) <= misfit)
        .then_some(shift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;

    fn basis(cam: Cam) -> Basis {
        cam.basis(1.5)
    }

    const RECT: Rect = Rect {
        min: Pos2::new(0.0, 0.0),
        max: Pos2::new(1200.0, 800.0),
    };

    fn cam(lon: f64, alt: f64, heading: f64, tilt: f64) -> Cam {
        Cam {
            lat: 46.5,
            lon,
            heading,
            alt,
            tilt,
            look_up: 0.0,
        }
    }

    /// A pan moves the frame with the ground: the place at the old centre shows where
    /// the new camera sees it.
    #[test]
    fn a_pan_moves_the_frame_with_the_ground() {
        let shown = basis(cam(8.0, 50.0, 0.0, 0.0));
        let now = basis(cam(8.05, 50.0, 0.0, 0.0));
        let shift = frame_shift(&shown, &now, RECT, None).expect("a pan");
        assert!((shift.scale - 1.0).abs() < 0.01, "{shift:?}");
        assert!(
            shift.offset.x < -10.0 && shift.offset.y.abs() < 5.0,
            "{shift:?}"
        );
        let ground = shown
            .model
            .geometry()
            .intersect(shown.c, shown.ray(RECT, RECT.center()))
            .unwrap();
        let seen = now.project_precise(ground, RECT).unwrap();
        assert!(
            shift
                .apply(RECT.center())
                .distance(Pos2::new(seen.x as f32, seen.y as f32))
                < 1.0
        );
    }

    /// A relief frame moves by its drawn ground: drawn at the surface it moves as the
    /// surface does; without drawn ground it stays.
    #[test]
    fn a_relief_frame_moves_by_its_drawn_ground() {
        let shown = basis(cam(8.0, 50.0, 0.0, 0.0));
        let now = basis(cam(8.05, 50.0, 0.0, 0.0));
        let (width, height) = (120, 80);
        let mut planes = vec![0.0f32; 4 * width * height];
        for y in 0..height {
            for x in 0..width {
                let at = Pos2::new(
                    (x as f32 + 0.5) / width as f32 * RECT.width(),
                    (y as f32 + 0.5) / height as f32 * RECT.height(),
                );
                let ray = shown.ray(RECT, at);
                let ground = shown.model.geometry().intersect(shown.c, ray).unwrap();
                let i = y * width + x;
                planes[width * height + i] = (ground - shown.c).length() as f32;
                planes[3 * width * height + i] = 1.0;
            }
        }
        let surface = crate::overlays::DepthMap::new(width, height, planes.clone().into());
        let drawn = frame_shift(&shown, &now, RECT, surface.as_ref()).expect("drawn ground");
        let plain = frame_shift(&shown, &now, RECT, None).unwrap();
        assert!(
            (drawn.scale - plain.scale).abs() < 0.01,
            "{drawn:?} {plain:?}"
        );
        assert!(
            (drawn.offset - plain.offset).length() < 2.0,
            "{drawn:?} {plain:?}"
        );
        planes[3 * width * height..].fill(-1.0);
        let unsure = crate::overlays::DepthMap::new(width, height, planes.into());
        assert_eq!(frame_shift(&shown, &now, RECT, unsure.as_ref()), None);
    }

    /// Coming closer enlarges the frame; the same view and a turn leave it be.
    #[test]
    fn a_zoom_scales_and_a_turn_stays() {
        let shown = basis(cam(8.0, 50.0, 0.0, 0.0));
        let closer =
            frame_shift(&shown, &basis(cam(8.0, 40.0, 0.0, 0.0)), RECT, None).expect("a zoom");
        assert!(closer.scale > 1.2, "{closer:?}");
        assert_eq!(frame_shift(&shown, &shown, RECT, None), None);
        assert_eq!(
            frame_shift(&shown, &basis(cam(8.0, 50.0, 30.0, 0.0)), RECT, None),
            None
        );
        assert_eq!(
            frame_shift(&shown, &basis(cam(8.0, 50.0, 0.0, 40.0)), RECT, None),
            None
        );
    }
}
