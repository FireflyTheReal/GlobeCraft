//! The aurora oval (roadmap 5, `HAZ-01`): NOAA's OVATION forecast drawn as a green
//! glow, brighter where the chance is higher, only on the night side when Sunlight is
//! on (aurora cannot be seen in daylight). It is a layer just above the clouds
//! (`AURORA_LAYER_KM`): over them seen from above, behind them from below, where it is
//! drawn behind the frame with the stars (user, 2026-11-18). Its 1° cells were drawn
//! only while all their corners were on screen, so close up the glow ended in a
//! rectangle smaller than the view; the screen's own grid has no such edge.
use eframe::egui::{self, Color32, Pos2, Rect, Shape};
use globecraft_core::camera::Basis;
#[cfg(test)]
use globecraft_data::aurora::LONGITUDES;
use globecraft_data::aurora::{AuroraGrid, LATITUDES};
use globecraft_render::clouds::{CLOUD_LAYER_KM, eye_height_km, layer_grid};
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
/// The aurora's layer: just above the clouds'. Real aurora glows 100 to 300 km up;
/// drawn there it would slide far across the ground with the view's tilt.
const AURORA_LAYER_KM: f64 = CLOUD_LAYER_KM + 0.5;
/// A corner of the glow's mesh every this many points: the forecast is a 1° grid,
/// smooth on the screen.
const MESH_STEP_POINTS: f32 = 16.0;
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

/// Whether the eye of `basis` is under the aurora's layer (so it is drawn behind the
/// frame, with the sky).
pub(in crate::app) fn seen_from_below(basis: &Basis) -> bool {
    eye_height_km(basis) < AURORA_LAYER_KM
}

/// How bright the glow is at a node: by chance, then by darkness.
fn alpha(chance: u8, night: f32) -> f32 {
    // From nothing at the cutoff, so the oval's edges fade instead of stepping.
    let floor = f32::from(MIN_CHANCE);
    let t = ((f32::from(chance) - floor) / (FULL_CHANCE - floor)).clamp(0.0, 1.0);
    t.powf(0.8) * MAX_ALPHA * night
}

/// Green, turning yellow-green where it is strongest.
/// The glow's colour at a chance, as on the night side (the legend's key).
pub(in crate::app) fn legend_colour(chance: u8) -> Color32 {
    colour(chance, alpha(chance, 1.0))
}

fn colour(chance: u8, alpha: f32) -> Color32 {
    let t = (f32::from(chance) / 100.0).clamp(0.0, 1.0);
    let (r, g, b) = (40.0 + 150.0 * t, 255.0, 140.0 - 60.0 * t);
    Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, (alpha * 255.0) as u8)
}

fn glow(basis: &Basis, rect: Rect, grid: &AuroraGrid, sun: Option<glam::DVec3>) -> Shape {
    let night = |up: glam::DVec3| -> f32 {
        let Some(sun) = sun else {
            return 1.0;
        };
        let t = ((DAY_COSINE - up.dot(sun)) / (DAY_COSINE - NIGHT_COSINE)).clamp(0.0, 1.0) as f32;
        t * t * (3.0 - 2.0 * t)
    };
    let layer = layer_grid(basis, rect, MESH_STEP_POINTS, AURORA_LAYER_KM);
    let colours: Vec<Option<(Pos2, Color32)>> = layer
        .corners
        .iter()
        .map(|corner| {
            corner.map(|corner| {
                let chance = chance_at(grid, corner.lat, corner.lon);
                let up = globecraft_core::math::ll_to_v3(corner.lat, corner.lon);
                let alpha = alpha(chance, night(up)) * corner.facing;
                (corner.pos, colour(chance, alpha))
            })
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    let row = layer.columns + 1;
    for y in 0..layer.rows {
        for x in 0..layer.columns {
            let corners = [
                y * row + x,
                y * row + x + 1,
                (y + 1) * row + x + 1,
                (y + 1) * row + x,
            ]
            .map(|index| colours[index]);
            let [Some(a), Some(b), Some(c), Some(d)] = corners else {
                continue;
            };
            if [a, b, c, d].iter().all(|(_, colour)| colour.a() == 0) {
                continue;
            }
            let base = mesh.vertices.len() as u32;
            for (at, colour) in [a, b, c, d] {
                mesh.colored_vertex(at, colour);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
    }
    Shape::mesh(mesh)
}

/// The forecast's chance (%) at a place, between its four nearest grid nodes.
fn chance_at(grid: &AuroraGrid, lat: f64, lon: f64) -> u8 {
    let x = lon.rem_euclid(360.0);
    let y = (lat + 90.0).clamp(0.0, (LATITUDES - 1) as f64);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: usize, dy: usize| f64::from(grid.value(x0 as usize + dx, y0 as usize + dy));
    let top = at(0, 0) * (1.0 - fx) + at(1, 0) * fx;
    let bottom = at(0, 1) * (1.0 - fx) + at(1, 1) * fx;
    (top * (1.0 - fy) + bottom * fy).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chance_runs_smoothly_between_grid_nodes_and_across_the_date_line() {
        let mut rows = Vec::new();
        for lon in 0..LONGITUDES {
            for lat in -90..=90 {
                // 0 % west of the date line's node, 80 % on it, 40 % at 1° E.
                let chance = match lon {
                    0 => 80,
                    1 => 40,
                    _ => 0,
                };
                rows.push(serde_json::json!([
                    lon,
                    lat,
                    if lat == 60 { chance } else { 0 }
                ]));
            }
        }
        let grid = AuroraGrid::parse(&serde_json::json!({
            "Observation Time": "2026-11-04T04:00:00Z",
            "Forecast Time": "2026-11-04T04:00:00Z",
            "coordinates": rows,
        }))
        .unwrap();
        assert_eq!(chance_at(&grid, 60.0, 0.0), 80);
        assert_eq!(chance_at(&grid, 60.0, 0.5), 60);
        assert_eq!(chance_at(&grid, 60.0, -0.5), 40);
        assert_eq!(chance_at(&grid, 60.5, 0.0), 40);
        assert_eq!(chance_at(&grid, 90.0, 0.0), 0);
    }

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
