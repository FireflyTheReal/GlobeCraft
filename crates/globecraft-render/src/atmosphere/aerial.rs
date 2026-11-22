//! The air between a camera inside it and the ground it sees (aerial perspective),
//! integrated along the view rays once per frame into a small table, as Hillaire's
//! "A Scalable and Production Ready Sky and Atmosphere Rendering Technique" (2020)
//! does with its aerial-perspective volume. The scattering table's way (all the air
//! along the ray, less what lies beyond the point) cancels two large values for rays
//! that skim the horizon: distant ground there showed a bright band with streaks
//! through it, strongest at twilight (Julien's screenshots of 2026-11-22, Mt Odake and
//! the Alps).
use super::{
    GROUND_KM, MIE_EXTINCTION_PER_KM, MIE_SCATTERING_PER_KM, OZONE_ABSORPTION_PER_KM,
    RAYLEIGH_SCATTERING_PER_KM, Tables, along, densities, phases,
};
use glam::{Vec3, Vec4};
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

/// View cosines (closest round the horizon), azimuths from the sun, distances.
const VIEWS: usize = 64;
const AZIMUTHS: usize = 16;
const DISTANCES: usize = 32;
/// Steps between two distances.
const SUBSTEPS: usize = 4;
/// The table reaches this far past the camera's sea-level horizon: mountains beyond
/// it still show.
const BEYOND_HORIZON_KM: f32 = 250.0;
const MIN_REACH_KM: f32 = 50.0;

/// One frame's table: per view cosine, azimuth and distance, the light the air
/// scatters toward the camera up to there and the share of the light from there that
/// arrives. Both final (phase functions applied), before exposure.
pub(super) struct Aerial {
    radius_km: f32,
    sun_cosine: f32,
    horizon: f32,
    reach_km: f32,
    cells: Vec<(Vec4, Vec4)>,
}

/// The last table made, kept while the camera's height and the sun stay the same.
static LAST: Mutex<Option<Arc<Aerial>>> = Mutex::new(None);

impl Aerial {
    /// The table for a camera `radius_km` from the centre with the sun's cosine
    /// `sun_cosine` there: the last one when that is unchanged.
    pub(super) fn at(tables: &Tables, radius_km: f32, sun_cosine: f32) -> Arc<Self> {
        let mut last = LAST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(table) = last.as_ref()
            && (table.radius_km - radius_km).abs() < 1e-3
            && (table.sun_cosine - sun_cosine).abs() < 1e-5
        {
            return Arc::clone(table);
        }
        let table = Arc::new(Self::new(tables, radius_km, sun_cosine));
        *last = Some(Arc::clone(&table));
        table
    }

    fn new(tables: &Tables, radius_km: f32, sun_cosine: f32) -> Self {
        let r = radius_km;
        let horizon = -(1.0 - (GROUND_KM / r).powi(2)).max(0.0).sqrt();
        let reach_km =
            ((r * r - GROUND_KM * GROUND_KM).max(0.0).sqrt() + BEYOND_HORIZON_KM).max(MIN_REACH_KM);
        let mut table = Self {
            radius_km,
            sun_cosine,
            horizon,
            reach_km,
            cells: Vec::new(),
        };
        let rayleigh = Vec3::from(RAYLEIGH_SCATTERING_PER_KM);
        let ozone = Vec3::from(OZONE_ABSORPTION_PER_KM);
        let sun_across = (1.0 - sun_cosine * sun_cosine).max(0.0).sqrt();
        table.cells = (0..VIEWS * AZIMUTHS)
            .into_par_iter()
            .flat_map_iter(|ray| {
                let mu = table.view_cosine(ray / AZIMUTHS);
                let azimuth = 2.0 * (ray % AZIMUTHS) as f32 / (AZIMUTHS - 1) as f32 - 1.0;
                let nu = mu * sun_cosine + (1.0 - mu * mu).max(0.0).sqrt() * sun_across * azimuth;
                let (air_phase, haze_phase) = phases(nu);
                let (mut light, mut crossed) = (Vec3::ZERO, Vec3::ONE);
                let mut cells = Vec::with_capacity(DISTANCES);
                cells.push((Vec4::ZERO, Vec4::ONE));
                for slice in 1..DISTANCES {
                    let (from, to) = (table.distance(slice - 1), table.distance(slice));
                    let step = (to - from) / SUBSTEPS as f32;
                    for sub in 0..SUBSTEPS {
                        let d = from + (sub as f32 + 0.5) * step;
                        let (r_d, _, mu_s_d) = along(r, mu, sun_cosine, nu, d);
                        let (air, haze, ozone_density) = densities(r_d - GROUND_KM);
                        let air_scattering = rayleigh * air;
                        let haze_scattering = MIE_SCATTERING_PER_KM * haze;
                        let extinction = air_scattering
                            + Vec3::splat(MIE_EXTINCTION_PER_KM * haze)
                            + ozone * ozone_density;
                        let sun = tables.to_sun(r_d, mu_s_d).truncate();
                        let multiple = tables.multiple(r_d, mu_s_d).truncate();
                        let source = (air_scattering * air_phase
                            + Vec3::splat(haze_scattering * haze_phase))
                            * sun
                            + (air_scattering + Vec3::splat(haze_scattering)) * multiple;
                        // The step's light integrated exactly over its extinction.
                        let through = (-extinction * step).exp();
                        let kept = (Vec3::ONE - through) / extinction.max(Vec3::splat(1e-9));
                        light += crossed * source * kept;
                        crossed *= through;
                    }
                    cells.push((Vec4::from((light, 0.0)), Vec4::from((crossed, 1.0))));
                }
                cells
            })
            .collect();
        table
    }

    /// The view cosine of row `index`: rows crowd round the camera's horizon.
    fn view_cosine(&self, index: usize) -> f32 {
        let horizon = self.horizon.asin();
        let v = index as f32 / (VIEWS - 1) as f32 * 2.0 - 1.0;
        let side = if v < 0.0 {
            -std::f32::consts::FRAC_PI_2 - horizon
        } else {
            std::f32::consts::FRAC_PI_2 - horizon
        };
        (horizon + v * v * side.abs() * v.signum()).sin()
    }
    /// The (fractional) row of view cosine `mu`.
    fn view_row(&self, mu: f32) -> f32 {
        let horizon = self.horizon.asin();
        let angle = mu.clamp(-1.0, 1.0).asin() - horizon;
        let side = if angle < 0.0 {
            std::f32::consts::FRAC_PI_2 + horizon
        } else {
            std::f32::consts::FRAC_PI_2 - horizon
        };
        let v = angle.signum() * (angle.abs() / side).sqrt();
        (v + 1.0) * 0.5 * (VIEWS - 1) as f32
    }
    /// How far slice `index` lies (km): slices crowd near the camera.
    fn distance(&self, index: usize) -> f32 {
        let share = index as f32 / (DISTANCES - 1) as f32;
        self.reach_km * share * share
    }

    /// The air's light and transmittance for a ray at view cosine `mu` and cosine `nu`
    /// to the sun, to `distance_km`.
    pub(super) fn sample(&self, mu: f32, nu: f32, distance_km: f32) -> (Vec4, Vec4) {
        let sun_across = (1.0 - self.sun_cosine * self.sun_cosine).max(0.0).sqrt();
        let across = (1.0 - mu * mu).max(0.0).sqrt() * sun_across;
        let azimuth = if across > 1e-6 {
            ((nu - mu * self.sun_cosine) / across).clamp(-1.0, 1.0)
        } else {
            1.0
        };
        let row = self.view_row(mu).clamp(0.0, (VIEWS - 1) as f32);
        let column =
            ((azimuth + 1.0) * 0.5 * (AZIMUTHS - 1) as f32).clamp(0.0, (AZIMUTHS - 1) as f32);
        let slice = ((distance_km / self.reach_km).clamp(0.0, 1.0).sqrt() * (DISTANCES - 1) as f32)
            .clamp(0.0, (DISTANCES - 1) as f32);
        let corner = |at: f32, count: usize| {
            let low = (at.floor() as usize).min(count - 2);
            (low, at - low as f32)
        };
        let ((row, row_t), (column, column_t), (slice, slice_t)) = (
            corner(row, VIEWS),
            corner(column, AZIMUTHS),
            corner(slice, DISTANCES),
        );
        let mut light = Vec4::ZERO;
        let mut crossed = Vec4::ZERO;
        for (dr, wr) in [(0, 1.0 - row_t), (1, row_t)] {
            for (dc, wc) in [(0, 1.0 - column_t), (1, column_t)] {
                for (ds, ws) in [(0, 1.0 - slice_t), (1, slice_t)] {
                    let weight = wr * wc * ws;
                    let (cell_light, cell_crossed) =
                        self.cells[((row + dr) * AZIMUTHS + column + dc) * DISTANCES + slice + ds];
                    light += cell_light * weight;
                    crossed += cell_crossed * weight;
                }
            }
        }
        (light, crossed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows crowd round the horizon and come back to the same view cosine.
    #[test]
    fn rows_round_trip_and_crowd_round_the_horizon() {
        let table = Aerial {
            radius_km: GROUND_KM + 2.0,
            sun_cosine: 0.1,
            horizon: -(1.0 - (GROUND_KM / (GROUND_KM + 2.0)).powi(2)).sqrt(),
            reach_km: 400.0,
            cells: Vec::new(),
        };
        for row in 0..VIEWS {
            let mu = table.view_cosine(row);
            assert!((table.view_row(mu) - row as f32).abs() < 1e-2, "{row}");
        }
        let middle = table.view_cosine(VIEWS / 2);
        let next = table.view_cosine(VIEWS / 2 + 1);
        assert!((next - middle).abs() < 0.01);
    }
}
