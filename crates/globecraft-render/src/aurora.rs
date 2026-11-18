//! The aurora as light in the air (roadmap 5, `HAZ-01`): NOAA's OVATION forecast glows
//! in a shell of air `AURORA_LOW_KM` to `AURORA_HIGH_KM` up, where real aurora is
//! brightest. Each ray adds the glow along its path through that shell, so the aurora
//! is faint looking straight down, a bright band above the limb seen edge-on, and a
//! glow in the sky from the ground (user, 2026-11-18: "rather fused with the
//! atmosphere"). It was a flat mesh drawn over the frame, cut into steps at the limb.
//! The GPU shader does the same sums (`overview.wgsl`, `aurora_glow`).
use glam::{DVec3, Vec3};
use globecraft_core::EARTH_RADIUS_KM;
use globecraft_data::aurora::{AuroraGrid, LATITUDES};
use std::sync::Arc;

/// The aurora for the renderer, while its layer shows and the forecast is current.
#[derive(Clone)]
pub struct AuroraCfg {
    pub grid: Arc<AuroraGrid>,
    /// The layer's opacity (0 to 1).
    pub opacity: f32,
}

/// The glowing shell's bottom and top above the surface (km): the green oxygen line
/// shines most between them.
pub(crate) const AURORA_LOW_KM: f32 = 90.0;
pub(crate) const AURORA_HIGH_KM: f32 = 170.0;
/// Below this chance (%) the forecast adds nothing; at the second the glow is full.
pub(crate) const MIN_CHANCE: f32 = 4.0;
pub(crate) const FULL_CHANCE: f32 = 60.0;
/// How strongly a km of fully glowing air shines (before `1 − e^−x`): straight down
/// through the shell about half the screen's range, edge-on many times it.
pub(crate) const GLOW_PER_KM: f32 = 0.008;
/// Long paths through the shell (edge-on, or from the ground toward the horizon) shine
/// at most about this much: the glow is compressed toward it, not cut. From the ground
/// the whole sky turned a flat bright green.
pub(crate) const MAX_LIGHT: f32 = 0.8;
/// The aurora's colour: oxygen's green.
pub(crate) const AURORA_COLOUR: [f32; 3] = [0.25, 1.0, 0.5];
/// The glow fades in between these sun heights where it shines (cosine of the sun's
/// zenith angle): none in daylight, full once the sun is well below the horizon.
pub(crate) const DAY_COSINE: f32 = 0.05;
pub(crate) const NIGHT_COSINE: f32 = -0.12;

/// The aurora's light (0 to 1 per colour, to be screened over what is behind) along a
/// ray from `camera` (Earth radii) in `direction` (unit) up to `end` (Earth radii;
/// `f64::INFINITY` for the sky), with the sun toward `sun` if shown.
pub(crate) fn glow(
    aurora: &AuroraCfg,
    sun: Option<DVec3>,
    camera: DVec3,
    direction: DVec3,
    end: f64,
) -> Vec3 {
    let shine = |from: f64, to: f64| -> f32 {
        let (from, to) = (from.max(0.0), to.min(end));
        if to <= from {
            return 0.0;
        }
        // The forecast where the ray is halfway through this stretch.
        let middle = (camera + direction * (0.5 * (from + to))).normalize();
        let night = sun.map_or(1.0, |sun| {
            let t = ((DAY_COSINE - middle.dot(sun) as f32) / (DAY_COSINE - NIGHT_COSINE))
                .clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        });
        if night <= 0.0 {
            return 0.0;
        }
        let (lat, lon) = (
            middle.z.clamp(-1.0, 1.0).asin().to_degrees(),
            middle.y.atan2(middle.x).to_degrees(),
        );
        strength(chance_at(&aurora.grid, lat, lon)) * night * ((to - from) * EARTH_RADIUS_KM) as f32
    };
    let (outer, inner) = (
        sphere(camera, direction, AURORA_HIGH_KM),
        sphere(camera, direction, AURORA_LOW_KM),
    );
    let Some((enter, leave)) = outer else {
        return Vec3::ZERO;
    };
    // The stretches between the two spheres: through the shell's near side and, past
    // the inner sphere (or edge-on, missing it), its far side.
    let km = match inner {
        Some((down, up)) => shine(enter, down) + shine(up, leave),
        None => shine(enter, leave),
    };
    let light = soften(km * GLOW_PER_KM * aurora.opacity);
    Vec3::from(AURORA_COLOUR).map(|colour| 1.0 - (-colour * light).exp())
}

/// The glow of the forecast's `chance` (%) seen straight down through the shell at
/// night, as drawn over black (0 to 1): the legend's key.
pub fn straight_down(chance: f32) -> [f32; 3] {
    let light = soften(strength(chance) * GLOW_PER_KM * (AURORA_HIGH_KM - AURORA_LOW_KM));
    AURORA_COLOUR.map(|colour| 1.0 - (-colour * light).exp())
}

/// The glow compressed toward `MAX_LIGHT`.
fn soften(light: f32) -> f32 {
    light / (1.0 + light / MAX_LIGHT)
}

/// Where a ray meets the sphere `km` above a unit Earth: its distances in and out.
fn sphere(camera: DVec3, direction: DVec3, km: f32) -> Option<(f64, f64)> {
    let radius = 1.0 + f64::from(km) / EARTH_RADIUS_KM;
    let b = camera.dot(direction);
    let disc = b * b - (camera.length_squared() - radius * radius);
    (disc > 0.0).then(|| (-b - disc.sqrt(), -b + disc.sqrt()))
}

/// How much the forecast's chance (%) glows, 0 to 1.
fn strength(chance: f32) -> f32 {
    ((chance - MIN_CHANCE) / (FULL_CHANCE - MIN_CHANCE))
        .clamp(0.0, 1.0)
        .powf(0.8)
}

/// The forecast's chance (%) at a place, between its four nearest grid nodes.
pub(crate) fn chance_at(grid: &AuroraGrid, lat: f64, lon: f64) -> f32 {
    let x = lon.rem_euclid(360.0);
    let y = (lat + 90.0).clamp(0.0, (LATITUDES - 1) as f64);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let at = |dx: usize, dy: usize| f32::from(grid.value(x0 as usize + dx, y0 as usize + dy));
    let top = at(0, 0) * (1.0 - fx) + at(1, 0) * fx;
    let bottom = at(0, 1) * (1.0 - fx) + at(1, 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_data::aurora::LONGITUDES;

    /// A forecast of `chance` everywhere north of 60° N.
    fn north(chance: u8) -> AuroraCfg {
        let mut rows = Vec::new();
        for lon in 0..LONGITUDES {
            for lat in -90..=90 {
                rows.push(serde_json::json!([
                    lon,
                    lat,
                    if lat >= 60 { chance } else { 0 }
                ]));
            }
        }
        let grid = AuroraGrid::parse(&serde_json::json!({
            "Observation Time": "2026-11-04T04:00:00Z",
            "Forecast Time": "2026-11-04T04:00:00Z",
            "coordinates": rows,
        }))
        .unwrap();
        AuroraCfg {
            grid: Arc::new(grid),
            opacity: 1.0,
        }
    }

    #[test]
    fn the_chance_runs_smoothly_between_grid_nodes() {
        let aurora = north(80);
        assert_eq!(chance_at(&aurora.grid, 70.0, 10.0), 80.0);
        assert_eq!(chance_at(&aurora.grid, 59.5, 10.0), 40.0);
        assert_eq!(chance_at(&aurora.grid, 30.0, -170.0), 0.0);
        assert_eq!(chance_at(&aurora.grid, 70.0, -0.5), 80.0);
    }

    #[test]
    fn faint_looking_down_bright_edge_on_and_none_by_day() {
        let aurora = north(80);
        let over = DVec3::Z;
        // From 1,000 km straight down over the pole to the ground.
        let camera = over * (1.0 + 1000.0 / EARTH_RADIUS_KM);
        let down = glow(&aurora, None, camera, -over, camera.length() - 1.0);
        assert!(down.y > 0.2 && down.y < 0.6, "{down}");
        // Edge-on, a ray grazing the shell over the pole: far brighter.
        let side = DVec3::new(-20.0, 0.0, 1.0 + 130.0 / EARTH_RADIUS_KM);
        let edge = glow(&aurora, None, side, DVec3::X, f64::INFINITY);
        assert!(edge.y > 0.5 && edge.y > down.y, "{edge}");
        // Never a flat full green.
        assert!(edge.y < 1.0 - (-MAX_LIGHT).exp() + 1e-6, "{edge}");
        // Green above all; nothing outside the oval or in daylight.
        assert!(edge.y > edge.x && edge.y > edge.z);
        let equator = DVec3::X * (1.0 + 1000.0 / EARTH_RADIUS_KM);
        assert_eq!(glow(&aurora, None, equator, -DVec3::X, 0.15), Vec3::ZERO);
        assert_eq!(glow(&aurora, Some(over), camera, -over, 0.15), Vec3::ZERO);
        // From the ground, looking up into it.
        let ground = over * 1.000_001;
        assert!(glow(&aurora, None, ground, over, f64::INFINITY).y > 0.2);
    }
}
