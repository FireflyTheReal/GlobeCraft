//! The air (roadmap 8.3): sunlight scattered by air molecules (Rayleigh) and haze
//! (Mie) and absorbed by ozone, so the limb is blue by day, red at the terminator and
//! dark at night, and distant ground fades into haze.
//!
//! Single scattering, from two tables computed once: the light that crosses the air
//! from a height in a direction (`transmittance`), and the sunlight scattered toward
//! the eye along a ray (`scattering`), by height, view angle, sun angle and the angle
//! between them. The parameters are those of Bruneton and Neyret, "Precomputed
//! Atmospheric Scattering" (2008), in Bruneton's 2017 form: distances to the ground
//! and to the top of the air instead of angles, which keeps the horizon sharp.
//!
//! Light scattered more than once is added as Hillaire's "A Scalable and Production
//! Ready Sky and Atmosphere Rendering Technique" (2020) does: a small table of the
//! light that reaches a point after any number of bounces, isotropic, by height and
//! sun angle (`multiple`). It keeps the twilight sky deep blue, lights the air in the
//! Earth's shadow, and is folded into the scattering table, so the GPU reads it too.
pub(crate) mod aerial;

use glam::Vec4;
use globecraft_core::EARTH_RADIUS_KM;
use std::sync::LazyLock;

/// The ground and the top of the air, from the Earth's centre. The air is taken as a
/// shell around a sphere; its last traces above 100 km scatter nothing visible.
pub(crate) const GROUND_KM: f32 = EARTH_RADIUS_KM as f32;
pub(crate) const HEIGHT_KM: f32 = 100.0;
pub(crate) const TOP_KM: f32 = GROUND_KM + HEIGHT_KM;
/// The height at which air (8 km) and haze (1.2 km) have thinned to 1/e.
const RAYLEIGH_SCALE_HEIGHT_KM: f32 = 8.0;
const MIE_SCALE_HEIGHT_KM: f32 = 1.2;
/// Scattering by air per kilometre at sea level for red, green and blue light
/// (680, 550 and 440 nm): blue scatters five times more than red.
const RAYLEIGH_SCATTERING_PER_KM: [f32; 3] = [5.802e-3, 13.558e-3, 33.1e-3];
/// Haze scatters all colours alike and absorbs a tenth more.
const MIE_SCATTERING_PER_KM: f32 = 3.996e-3;
const MIE_EXTINCTION_PER_KM: f32 = 4.40e-3;
/// How strongly haze scatters forward (Cornette-Shanks asymmetry).
const MIE_ASYMMETRY: f32 = 0.8;
/// Ozone absorbs (mostly green and red, which keeps the twilight sky blue) in a
/// layer centred on 25 km that thins to nothing 15 km above and below.
const OZONE_ABSORPTION_PER_KM: [f32; 3] = [0.650e-3, 1.881e-3, 0.085e-3];
const OZONE_CENTRE_KM: f32 = 25.0;
const OZONE_HALF_WIDTH_KM: f32 = 15.0;
/// The sun's disc sets over this many radians.
const SUN_ANGULAR_RADIUS: f32 = 0.004_675;
/// The lowest sun the scattering table holds, 30° below the horizon. A ray from
/// space crosses up to 2,000 km of air, a fifth of the way round the horizon: with
/// the sun 30° down where it enters, no air along it is lit any more.
const MIN_SUN_COSINE: f32 = -0.5;

/// Table sizes. Transmittance: by height and by view angle. Scattering: by height,
/// view angle (half for rays that reach the ground, half for rays that leave), sun
/// angle and the angle between view and sun.
pub(crate) const TRANSMITTANCE_HEIGHTS: usize = 64;
pub(crate) const TRANSMITTANCE_ANGLES: usize = 256;
pub(crate) const SCATTERING_HEIGHTS: usize = 16;
pub(crate) const SCATTERING_VIEWS: usize = 64;
pub(crate) const SCATTERING_SUNS: usize = 32;
pub(crate) const SCATTERING_PHASES: usize = 8;
/// Steps along a ray when the tables are computed.
const TRANSMITTANCE_STEPS: usize = 200;
const SCATTERING_STEPS: usize = 50;
/// The multiple-scattering table: by height and sun cosine (-1 to 1); directions
/// around each point, and steps along each, when it is computed.
const MULTIPLE_HEIGHTS: usize = 32;
const MULTIPLE_SUNS: usize = 64;
const MULTIPLE_DIRECTIONS: usize = 64;
const MULTIPLE_STEPS: usize = 24;
/// The share of light the ground sends back (a mean over land and sea).
const GROUND_ALBEDO: f32 = 0.3;
/// The eye adapts to twilight (Bing Maps' blue hour, Julien's recordings of
/// 2026-11-21): from inside the air, the air's light is drawn up to this many times
/// brighter and bluer as the sun goes from this sine (6° up) to this one (8° down);
/// from the camera's height in the air on, less, and not at all from space.
const MAX_ADAPTATION: f32 = 24.0;
const ADAPT_FROM_SUN_COSINE: f32 = 0.105;
const ADAPT_TO_SUN_COSINE: f32 = -0.139;
const BLUE_HOUR: [f32; 3] = [0.6, 0.8, 1.5];
const ADAPT_FADE_KM: [f32; 2] = [20.0, 80.0];
/// The air between the camera and the ground is integrated along the ray
/// (`aerial`) with the camera below the first height (km), the tables' way above the
/// second, a mix between.
pub(crate) const AERIAL_FADE_KM: [f32; 2] = [15.0, 30.0];

/// How bright scattered sunlight is drawn: radiance for a sun of irradiance one, times
/// this, then compressed to the screen's range (`tone`).
pub(crate) const EXPOSURE: f32 = 18.0;
/// How much of the air between the eye and the ground is drawn when looking straight
/// down from space. The imagery was photographed through the air and corrected for
/// it, so the full physical haze would wash it out; toward the limb the share rises
/// to all of it (`ground_share`), where it meets the sky.
pub(crate) const NADIR_HAZE_SHARE: f32 = 0.3;
/// Without a sun (the globe lit everywhere) the air is lit as by a sun this high
/// (cosine of its zenith angle), at right angles to the view.
pub(crate) const UNLIT_SUN_COSINE: f32 = 0.8;
/// With the night lights and no sun the whole globe is night: the air is lit as by
/// a sun straight below, that is not at all.
pub(crate) const NIGHT_SUN_COSINE: f32 = -1.0;

fn horizon_km() -> f32 {
    (TOP_KM * TOP_KM - GROUND_KM * GROUND_KM).sqrt()
}

/// The light of a ray through the air: sunlight scattered toward the eye (radiance
/// for a sun of irradiance one, before `EXPOSURE`) and the share of the light behind
/// it that arrives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Air {
    pub(crate) scattered: Vec4,
    pub(crate) transmitted: Vec4,
    /// The colour of the sunlight that reaches the ground behind the air, as a factor
    /// on the screen's colours: white under a high sun, orange and red as it sets.
    pub(crate) sunlight: Vec4,
}

impl Air {
    /// The sky's own `colour` (stars, the Milky Way) behind this air. The eye and
    /// the camera adapt to a bright sky: the stars go out by day.
    pub(crate) fn over_sky(self, colour: Vec4) -> Vec4 {
        let glow = tone(self.scattered * EXPOSURE);
        let dark = 1.0 - glow.max_element();
        let dark = dark * dark;
        let linear = colour * colour * self.transmitted * (dark * dark) + glow;
        linear.sqrt()
    }
    /// [`Self::over_sky`] for a sky drawn behind the frame: the glow as premultiplied
    /// colour and, as alpha, how much of the sky it hides. The two add up as drawn,
    /// not in linear light, and the air's tint of the stars is left out: close
    /// enough where it shows, at the limb's thin glow.
    pub(crate) fn behind(self) -> Vec4 {
        let glow = tone(self.scattered * EXPOSURE);
        let dark = 1.0 - glow.max_element();
        let through = self.transmitted.truncate().element_sum() / 3.0;
        glow.truncate()
            .powf(0.5)
            .extend(1.0 - through.sqrt() * dark * dark)
    }
    /// `colour` (as drawn, 0 to 1) seen through this air, with `share` of the air
    /// counted. Light adds up in linear terms: the screen's colours are squared,
    /// dimmed, the scattered light is added, and the root is taken again.
    pub(crate) fn over(self, colour: Vec4, share: f32) -> Vec4 {
        let transmitted = Vec4::ONE + (self.transmitted - Vec4::ONE) * share;
        let linear = colour * colour * transmitted + tone(self.scattered * (EXPOSURE * share));
        linear.sqrt()
    }
}

/// Compresses scattered light to the screen's range: x / (1 + x).
fn tone(light: Vec4) -> Vec4 {
    light / (Vec4::ONE + light)
}

/// The share of the air drawn over ground seen from space at this view angle (the
/// cosine between the view and the vertical there): `NADIR_HAZE_SHARE` straight
/// down, all of it at the limb.
pub(crate) fn ground_share(view_cosine: f32) -> f32 {
    let grazing = 1.0 - view_cosine;
    let grazing = grazing * grazing;
    NADIR_HAZE_SHARE + (1.0 - NADIR_HAZE_SHARE) * grazing * grazing
}

/// The tables, computed on first use (a fraction of a second).
pub(crate) static TABLES: LazyLock<Tables> = LazyLock::new(Tables::compute);

/// `f` over `0..count`, in order, on plain threads. Not on rayon's: the tables are
/// made inside `TABLES`' lock, and a rayon thread waiting there on its own parallel
/// work picks up other jobs, such as a pixel row that waits on the same lock: the
/// frame never came (black probe frames, 2026-11-22).
fn spread<T: Send>(count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let chunk = count.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let parts: Vec<_> = (0..count)
            .step_by(chunk)
            .map(|start| {
                let f = &f;
                scope.spawn(move || {
                    (start..(start + chunk).min(count))
                        .map(f)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        parts
            .into_iter()
            .flat_map(|part| {
                part.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

/// The colour of the sunlight on the ground where the sun's cosine is `sun_cosine`
/// (as `Air::sunlight`), for what is lit outside the raster (buildings).
pub(crate) fn ground_sunlight(sun_cosine: f32) -> Vec4 {
    TABLES.sunlight(TABLES.to_top(GROUND_KM, sun_cosine.max(0.0)))
}

pub(crate) struct Tables {
    /// The distance from the top of the air to its horizon: √(top² − ground²).
    pub(crate) horizon: f32,
    /// `sun_coordinate`'s measure of the lowest sun the scattering table holds.
    pub(crate) lowest_sun: f32,
    /// Red against green and against blue in sunlight from straight overhead.
    pub(crate) noon: [f32; 2],
    /// The share of light that reaches the top of the air from a height (rows) along
    /// a ray that does not meet the ground (columns, by its length).
    transmittance: Vec<Vec4>,
    /// Sunlight scattered toward the eye along a ray, before the phase functions:
    /// air in the first three channels, haze (its red) in the fourth.
    scattering: Vec<Vec4>,
    /// Light reaching a point after two or more bounces, per unit of scattering, by
    /// height (rows) and sun cosine (columns); only while the tables are computed.
    multiple: Vec<Vec4>,
}

/// Air and haze density (relative to sea level) and ozone (relative to its layer's
/// centre) at a height.
fn densities(height_km: f32) -> (f32, f32, f32) {
    (
        (-height_km / RAYLEIGH_SCALE_HEIGHT_KM).exp(),
        (-height_km / MIE_SCALE_HEIGHT_KM).exp(),
        (1.0 - (height_km - OZONE_CENTRE_KM).abs() / OZONE_HALF_WIDTH_KM).max(0.0),
    )
}

/// How far a ray from radius `r` at view cosine `mu` runs to the top of the air.
fn distance_to_top(r: f32, mu: f32) -> f32 {
    (-r * mu + (r * r * (mu * mu - 1.0) + TOP_KM * TOP_KM).max(0.0).sqrt()).max(0.0)
}

/// How far it runs to the ground, when it meets it.
fn distance_to_ground(r: f32, mu: f32) -> f32 {
    (-r * mu
        - (r * r * (mu * mu - 1.0) + GROUND_KM * GROUND_KM)
            .max(0.0)
            .sqrt())
    .max(0.0)
}

/// Whether a ray from radius `r` at view cosine `mu` meets the ground.
pub(crate) fn meets_ground(r: f32, mu: f32) -> bool {
    mu < 0.0 && r * r * (mu * mu - 1.0) + GROUND_KM * GROUND_KM >= 0.0
}

/// The radius and the view and sun cosines `d` further along a ray.
fn along(r: f32, mu: f32, mu_s: f32, nu: f32, d: f32) -> (f32, f32, f32) {
    let r_d = (d * d + 2.0 * r * mu * d + r * r)
        .sqrt()
        .clamp(GROUND_KM, TOP_KM);
    (
        r_d,
        ((r * mu + d) / r_d).clamp(-1.0, 1.0),
        ((r * mu_s + d * nu) / r_d).clamp(-1.0, 1.0),
    )
}

/// The phase functions' constant factors (Rayleigh; Cornette-Shanks for haze).
pub(crate) const AIR_PHASE: f32 = 3.0 / (16.0 * std::f32::consts::PI);
pub(crate) const HAZE_PHASE: f32 = 3.0 / (8.0 * std::f32::consts::PI)
    * (1.0 - MIE_ASYMMETRY * MIE_ASYMMETRY)
    / (2.0 + MIE_ASYMMETRY * MIE_ASYMMETRY);
pub(crate) const HAZE_SPREAD: [f32; 2] = [1.0 + MIE_ASYMMETRY * MIE_ASYMMETRY, 2.0 * MIE_ASYMMETRY];
/// Haze's green and blue from its red: it scatters all colours alike, the air does not.
pub(crate) const HAZE_GREEN: f32 = RAYLEIGH_SCATTERING_PER_KM[0] / RAYLEIGH_SCATTERING_PER_KM[1];
pub(crate) const HAZE_BLUE: f32 = RAYLEIGH_SCATTERING_PER_KM[0] / RAYLEIGH_SCATTERING_PER_KM[2];

/// How brightly air and haze scatter by the cosine between sun and view.
fn phases(nu: f32) -> (f32, f32) {
    let spread = HAZE_SPREAD[0] - HAZE_SPREAD[1] * nu;
    let both = 1.0 + nu * nu;
    (
        AIR_PHASE * both,
        HAZE_PHASE * both / (spread * spread.sqrt()),
    )
}

/// The scattered light of a table entry: air in rgb, haze from its red in `w` (haze
/// scatters all colours alike, so its green and blue follow the air's).
pub(crate) fn radiance(entry: Vec4, nu: f32) -> Vec4 {
    let (air, haze) = phases(nu);
    let haze_light = if entry.x > 0.0 {
        let ratio = entry.w / entry.x;
        Vec4::new(
            entry.w,
            entry.y * ratio * HAZE_GREEN,
            entry.z * ratio * HAZE_BLUE,
            0.0,
        )
    } else {
        Vec4::ZERO
    };
    Vec4::new(entry.x, entry.y, entry.z, 0.0) * air + haze_light * haze
}

fn lerp(a: Vec4, b: Vec4, t: f32) -> Vec4 {
    a + (b - a) * t
}

/// A table position: the cell below it (at most the last but one) and the way to the
/// next.
fn cell(position: f32, cells: usize) -> (usize, f32) {
    let position = position.clamp(0.0, (cells - 1) as f32);
    let below = (position as usize).min(cells - 2);
    (below, position - below as f32)
}

impl Tables {
    fn compute() -> Self {
        let horizon = horizon_km();
        let mut tables = Self {
            horizon,
            lowest_sun: (distance_to_top(GROUND_KM, MIN_SUN_COSINE) - HEIGHT_KM)
                / (horizon - HEIGHT_KM),
            noon: [1.0; 2],
            transmittance: Vec::new(),
            scattering: Vec::new(),
            multiple: Vec::new(),
        };
        tables.transmittance = spread(TRANSMITTANCE_HEIGHTS * TRANSMITTANCE_ANGLES, |index| {
            let rho = horizon * (index / TRANSMITTANCE_ANGLES) as f32
                / (TRANSMITTANCE_HEIGHTS - 1) as f32;
            let r = (rho * rho + GROUND_KM * GROUND_KM).sqrt();
            let (d_min, d_max) = (TOP_KM - r, rho + horizon);
            let d = d_min
                + (d_max - d_min) * (index % TRANSMITTANCE_ANGLES) as f32
                    / (TRANSMITTANCE_ANGLES - 1) as f32;
            let mu = if d == 0.0 {
                1.0
            } else {
                ((horizon * horizon - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
            };
            optical_depth_to_top(r, mu)
        });
        let noon = tables.transmittance[0];
        tables.noon = [noon.x / noon.y, noon.x / noon.z];
        tables.multiple = spread(MULTIPLE_HEIGHTS * MULTIPLE_SUNS, |index| {
            let height = HEIGHT_KM * (index / MULTIPLE_SUNS) as f32 / (MULTIPLE_HEIGHTS - 1) as f32;
            let mu_s = 2.0 * (index % MULTIPLE_SUNS) as f32 / (MULTIPLE_SUNS - 1) as f32 - 1.0;
            tables.multiple_at(GROUND_KM + height, mu_s)
        });
        let views = SCATTERING_VIEWS / 2;
        tables.scattering = spread(
            SCATTERING_HEIGHTS * SCATTERING_VIEWS * SCATTERING_SUNS,
            |index| {
                let rho = horizon * (index / (SCATTERING_VIEWS * SCATTERING_SUNS)) as f32
                    / (SCATTERING_HEIGHTS - 1) as f32;
                let r = (rho * rho + GROUND_KM * GROUND_KM).sqrt();
                let view = index / SCATTERING_SUNS % SCATTERING_VIEWS;
                // The lower half holds the rays that reach the ground, from the horizon
                // (first) to straight down; the upper half the rays that leave, from
                // straight up to the horizon (last).
                let (mu, to_ground) = if view < views {
                    let (d_min, d_max) = (r - GROUND_KM, rho);
                    let d =
                        d_min + (d_max - d_min) * (views - 1 - view) as f32 / (views - 1) as f32;
                    let mu = if d == 0.0 {
                        -1.0
                    } else {
                        (-(rho * rho + d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
                    };
                    (mu, true)
                } else {
                    let (d_min, d_max) = (TOP_KM - r, rho + horizon);
                    let d = d_min + (d_max - d_min) * (view - views) as f32 / (views - 1) as f32;
                    let mu = if d == 0.0 {
                        1.0
                    } else {
                        ((horizon * horizon - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
                    };
                    (mu, false)
                };
                let tables = &tables;
                let mu_s = tables
                    .sun_cosine((index % SCATTERING_SUNS) as f32 / (SCATTERING_SUNS - 1) as f32);
                (0..SCATTERING_PHASES)
                    .map(move |phase| {
                        // Only some angles between view and sun exist for a view and a sun
                        // direction; the others take the nearest that does.
                        let reach = ((1.0 - mu * mu) * (1.0 - mu_s * mu_s)).max(0.0).sqrt();
                        let nu = (2.0 * phase as f32 / (SCATTERING_PHASES - 1) as f32 - 1.0)
                            .clamp(mu * mu_s - reach, mu * mu_s + reach);
                        tables.scattered_along(r, mu, mu_s, nu, to_ground)
                    })
                    .collect::<Vec<_>>()
            },
        )
        .into_iter()
        .flatten()
        .collect();
        tables
    }

    /// Where the sun's cosine falls in the scattering table (0 to 1): finer toward the
    /// horizon, down to `MIN_SUN_COSINE`.
    fn sun_coordinate(&self, mu_s: f32) -> f32 {
        let a = (distance_to_top(GROUND_KM, mu_s) - HEIGHT_KM) / (self.horizon - HEIGHT_KM);
        (1.0 - a / self.lowest_sun).max(0.0) / (1.0 + a)
    }

    /// The sun's cosine at a table coordinate (the inverse of `sun_coordinate`).
    fn sun_cosine(&self, coordinate: f32) -> f32 {
        let limit = self.lowest_sun;
        let a = (limit - coordinate * limit) / (1.0 + coordinate * limit);
        let d = HEIGHT_KM + a.min(limit) * (self.horizon - HEIGHT_KM);
        if d == 0.0 {
            1.0
        } else {
            ((self.horizon * self.horizon - d * d) / (2.0 * GROUND_KM * d)).clamp(-1.0, 1.0)
        }
    }

    /// Sunlight scattered toward a ray's start along its whole way through the air
    /// (a scattering table entry), by stepping along it.
    fn scattered_along(&self, r: f32, mu: f32, mu_s: f32, nu: f32, to_ground: bool) -> Vec4 {
        let length = if to_ground {
            distance_to_ground(r, mu)
        } else {
            distance_to_top(r, mu)
        };
        let step = length / SCATTERING_STEPS as f32;
        let (mut air, mut haze, mut multiple) = (Vec4::ZERO, 0.0, Vec4::ZERO);
        let rayleigh = Vec4::from((glam::Vec3::from(RAYLEIGH_SCATTERING_PER_KM), 0.0));
        for index in 0..=SCATTERING_STEPS {
            let d = step * index as f32;
            let (r_d, _, mu_s_d) = along(r, mu, mu_s, nu, d);
            let crossed = self.between(r, mu, d, to_ground);
            let light = crossed * self.to_sun(r_d, mu_s_d);
            let (air_density, haze_density, _) = densities(r_d - GROUND_KM);
            let weight = if index == 0 || index == SCATTERING_STEPS {
                0.5
            } else {
                1.0
            };
            air += light * (air_density * weight);
            haze += light.x * haze_density * weight;
            if !self.multiple.is_empty() {
                let scattering =
                    rayleigh * air_density + Vec4::splat(MIE_SCATTERING_PER_KM * haze_density);
                multiple += crossed * scattering * self.multiple(r_d, mu_s_d) * weight;
            }
        }
        let [red, green, blue] = RAYLEIGH_SCATTERING_PER_KM;
        // Light from many bounces comes from all around: the table's entries are
        // drawn with the air's phase function (`radiance`), which this undoes.
        let (air_phase, _) = phases(nu);
        let multiple = multiple * (step / air_phase);
        Vec4::new(
            air.x * red * step + multiple.x,
            air.y * green * step + multiple.y,
            air.z * blue * step + multiple.z,
            haze * MIE_SCATTERING_PER_KM * step,
        )
    }

    /// The light reaching radius `r` (sun cosine `mu_s`) after two or more bounces,
    /// per unit of scattering there (Hillaire 2020, his Ψ_ms): the light of second
    /// order from all around, through an isotropic phase, divided by 1 − f_ms, the
    /// share each further order keeps.
    fn multiple_at(&self, r: f32, mu_s: f32) -> Vec4 {
        let isotropic = 1.0 / (4.0 * std::f32::consts::PI);
        let sun = glam::Vec3::new((1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s);
        let (mut second, mut kept) = (Vec4::ZERO, Vec4::ZERO);
        for direction in 0..MULTIPLE_DIRECTIONS {
            // Spread evenly over the sphere (a Fibonacci lattice).
            let mu = 1.0 - 2.0 * (direction as f32 + 0.5) / MULTIPLE_DIRECTIONS as f32;
            let turn = direction as f32 * 2.399_963;
            let across = (1.0 - mu * mu).max(0.0).sqrt();
            let view = glam::Vec3::new(across * turn.cos(), across * turn.sin(), mu);
            let nu = view.dot(sun);
            let to_ground = meets_ground(r, mu);
            let length = if to_ground {
                distance_to_ground(r, mu)
            } else {
                distance_to_top(r, mu)
            };
            let step = length / MULTIPLE_STEPS as f32;
            let mut depth = Vec4::ZERO;
            for index in 0..MULTIPLE_STEPS {
                let d = step * (index as f32 + 0.5);
                let (r_d, _, mu_s_d) = along(r, mu, mu_s, nu, d);
                let (air, haze, ozone) = densities(r_d - GROUND_KM);
                let scattering = Vec4::new(
                    RAYLEIGH_SCATTERING_PER_KM[0] * air + MIE_SCATTERING_PER_KM * haze,
                    RAYLEIGH_SCATTERING_PER_KM[1] * air + MIE_SCATTERING_PER_KM * haze,
                    RAYLEIGH_SCATTERING_PER_KM[2] * air + MIE_SCATTERING_PER_KM * haze,
                    0.0,
                );
                let extinction = Vec4::new(
                    RAYLEIGH_SCATTERING_PER_KM[0] * air
                        + MIE_EXTINCTION_PER_KM * haze
                        + OZONE_ABSORPTION_PER_KM[0] * ozone,
                    RAYLEIGH_SCATTERING_PER_KM[1] * air
                        + MIE_EXTINCTION_PER_KM * haze
                        + OZONE_ABSORPTION_PER_KM[1] * ozone,
                    RAYLEIGH_SCATTERING_PER_KM[2] * air
                        + MIE_EXTINCTION_PER_KM * haze
                        + OZONE_ABSORPTION_PER_KM[2] * ozone,
                    0.0,
                );
                let middle = depth + extinction * (0.5 * step);
                let crossed =
                    Vec4::new((-middle.x).exp(), (-middle.y).exp(), (-middle.z).exp(), 0.0);
                second += crossed * scattering * self.to_sun(r_d, mu_s_d) * (isotropic * step);
                kept += crossed * scattering * step;
                depth += extinction * step;
            }
            if to_ground {
                let crossed = Vec4::new((-depth.x).exp(), (-depth.y).exp(), (-depth.z).exp(), 0.0);
                let (_, _, mu_s_g) = along(r, mu, mu_s, nu, length);
                second += crossed
                    * self.to_sun(GROUND_KM, mu_s_g)
                    * (mu_s_g.max(0.0) * GROUND_ALBEDO / std::f32::consts::PI);
            }
        }
        // The means over the sphere, each direction weighed by the isotropic phase.
        let directions = MULTIPLE_DIRECTIONS as f32;
        let (second, kept) = (second / directions, kept / directions);
        let most = Vec4::splat(0.99);
        second / (Vec4::ONE - kept.min(most))
    }

    /// The multiple-scattering table's entry at radius `r` and sun cosine `mu_s`.
    fn multiple(&self, r: f32, mu_s: f32) -> Vec4 {
        let (height, height_step) = cell(
            (r - GROUND_KM) / HEIGHT_KM * (MULTIPLE_HEIGHTS - 1) as f32,
            MULTIPLE_HEIGHTS,
        );
        let (sun, sun_step) = cell(
            (mu_s + 1.0) * 0.5 * (MULTIPLE_SUNS - 1) as f32,
            MULTIPLE_SUNS,
        );
        let at = |height: usize, sun: usize| self.multiple[height * MULTIPLE_SUNS + sun];
        lerp(
            lerp(at(height, sun), at(height, sun + 1), sun_step),
            lerp(at(height + 1, sun), at(height + 1, sun + 1), sun_step),
            height_step,
        )
    }

    /// The share of light that reaches the top of the air from radius `r` along a ray
    /// at view cosine `mu` that does not meet the ground.
    pub(crate) fn to_top(&self, r: f32, mu: f32) -> Vec4 {
        let horizon = self.horizon;
        let rho = (r * r - GROUND_KM * GROUND_KM).max(0.0).sqrt();
        let (d_min, d_max) = (TOP_KM - r, rho + horizon);
        let (column, across) = cell(
            (distance_to_top(r, mu) - d_min) / (d_max - d_min) * (TRANSMITTANCE_ANGLES - 1) as f32,
            TRANSMITTANCE_ANGLES,
        );
        let (row, up) = cell(
            rho / horizon * (TRANSMITTANCE_HEIGHTS - 1) as f32,
            TRANSMITTANCE_HEIGHTS,
        );
        let at =
            |row: usize, column: usize| self.transmittance[row * TRANSMITTANCE_ANGLES + column];
        lerp(
            lerp(at(row, column), at(row, column + 1), across),
            lerp(at(row + 1, column), at(row + 1, column + 1), across),
            up,
        )
    }

    /// The share of light that crosses the first `d` km of a ray from radius `r` at
    /// view cosine `mu`.
    fn between(&self, r: f32, mu: f32, d: f32, to_ground: bool) -> Vec4 {
        let (r_d, mu_d, _) = along(r, mu, 0.0, 0.0, d);
        // Light passes the same either way: a ray going down is measured upward.
        let (whole, rest) = if to_ground {
            (self.to_top(r_d, -mu_d), self.to_top(r, -mu))
        } else {
            (self.to_top(r, mu), self.to_top(r_d, mu_d))
        };
        (whole / rest.max(Vec4::splat(f32::MIN_POSITIVE))).min(Vec4::ONE)
    }

    /// The share of sunlight that reaches radius `r` where the sun's cosine is
    /// `mu_s`: none once the ground hides it, fading as its disc sets.
    fn to_sun(&self, r: f32, mu_s: f32) -> Vec4 {
        let sine = GROUND_KM / r;
        let horizon_cosine = -(1.0 - sine * sine).max(0.0).sqrt();
        let edge = sine * SUN_ANGULAR_RADIUS;
        let above = ((mu_s - horizon_cosine + edge) / (2.0 * edge)).clamp(0.0, 1.0);
        self.to_top(r, mu_s) * (above * above * (3.0 - 2.0 * above))
    }

    /// The scattering table's entry for a ray from radius `r` at view cosine `mu`,
    /// with the sun's cosine `mu_s` there and `nu` between view and sun.
    fn scattering(&self, r: f32, mu: f32, mu_s: f32, nu: f32, to_ground: bool) -> Vec4 {
        let horizon = self.horizon;
        let rho = (r * r - GROUND_KM * GROUND_KM).max(0.0).sqrt();
        let views = SCATTERING_VIEWS / 2;
        let (view, view_step) = if to_ground {
            let (d_min, d_max) = (r - GROUND_KM, rho);
            let way = if d_max == d_min {
                0.0
            } else {
                (distance_to_ground(r, mu) - d_min) / (d_max - d_min)
            };
            cell((views - 1) as f32 * (1.0 - way), views)
        } else {
            let (d_min, d_max) = (TOP_KM - r, rho + horizon);
            let (view, step) = cell(
                (views - 1) as f32 * (distance_to_top(r, mu) - d_min) / (d_max - d_min),
                views,
            );
            (view + views, step)
        };
        let (height, height_step) = cell(
            rho / horizon * (SCATTERING_HEIGHTS - 1) as f32,
            SCATTERING_HEIGHTS,
        );
        let (sun, sun_step) = cell(
            self.sun_coordinate(mu_s) * (SCATTERING_SUNS - 1) as f32,
            SCATTERING_SUNS,
        );
        let (phase, phase_step) = cell(
            (nu + 1.0) * 0.5 * (SCATTERING_PHASES - 1) as f32,
            SCATTERING_PHASES,
        );
        let at = |height: usize, view: usize, sun: usize| {
            let first = ((height * SCATTERING_VIEWS + view) * SCATTERING_SUNS + sun)
                * SCATTERING_PHASES
                + phase;
            lerp(
                self.scattering[first],
                self.scattering[first + 1],
                phase_step,
            )
        };
        let layer = |height: usize| {
            lerp(
                lerp(at(height, view, sun), at(height, view, sun + 1), sun_step),
                lerp(
                    at(height, view + 1, sun),
                    at(height, view + 1, sun + 1),
                    sun_step,
                ),
                view_step,
            )
        };
        if height_step == 0.0 {
            return layer(height);
        }
        if height_step == 1.0 {
            return layer(height + 1);
        }
        lerp(layer(height), layer(height + 1), height_step)
    }

    /// The colour of sunlight that crossed the air with `transmitted` left of it,
    /// against the same under the highest sun (so that noon imagery is unchanged):
    /// green and blue relative to red, as factors on the screen's colours.
    fn sunlight(&self, transmitted: Vec4) -> Vec4 {
        let red = transmitted.x.max(f32::MIN_POSITIVE);
        Vec4::new(
            1.0,
            transmitted.y / red * self.noon[0],
            transmitted.z / red * self.noon[1],
            0.0,
        )
        .min(Vec4::ONE)
        .sqrt()
    }

    /// The air in front of ground seen from space, by the angles at the ground: the
    /// cosines between the view and the vertical (`view_cosine`), the sun and the
    /// vertical, and the view direction and the sun. The GPU shader does the same
    /// sums on the same numbers (`ground_transmittance`, `top_scattering`).
    /// Without a sun the air is lit as by one whose cosine is `unlit` everywhere.
    pub(crate) fn ground_from_space(
        &self,
        view_cosine: f32,
        sun: Option<(f32, f32)>,
        unlit: f32,
    ) -> Air {
        let horizon = self.horizon;
        // From the ground up the view ray to the top of the air.
        let d = horizon * horizon
            / (GROUND_KM * view_cosine
                + (GROUND_KM * GROUND_KM * view_cosine * view_cosine + horizon * horizon).sqrt());
        // 0 straight down, 1 along the horizon.
        let way = ((d - HEIGHT_KM) / (horizon - HEIGHT_KM)).clamp(0.0, 1.0);
        let (column, across) = cell(
            way * (TRANSMITTANCE_ANGLES - 1) as f32,
            TRANSMITTANCE_ANGLES,
        );
        let transmitted = lerp(
            self.transmittance[column],
            self.transmittance[column + 1],
            across,
        );
        let (mu_s, nu) = match sun {
            // The sun's cosine where the ray enters the air.
            Some((mu_s, nu)) => (((GROUND_KM * mu_s - d * nu) / TOP_KM).clamp(-1.0, 1.0), nu),
            None => (unlit, -view_cosine * unlit),
        };
        let views = SCATTERING_VIEWS / 2;
        let (view, view_step) = cell((views - 1) as f32 * (1.0 - way), views);
        let (sun_cell, sun_step) = cell(
            self.sun_coordinate(mu_s) * (SCATTERING_SUNS - 1) as f32,
            SCATTERING_SUNS,
        );
        let (phase, phase_step) = cell(
            (nu + 1.0) * 0.5 * (SCATTERING_PHASES - 1) as f32,
            SCATTERING_PHASES,
        );
        let top = self.top_scattering();
        let at = |view: usize, sun: usize| {
            let first = (view * SCATTERING_SUNS + sun) * SCATTERING_PHASES + phase;
            lerp(top[first], top[first + 1], phase_step)
        };
        let entry = lerp(
            lerp(at(view, sun_cell), at(view, sun_cell + 1), sun_step),
            lerp(at(view + 1, sun_cell), at(view + 1, sun_cell + 1), sun_step),
            view_step,
        );
        let sunlight = match sun {
            Some((ground_sun, _)) => {
                // From the ground toward the sun to the top of the air.
                let ground_sun = ground_sun.max(0.0);
                let d = horizon * horizon
                    / (GROUND_KM * ground_sun
                        + (GROUND_KM * GROUND_KM * ground_sun * ground_sun + horizon * horizon)
                            .sqrt());
                let (column, across) = cell(
                    ((d - HEIGHT_KM) / (horizon - HEIGHT_KM)).clamp(0.0, 1.0)
                        * (TRANSMITTANCE_ANGLES - 1) as f32,
                    TRANSMITTANCE_ANGLES,
                );
                self.sunlight(lerp(
                    self.transmittance[column],
                    self.transmittance[column + 1],
                    across,
                ))
            }
            None => Vec4::ONE,
        };
        Air {
            scattered: radiance(entry, nu),
            transmitted,
            sunlight,
        }
    }

    /// The transmittance table (`TRANSMITTANCE_HEIGHTS` rows of `TRANSMITTANCE_ANGLES`),
    /// its first row from the ground to space by view angle.
    #[cfg(feature = "gpu")]
    pub(crate) fn transmittance_table(&self) -> &[Vec4] {
        &self.transmittance
    }
    /// The whole scattering table (height, view, sun, phase), its last height layer
    /// `top_scattering`.
    #[cfg(feature = "gpu")]
    pub(crate) fn scattering_table(&self) -> &[Vec4] {
        &self.scattering
    }

    /// The scattering table's top layer: rays entering the air from space.
    pub(crate) fn top_scattering(&self) -> &[Vec4] {
        let layer = SCATTERING_VIEWS * SCATTERING_SUNS * SCATTERING_PHASES;
        &self.scattering[(SCATTERING_HEIGHTS - 1) * layer..]
    }
}

/// The share of light that reaches the top of the air from radius `r` at view cosine
/// `mu`, by stepping along the ray (a transmittance table entry).
fn optical_depth_to_top(r: f32, mu: f32) -> Vec4 {
    let step = distance_to_top(r, mu) / TRANSMITTANCE_STEPS as f32;
    let (mut air, mut haze, mut ozone) = (0.0, 0.0, 0.0);
    for index in 0..=TRANSMITTANCE_STEPS {
        let d = step * index as f32;
        let height = (d * d + 2.0 * r * mu * d + r * r).sqrt() - GROUND_KM;
        let (air_density, haze_density, ozone_density) = densities(height);
        let weight = if index == 0 || index == TRANSMITTANCE_STEPS {
            0.5
        } else {
            1.0
        };
        air += air_density * weight;
        haze += haze_density * weight;
        ozone += ozone_density * weight;
    }
    let depth = |channel: usize| {
        step * (RAYLEIGH_SCATTERING_PER_KM[channel] * air
            + MIE_EXTINCTION_PER_KM * haze
            + OZONE_ABSORPTION_PER_KM[channel] * ozone)
    };
    Vec4::new((-depth(0)).exp(), (-depth(1)).exp(), (-depth(2)).exp(), 1.0)
}

/// The air as one frame sees it: the camera's height and the sun are the same for
/// every pixel, so inside the air the tables are narrowed to them once (the
/// scattering table from four dimensions to two), and each pixel reads a few
/// entries instead of dozens.
pub(crate) struct Frame {
    tables: &'static Tables,
    /// The camera: its distance from the Earth's centre and its vertical.
    radius_km: f32,
    up: glam::DVec3,
    /// Where the sun is and its cosine at the camera; without it the air is lit
    /// evenly (`UNLIT_SUN_COSINE`), or not at all at night (`NIGHT_SUN_COSINE`).
    sun: Option<glam::DVec3>,
    sun_cosine: f32,
    inside: Option<Inside>,
    /// The eye's adaptation to twilight, on the air's light (`adaptation`).
    adapt: Vec4,
    /// The air between the camera and the ground, with the camera inside it and a sun.
    aerial: Option<std::sync::Arc<aerial::Aerial>>,
}

/// How far the eye has adapted to twilight (0: day, 1: fully) for a camera `height_km`
/// up where the sun's cosine is `sun_cosine`.
fn adaptation_share(height_km: f32, sun_cosine: f32) -> f32 {
    let low = smoothstep(
        (ADAPT_FROM_SUN_COSINE - sun_cosine) / (ADAPT_FROM_SUN_COSINE - ADAPT_TO_SUN_COSINE),
    );
    low * eye_weight(height_km)
}

/// How far the eye adapts to twilight with the camera `height_km` up: fully in the
/// lower air, not at all from space (`ADAPT_FADE_KM`). The air's light and the
/// ground's (`pixel::Daylight`) both follow it.
pub(crate) fn eye_weight(height_km: f32) -> f32 {
    let [from, to] = ADAPT_FADE_KM;
    1.0 - smoothstep((height_km - from) / (to - from))
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The factor on the air's light for an adaptation `share`: brighter and bluer.
fn adaptation(share: f32) -> Vec4 {
    let tint = Vec4::ONE.lerp(Vec4::from((glam::Vec3::from(BLUE_HOUR), 1.0)), share);
    tint * MAX_ADAPTATION.powf(share)
}

/// The tables at a camera inside the air.
struct Inside {
    /// √(radius² − ground²): the distance to the camera's own horizon.
    rho: f32,
    /// The scattering table at the camera's height and sun, by view and phase.
    scattering: Vec<Vec4>,
    /// The transmittance table's row at the camera's height.
    transmittance: Vec<Vec4>,
}

/// Relief this close to sea level counts as the sea-level ground, where a ray that
/// has arrived scatters nothing further.
pub(crate) const SEA_LEVEL_SLACK_KM: f32 = 0.01;

impl Frame {
    /// `radius` is the camera's distance from the centre in Earth radii, in the
    /// surface's own measure (so that it is right over an ellipsoid's poles too).
    /// Without a `sun`, `night` leaves the air unlit instead of evenly lit.
    pub(crate) fn new(
        camera: glam::DVec3,
        radius: f64,
        sun: Option<glam::DVec3>,
        night: bool,
    ) -> Self {
        let tables: &'static Tables = &TABLES;
        let radius_km = (radius as f32 * GROUND_KM).max(GROUND_KM);
        let up = camera.normalize_or_zero();
        let unlit = if night {
            NIGHT_SUN_COSINE
        } else {
            UNLIT_SUN_COSINE
        };
        let sun_cosine = sun.map_or(unlit, |sun| up.dot(sun) as f32);
        let inside = (radius_km < TOP_KM).then(|| {
            let rho = (radius_km * radius_km - GROUND_KM * GROUND_KM)
                .max(0.0)
                .sqrt();
            let (height, height_step) = cell(
                rho / tables.horizon * (SCATTERING_HEIGHTS - 1) as f32,
                SCATTERING_HEIGHTS,
            );
            let (sun, sun_step) = cell(
                tables.sun_coordinate(sun_cosine) * (SCATTERING_SUNS - 1) as f32,
                SCATTERING_SUNS,
            );
            let at = |height: usize, view: usize, sun: usize, phase: usize| {
                tables.scattering[((height * SCATTERING_VIEWS + view) * SCATTERING_SUNS + sun)
                    * SCATTERING_PHASES
                    + phase]
            };
            let scattering = (0..SCATTERING_VIEWS * SCATTERING_PHASES)
                .map(|index| {
                    let (view, phase) = (index / SCATTERING_PHASES, index % SCATTERING_PHASES);
                    let layer = |height: usize| {
                        lerp(
                            at(height, view, sun, phase),
                            at(height, view, sun + 1, phase),
                            sun_step,
                        )
                    };
                    lerp(layer(height), layer(height + 1), height_step)
                })
                .collect();
            let (row, row_step) = cell(
                rho / tables.horizon * (TRANSMITTANCE_HEIGHTS - 1) as f32,
                TRANSMITTANCE_HEIGHTS,
            );
            let transmittance = (0..TRANSMITTANCE_ANGLES)
                .map(|column| {
                    lerp(
                        tables.transmittance[row * TRANSMITTANCE_ANGLES + column],
                        tables.transmittance[(row + 1) * TRANSMITTANCE_ANGLES + column],
                        row_step,
                    )
                })
                .collect();
            Inside {
                rho,
                scattering,
                transmittance,
            }
        });
        let adapt = match (&inside, sun) {
            (Some(_), Some(_)) => adaptation(adaptation_share(radius_km - GROUND_KM, sun_cosine)),
            _ => Vec4::ONE,
        };
        let aerial =
            (inside.is_some() && sun.is_some() && radius_km - GROUND_KM < AERIAL_FADE_KM[1])
                .then(|| aerial::Aerial::at(tables, radius_km, sun_cosine));
        Self {
            tables,
            radius_km,
            up,
            sun,
            sun_cosine,
            inside,
            adapt,
            aerial,
        }
    }

    /// The cosine between a view direction and the sun; without a sun, as for one at
    /// right angles to the view.
    fn phase(&self, direction: glam::DVec3, mu: f32) -> f32 {
        self.sun
            .map_or(mu * self.sun_cosine, |sun| direction.dot(sun) as f32)
    }

    /// The air in front of the sky along `direction`, whose cosine to the camera's
    /// vertical is `mu`; `None` when the ray passes the air by.
    pub(crate) fn sky(&self, direction: glam::DVec3, mu: f32) -> Option<Air> {
        let r = self.radius_km;
        if let Some(inside) = &self.inside {
            let nu = self.phase(direction, mu);
            let way = inside.way_out(self.tables, r, mu);
            return Some(Air {
                scattered: radiance(inside.scattered(way, false, nu), nu) * self.adapt,
                transmitted: inside.crossed(way),
                sunlight: Vec4::ONE,
            });
        }
        // From space: where the ray enters the air, if it does.
        let reach = r * r * (mu * mu - 1.0) + TOP_KM * TOP_KM;
        if mu >= 0.0 || reach < 0.0 {
            return None;
        }
        let entry = -r * mu - reach.sqrt();
        let mu_top = ((r * mu + entry) / TOP_KM).clamp(-1.0, 1.0);
        let (mu_s, nu) = match self.sun {
            Some(sun) => {
                let nu = direction.dot(sun) as f32;
                (
                    ((r * self.sun_cosine + entry * nu) / TOP_KM).clamp(-1.0, 1.0),
                    nu,
                )
            }
            None => (self.sun_cosine, mu_top * self.sun_cosine),
        };
        Some(Air {
            scattered: radiance(self.tables.scattering(TOP_KM, mu_top, mu_s, nu, false), nu),
            transmitted: self.tables.to_top(TOP_KM, mu_top),
            sunlight: Vec4::ONE,
        })
    }

    /// The air in front of ground or relief `distance_km` along `direction`, where
    /// the surface's normal is `normal` and its cosine to the view `view_cosine`.
    pub(crate) fn ground(
        &self,
        direction: glam::DVec3,
        distance_km: f32,
        normal: glam::DVec3,
        view_cosine: f32,
    ) -> Air {
        let Some(inside) = &self.inside else {
            let sun = self
                .sun
                .map(|sun| (normal.dot(sun) as f32, direction.dot(sun) as f32));
            return self
                .tables
                .ground_from_space(view_cosine, sun, self.sun_cosine);
        };
        let (tables, r) = (self.tables, self.radius_km);
        let mu = direction.dot(self.up) as f32;
        let nu = self.phase(direction, mu);
        // Low down, the air integrated along the ray (`aerial`), faded into the
        // tables' way higher up, where rays rarely skim the horizon.
        let aerial = self.aerial.as_ref().map(|aerial| {
            let height = r - GROUND_KM;
            let [from, to] = AERIAL_FADE_KM;
            let share = 1.0 - smoothstep((height - from) / (to - from));
            (aerial.sample(mu, nu, distance_km), share)
        });
        if let Some(((light, crossed), share)) = aerial
            && share >= 1.0
        {
            let (r_d, _, mu_s_d) = along(r, mu, self.sun_cosine, nu, distance_km);
            return Air {
                scattered: light * self.adapt,
                transmitted: crossed,
                sunlight: tables.sunlight(tables.to_top(r_d, mu_s_d.max(0.0))),
            };
        }
        let to_ground = meets_ground(r, mu);
        let (r_d, mu_d, mu_s_d) = along(r, mu, self.sun_cosine, nu, distance_km);
        // Light passes the same either way: a ray going down is measured upward.
        let (whole, rest) = if to_ground {
            (
                tables.to_top(r_d, -mu_d),
                inside.crossed(inside.way_out(tables, r, -mu)),
            )
        } else {
            (
                inside.crossed(inside.way_out(tables, r, mu)),
                tables.to_top(r_d, mu_d),
            )
        };
        let transmitted = (whole / rest.max(Vec4::splat(f32::MIN_POSITIVE))).min(Vec4::ONE);
        let way = if to_ground {
            inside.way_down(r, mu)
        } else {
            inside.way_out(tables, r, mu)
        };
        // What the whole ray scatters, less what it scatters beyond the point (which
        // is nothing once it has reached the sea-level ground).
        let mut near = inside.scattered(way, to_ground, nu);
        if r_d > GROUND_KM + SEA_LEVEL_SLACK_KM {
            let beyond = tables.scattering(r_d, mu_d, mu_s_d, nu, to_ground);
            let reached = Vec4::new(transmitted.x, transmitted.y, transmitted.z, transmitted.x);
            near = (near - beyond * reached).max(Vec4::ZERO);
        }
        let (scattered, transmitted) = match aerial {
            Some(((light, crossed), share)) => (
                radiance(near, nu).lerp(light, share),
                transmitted.lerp(crossed, share),
            ),
            None => (radiance(near, nu), transmitted),
        };
        Air {
            scattered: scattered * self.adapt,
            transmitted,
            sunlight: match self.sun {
                Some(_) => tables.sunlight(tables.to_top(r_d, mu_s_d.max(0.0))),
                None => Vec4::ONE,
            },
        }
    }
}

/// What the GPU needs of a frame's air with the camera inside it (`Frame::inside_air`).
#[cfg(feature = "gpu")]
pub(crate) struct InsideAir<'a> {
    pub(crate) radius_km: f32,
    pub(crate) up: glam::DVec3,
    pub(crate) sun_cosine: f32,
    pub(crate) rho: f32,
    /// `Inside::scattering` then `Inside::transmittance`.
    pub(crate) scattering: &'a [Vec4],
    pub(crate) transmittance: &'a [Vec4],
    pub(crate) adapt: Vec4,
    pub(crate) aerial: Option<&'a std::sync::Arc<aerial::Aerial>>,
}

#[cfg(feature = "gpu")]
impl Frame {
    /// The frame's air for the GPU, when the camera is inside it.
    pub(crate) fn inside_air(&self) -> Option<InsideAir<'_>> {
        let inside = self.inside.as_ref()?;
        Some(InsideAir {
            radius_km: self.radius_km,
            up: self.up,
            sun_cosine: self.sun_cosine,
            rho: inside.rho,
            scattering: &inside.scattering,
            transmittance: &inside.transmittance,
            adapt: self.adapt,
            aerial: self.aerial.as_ref(),
        })
    }
}

impl Inside {
    /// How far along the transmittance row and the upper half of the views a ray
    /// that leaves the air lies (0 straight up, 1 along the horizon).
    fn way_out(&self, tables: &Tables, r: f32, mu: f32) -> f32 {
        let (d_min, d_max) = (TOP_KM - r, self.rho + tables.horizon);
        (distance_to_top(r, mu) - d_min) / (d_max - d_min)
    }

    /// The same for a ray that reaches the ground (0 straight down, 1 the horizon).
    fn way_down(&self, r: f32, mu: f32) -> f32 {
        let (d_min, d_max) = (r - GROUND_KM, self.rho);
        if d_max == d_min {
            0.0
        } else {
            (distance_to_ground(r, mu) - d_min) / (d_max - d_min)
        }
    }

    /// The share of light that reaches the top of the air along a ray `way_out`.
    fn crossed(&self, way: f32) -> Vec4 {
        let (column, across) = cell(
            way * (TRANSMITTANCE_ANGLES - 1) as f32,
            TRANSMITTANCE_ANGLES,
        );
        lerp(
            self.transmittance[column],
            self.transmittance[column + 1],
            across,
        )
    }

    /// The scattering table's entry for a ray `way` along its half of the views.
    fn scattered(&self, way: f32, to_ground: bool, nu: f32) -> Vec4 {
        let views = SCATTERING_VIEWS / 2;
        let (view, view_step) = if to_ground {
            cell((views - 1) as f32 * (1.0 - way), views)
        } else {
            let (view, step) = cell((views - 1) as f32 * way, views);
            (view + views, step)
        };
        let (phase, phase_step) = cell(
            (nu + 1.0) * 0.5 * (SCATTERING_PHASES - 1) as f32,
            SCATTERING_PHASES,
        );
        let at = |view: usize| {
            let first = view * SCATTERING_PHASES + phase;
            lerp(
                self.scattering[first],
                self.scattering[first + 1],
                phase_step,
            )
        };
        lerp(at(view), at(view + 1), view_step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    /// A camera `height_km` above the ground on the z axis, the sun `sun_cosine`
    /// from its vertical in the xz plane.
    fn frame(height_km: f32, sun_cosine: f64) -> Frame {
        let radius = f64::from(1.0 + height_km / GROUND_KM);
        let sun = DVec3::new((1.0 - sun_cosine * sun_cosine).sqrt(), 0.0, sun_cosine);
        Frame::new(DVec3::new(0.0, 0.0, radius), radius, Some(sun), false)
    }
    /// A direction `mu` from the vertical, turned toward the sun's side.
    fn toward_sun(mu: f64) -> DVec3 {
        DVec3::new((1.0 - mu * mu).sqrt(), 0.0, mu)
    }

    #[test]
    fn the_air_dims_light_as_measured() {
        // Straight up from sea level: Rayleigh optical depths 0.046, 0.108 and 0.265
        // (scattering x 8 km), haze 0.005, ozone 0.010, 0.028 and 0.001.
        let up = TABLES.to_top(GROUND_KM, 1.0);
        for (channel, expected) in [(up.x, 0.941), (up.y, 0.868), (up.z, 0.762)] {
            assert!((channel - expected).abs() < 0.01, "{up:?}");
        }
        // Along the horizon the sun is red: blue is all but gone.
        let low = TABLES.to_top(GROUND_KM, 0.0);
        assert!(low.x > 3.0 * low.y && low.z < 0.01, "{low:?}");
    }

    #[test]
    #[ignore = "manual: prints twilight sky colours"]
    fn twilight_sky_probe() {
        for deg in [5.0f64, 0.0, -2.0, -4.0, -6.0, -9.0] {
            let sun = deg.to_radians().sin();
            for (name, side, up) in [
                ("toward", 1.0, 0.03),
                ("toward", 1.0, 0.3),
                ("away", -1.0, 0.03),
                ("away", -1.0, 0.3),
                ("zenith", 1.0, 0.99),
            ] {
                let up: f64 = up;
                let direction = DVec3::new(side * (1.0 - up * up).sqrt(), 0.0, up);
                let air = frame(0.1, sun).sky(direction, up as f32).unwrap();
                let c = tone(air.scattered * EXPOSURE);
                let c = c.max(Vec4::ZERO);
                let c = c.truncate().to_array().map(|v| (v.sqrt() * 255.0) as u8);
                println!("sun {deg:>4}° {name:>6} up {up:.2}: {c:?}");
            }
        }
    }

    #[test]
    fn the_sky_is_blue_by_day_red_at_sunset_and_dark_at_night() {
        let day = frame(0.1, 0.8).sky(toward_sun(0.2), 0.2).unwrap().scattered;
        assert!(day.z > 1.5 * day.x && day.z > 0.005, "day {day:?}");
        // Looking at the horizon where the sun sets.
        let sunset = frame(0.1, 0.01)
            .sky(toward_sun(0.02), 0.02)
            .unwrap()
            .scattered;
        assert!(sunset.x > 1.5 * sunset.z, "sunset {sunset:?}");
        let night = frame(0.1, -0.6)
            .sky(toward_sun(0.2), 0.2)
            .unwrap()
            .scattered;
        assert!(night.max_element() < 1e-6, "night {night:?}");
        // From space, a ray that passes above the air meets none.
        assert!(frame(400.0, 0.5).sky(toward_sun(-0.2), -0.2).is_none());
    }

    /// Low down the air toward the ground is integrated along the ray (`aerial`): it
    /// agrees with the tables' way where that is sound (looking down), and toward a
    /// far mountain just under the horizon it grows steadily with distance instead of
    /// peaking in a band (Julien's screenshots of 2026-11-22).
    #[test]
    fn the_air_to_the_ground_grows_with_distance_and_agrees_with_the_tables() {
        let height_km = 2.0;
        let sun_cosine = 0.05;
        let low = frame(height_km, sun_cosine);
        let camera = DVec3::new(0.0, 0.0, f64::from(1.0 + height_km / GROUND_KM));
        let normal = DVec3::Z;
        // Looking down at 45° toward the sun: both ways agree within a fifth.
        let down = toward_sun(-0.7);
        let reach = -camera.dot(down);
        let distance =
            (reach - (reach * reach - camera.length_squared() + 1.0).sqrt()) as f32 * GROUND_KM;
        let integrated = low.ground(down, distance, normal, 0.7);
        let mut tables = frame(height_km, sun_cosine);
        tables.aerial = None;
        let classic = tables.ground(down, distance, normal, 0.7);
        let close = |a: Vec4, b: Vec4| (a - b).abs().max_element() <= 0.2 * b.max_element();
        assert!(
            close(integrated.scattered, classic.scattered),
            "{integrated:?} {classic:?}"
        );
        assert!(
            close(integrated.transmitted, classic.transmitted),
            "{integrated:?} {classic:?}"
        );
        // Just under the horizon, toward the sun: the light grows with distance.
        let horizon = -f64::from(1.0 - (GROUND_KM / (GROUND_KM + height_km)).powi(2)).sqrt();
        let skim = toward_sun(horizon - 0.002);
        let mut last = Vec4::ZERO;
        for distance in [5.0, 20.0, 50.0, 90.0, 130.0, 160.0] {
            let air = low.ground(skim, distance, normal, 0.05);
            assert!(air.scattered.x >= last.x - 1e-6, "{distance} km: {air:?}");
            last = air.scattered;
        }
    }

    /// Around the sun the integrated air follows the tables' way (sound well below the
    /// horizon) at every azimuth: no seam where one column of the table took over
    /// (a bright wedge 30° either side of the sun, Julien's recording of 2026-11-22).
    #[test]
    fn the_integrated_air_follows_the_tables_round_the_sun() {
        let (height_km, sun_cosine, mu) = (5.0, 0.05, -0.2f64);
        let integrated = frame(height_km, sun_cosine);
        let mut tables = frame(height_km, sun_cosine);
        tables.aerial = None;
        for step in 0..=36 {
            let azimuth = (f64::from(step) * 5.0).to_radians();
            let across = (1.0 - mu * mu).sqrt();
            let direction = DVec3::new(across * azimuth.cos(), across * azimuth.sin(), mu);
            let (a, b) = (
                integrated.ground(direction, 20.0, DVec3::Z, 0.2),
                tables.ground(direction, 20.0, DVec3::Z, 0.2),
            );
            assert!(
                (a.scattered - b.scattered).abs().max_element() <= 0.15 * b.scattered.max_element(),
                "{}°: {a:?} {b:?}",
                step * 5
            );
        }
    }

    #[test]
    fn the_air_is_the_same_just_inside_and_just_outside_its_top() {
        // The narrowed tables (inside) against the general ones (from space).
        let close = |a: Vec4, b: Vec4| (a - b).abs().max_element() < 0.02 * b.max_element() + 1e-5;
        for sun_cosine in [0.9, 0.3, -0.05] {
            let (inside, outside) = (frame(99.9, sun_cosine), frame(100.1, sun_cosine));
            // The limb: a ray that passes 30 km above the ground.
            let mu = -f64::from(1.0 - ((GROUND_KM + 30.0) / TOP_KM).powi(2)).sqrt();
            let (a, b) = (
                inside.sky(toward_sun(mu), mu as f32).unwrap(),
                outside.sky(toward_sun(mu), mu as f32).unwrap(),
            );
            assert!(close(a.scattered, b.scattered), "sky {a:?} {b:?}");
            assert!(close(a.transmitted, b.transmitted), "sky {a:?} {b:?}");
            // The ground straight below and at a slant.
            for direction in [DVec3::NEG_Z, toward_sun(-0.9)] {
                let ground = |frame: &Frame, height_km: f32| {
                    // Where the ray meets the unit sphere, and the surface there.
                    let camera = DVec3::new(0.0, 0.0, f64::from(1.0 + height_km / GROUND_KM));
                    let reach = -camera.dot(direction);
                    let distance = reach - (reach * reach - camera.length_squared() + 1.0).sqrt();
                    let normal = camera + direction * distance;
                    frame.ground(
                        direction,
                        distance as f32 * GROUND_KM,
                        normal,
                        (-direction.dot(normal)) as f32,
                    )
                };
                let (a, b) = (ground(&inside, 99.9), ground(&outside, 100.1));
                assert!(close(a.scattered, b.scattered), "ground {a:?} {b:?}");
                assert!(close(a.transmitted, b.transmitted), "ground {a:?} {b:?}");
                assert!(close(a.sunlight, b.sunlight), "ground {a:?} {b:?}");
            }
        }
    }

    /// Pictures of the air for judging by eye (the numbers cannot say whether a dusk
    /// looks right). Reads imagery from `GLOBECRAFT_STILL_TILES` (`sat/z/x/y` and
    /// `night/z/x/y` files) and writes PNGs to `GLOBECRAFT_SHOTS`.
    #[test]
    #[ignore = "writes pictures for judging by eye; needs GLOBECRAFT_STILL_TILES and GLOBECRAFT_SHOTS"]
    fn atmosphere_stills() {
        use globecraft_core::{camera::Cam, math::ll_to_v3};
        use globecraft_data::tiles::{Lookup, Provider, Tile, TileKey, unkey};
        use std::{collections::HashMap, path::PathBuf, sync::Arc, sync::Mutex};
        let tiles = PathBuf::from(std::env::var_os("GLOBECRAFT_STILL_TILES").unwrap());
        let shots = PathBuf::from(std::env::var_os("GLOBECRAFT_SHOTS").unwrap());
        let decoded: Mutex<HashMap<TileKey, Option<Arc<Tile>>>> = Mutex::default();
        let lookup = |key: TileKey| {
            let (provider, z, x, y, _) = unkey(key);
            let folder = if provider == Provider::NightLights {
                "night"
            } else {
                "sat"
            };
            let tile = decoded
                .lock()
                .unwrap()
                .entry(key)
                .or_insert_with(|| {
                    let bytes = std::fs::read(tiles.join(format!("{folder}/{z}/{x}/{y}"))).ok()?;
                    Tile::decode(&bytes).ok().map(Arc::new)
                })
                .clone();
            tile.map_or(Lookup::Unavailable, Lookup::Ready)
        };
        let (w, h) = (960, 720);
        let cam = |lat, lon, alt, tilt, heading| Cam {
            lat,
            lon,
            heading,
            alt,
            tilt,
            look_up: 0.0,
        };
        // The view (latitude, longitude, altitude km, tilt, heading) and where the sun
        // is overhead (latitude, longitude).
        let views = [
            (
                "01-globe-day",
                cam(20.0, 10.0, 14000.0, 0.0, 0.0),
                Some((15.0, -10.0)),
            ),
            (
                "02-globe-terminator",
                cam(30.0, 0.0, 12000.0, 0.0, 0.0),
                Some((10.0, 80.0)),
            ),
            (
                "03-globe-night",
                cam(30.0, 0.0, 12000.0, 0.0, 0.0),
                Some((-10.0, 175.0)),
            ),
            (
                "04-globe-crescent",
                cam(30.0, 0.0, 12000.0, 0.0, 0.0),
                Some((5.0, 150.0)),
            ),
            ("05-globe-no-sun", cam(20.0, 10.0, 14000.0, 0.0, 0.0), None),
            (
                "06-europe-2500",
                cam(45.0, 9.0, 2500.0, 0.0, 0.0),
                Some((20.0, 20.0)),
            ),
            (
                "07-limb-400-day",
                cam(45.0, 9.0, 400.0, 72.0, 90.0),
                Some((20.0, 40.0)),
            ),
            (
                "08-limb-400-sunset",
                cam(45.0, 9.0, 400.0, 72.0, 270.0),
                Some((10.0, -82.0)),
            ),
            (
                "09-above-top-110",
                cam(45.0, 9.0, 110.0, 75.0, 90.0),
                Some((20.0, 40.0)),
            ),
            (
                "10-below-top-95",
                cam(45.0, 9.0, 95.0, 75.0, 90.0),
                Some((20.0, 40.0)),
            ),
            (
                "11-inside-30-day",
                cam(45.0, 9.0, 30.0, 80.0, 90.0),
                Some((20.0, 40.0)),
            ),
            (
                "12-inside-5-day",
                cam(45.0, 9.0, 5.0, 84.0, 180.0),
                Some((20.0, 40.0)),
            ),
            (
                "13-inside-5-sunset",
                cam(45.0, 9.0, 5.0, 84.0, 270.0),
                Some((10.0, -80.0)),
            ),
            (
                "14-inside-5-night",
                cam(45.0, 9.0, 5.0, 84.0, 270.0),
                Some((-20.0, 170.0)),
            ),
        ];
        for (name, cam, sun) in views {
            let cfg = crate::RenderCfg {
                sky: None,
                provider: Provider::Satellite,
                terrain: None,
                rel: 0,
                grid: false,
                grid_step: 10.0,
                atmo: true,
                sun: sun.map(|(lat, lon)| ll_to_v3(lat, lon)),
                lod_scale: 1.0,
                level_blend: true,
                fixed_level: None,
                night_lights: sun.map(|_| 0),
                stars: None,
                sky_behind: false,
                clouds: None,
                glint: None,
                aurora: None,
            };
            let basis = cam.basis_for_model(w as f64 / h as f64, Default::default());
            let (pixels, _, _) =
                crate::render_pixels_into(w, h, 0..h, &basis, &cfg, &lookup, Vec::new());
            image::RgbaImage::from_raw(w as u32, h as u32, pixels)
                .unwrap()
                .save(shots.join(format!("{name}.png")))
                .unwrap();
        }
    }
}
