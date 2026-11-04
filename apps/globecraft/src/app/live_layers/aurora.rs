//! The aurora oval (roadmap 5, `HAZ-01`): NOAA's OVATION forecast drawn as a green
//! glow over the ground, brighter where the chance is higher, only on the night side
//! when Sunlight is on (aurora cannot be seen in daylight).
use eframe::egui::{self, Color32, Pos2, Rect, Shape};
use globecraft_core::camera::Basis;
use globecraft_data::aurora::{AuroraGrid, LATITUDES, LONGITUDES};
use std::sync::Arc;

/// Below this chance (%) a grid node adds nothing.
const MIN_CHANCE: u8 = 4;
/// The chance at which the glow is at its brightest.
const FULL_CHANCE: f32 = 60.0;
/// The brightest glow's opacity.
const MAX_ALPHA: f32 = 0.7;
/// The glow fades in between these sun heights (cosine of the sun's zenith angle):
/// none in daylight, full once the sun is well below the horizon.
const DAY_COSINE: f64 = 0.05;
const NIGHT_COSINE: f64 = -0.12;
/// The forecast shows while the app's time is this close to its moment.
pub(in crate::app) const SHOWN_WITHIN_SECS: f64 = 3.0 * 3600.0;
/// The sun's position is taken to the nearest this many seconds for the cache.
const SUN_STEP_SECS: f64 = 600.0;

/// What one drawing was made from.
#[derive(PartialEq)]
struct Key {
    basis: Basis,
    rect: Rect,
    grid: usize,
    sun_step: Option<i64>,
}

#[derive(Default)]
pub(in crate::app) struct AuroraShapes {
    built: Option<(Key, Shape)>,
}

impl AuroraShapes {
    /// Paints the glow for `grid` at the app's time `scene` (seconds since 1970);
    /// `sunlight` lights the day side, which hides the glow there.
    pub(in crate::app) fn paint(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        grid: &Arc<AuroraGrid>,
        scene: f64,
        sunlight: bool,
    ) {
        let sun_step = sunlight.then(|| (scene / SUN_STEP_SECS).round() as i64);
        let key = Key {
            basis: *basis,
            rect,
            grid: Arc::as_ptr(grid) as usize,
            sun_step,
        };
        if self.built.as_ref().is_none_or(|(built, _)| *built != key) {
            let sun = sun_step.map(|step| {
                let (lat, lon) = globecraft_core::math::subsolar(step as f64 * SUN_STEP_SECS);
                globecraft_core::math::ll_to_v3(lat, lon)
            });
            let shape = glow(basis, rect, grid, sun);
            self.built = Some((key, shape));
        }
        if let Some((_, shape)) = &self.built {
            painter.add(shape.clone());
        }
    }
}

/// How bright the glow is at a node: by chance, then by darkness.
fn alpha(chance: u8, night: f32) -> f32 {
    // From nothing at the cutoff, so the oval's edges fade instead of stepping.
    let floor = f32::from(MIN_CHANCE);
    let t = ((f32::from(chance) - floor) / (FULL_CHANCE - floor)).clamp(0.0, 1.0);
    t.powf(0.8) * MAX_ALPHA * night
}

/// Green, turning yellow-green where it is strongest.
fn colour(chance: u8, alpha: f32) -> Color32 {
    let t = (f32::from(chance) / 100.0).clamp(0.0, 1.0);
    let (r, g, b) = (40.0 + 150.0 * t, 255.0, 140.0 - 60.0 * t);
    Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, (alpha * 255.0) as u8)
}

fn glow(basis: &Basis, rect: Rect, grid: &AuroraGrid, sun: Option<glam::DVec3>) -> Shape {
    let clip = rect.expand(64.0);
    // Each node's place on screen, worked out once (`None` inside: hidden or off).
    let mut nodes: Vec<Option<Option<Pos2>>> = vec![None; LONGITUDES * LATITUDES];
    let mut place = |lon: usize, lat: usize| -> Option<Pos2> {
        let index = (lon % LONGITUDES) * LATITUDES + lat;
        *nodes[index].get_or_insert_with(|| {
            globecraft_render::overlays::project_unhidden(
                basis,
                rect,
                None,
                None,
                lat as f64 - 90.0,
                lon as f64,
            )
            .map(|at| Pos2::new(at.x as f32, at.y as f32))
            .filter(|at| clip.contains(*at))
        })
    };
    let night = |lon: usize, lat: usize| -> f32 {
        let Some(sun) = sun else {
            return 1.0;
        };
        let cosine = globecraft_core::math::ll_to_v3(lat as f64 - 90.0, lon as f64).dot(sun);
        let t = ((DAY_COSINE - cosine) / (DAY_COSINE - NIGHT_COSINE)).clamp(0.0, 1.0) as f32;
        t * t * (3.0 - 2.0 * t)
    };
    let mut mesh = egui::Mesh::default();
    for lon in 0..LONGITUDES {
        for lat in 0..LATITUDES - 1 {
            let corners = [
                (lon, lat),
                (lon + 1, lat),
                (lon + 1, lat + 1),
                (lon, lat + 1),
            ];
            if corners.iter().all(|&(x, y)| grid.value(x, y) < MIN_CHANCE) {
                continue;
            }
            let mut points = [Pos2::ZERO; 4];
            let mut colours = [Color32::TRANSPARENT; 4];
            let mut visible = true;
            for (k, &(x, y)) in corners.iter().enumerate() {
                let Some(at) = place(x, y) else {
                    visible = false;
                    break;
                };
                let chance = grid.value(x, y);
                points[k] = at;
                colours[k] = colour(chance, alpha(chance, night(x, y)));
            }
            if !visible || colours.iter().all(|c| c.a() == 0) {
                continue;
            }
            let base = mesh.vertices.len() as u32;
            for (at, colour) in points.into_iter().zip(colours) {
                mesh.colored_vertex(at, colour);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glow_grows_with_the_chance_and_the_dark() {
        assert_eq!(alpha(0, 1.0), 0.0);
        assert_eq!(alpha(MIN_CHANCE, 1.0), 0.0);
        assert!(alpha(20, 1.0) < alpha(50, 1.0));
        assert_eq!(alpha(80, 1.0), MAX_ALPHA);
        assert_eq!(alpha(80, 0.0), 0.0);
        assert!(alpha(80, 0.5) < alpha(80, 1.0));
    }
}
