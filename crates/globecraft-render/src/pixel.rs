//! Shared geographic decisions and the sky's colours.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub(super) u: f64,
    pub(super) v: f64,
    pub(super) z0: i32,
    pub(super) frac: f32,
    pub(super) dist: f32,
    pub(super) grid: bool,
    pub(super) shade: f32,
    /// With the sun: the light in its two parts (`Daylight`); `shade` is then unused.
    pub(super) daylight: Option<Daylight>,
    pub(super) night: f32,
    /// How far into the night the pixel is: 0 by day, 1 past twilight (0 without sun).
    pub(super) darkness: f32,
    /// The air between the eye and the pixel and the share of it that is drawn
    /// (`atmosphere::ground_share`): set by `with_air` where a frame is drawn.
    pub(super) air: Option<(atmosphere::Air, f32)>,
    /// Where the pixel is, degrees, and how much ground it covers (metres): for the
    /// polar caps, which use their own tiles (see `polar`).
    pub(super) lat: f64,
    pub(super) lon: f64,
    pub(super) metres_per_pixel: f64,
    /// The sun's glint off the water there (0 to 1, `sun::glint`).
    pub(super) glint: f32,
    /// The aurora's light between the eye and the pixel (`aurora::glow`).
    pub(super) aurora: glam::Vec3,
}
pub(super) fn surface<const ELLIPSOID: bool>(
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    ray: &rays::RaySample,
    cfg: &RenderCfg,
    geometry: globecraft_core::earth_model::SurfaceGeometry,
) -> Surface {
    // The per-pixel angle is referenced to the displayed pixel density, not the
    // raster's: interactive rasters render at a reduced scale on purpose, and
    // the level of detail must not follow that internal resolution or motion
    // frames would sample a coarser zoom band than idle frames. A 0.4-scale
    // raster is 2.5× smaller, so its angle per pixel is 2.5× coarser.
    let pa = 2.0 * VERTICAL_FOV_HALF_TAN * cfg.lod_scale as f64 / h as f64;
    let maxz = cfg.provider.max_zoom() as f64;
    let t = ray.distance;
    let cosinc = ray.incidence;
    let normal = if ELLIPSOID { ray.normal } else { ray.point };
    let latitude_cosine = if ELLIPSOID {
        normal.x.hypot(normal.y)
    } else {
        0.0
    };
    let (lat, lon) = if ELLIPSOID {
        (
            normal.z.atan2(latitude_cosine).to_degrees(),
            normal.y.atan2(normal.x).to_degrees(),
        )
    } else {
        v3_to_ll(ray.point)
    };
    let coslat = if ELLIPSOID {
        latitude_cosine
    } else {
        lat.to_radians().cos()
    }
    .max(MIN_LOD_LATITUDE_COSINE);
    // Level of detail (LOD): softer grazing-angle limit and a sharpness bias,
    // which greatly enlarges the high-resolution area.
    let mpp = t * EARTH_RADIUS_M * pa / cosinc.max(MIN_LOD_INCIDENCE_COSINE).sqrt();
    let (meridional_km, prime_vertical_km) = if ELLIPSOID {
        geometry.curvature_km_from_latitude_sine(normal.z)
    } else {
        (0.0, 0.0)
    };
    let latitude_radius_m = if !ELLIPSOID {
        EARTH_RADIUS_M
    } else {
        prime_vertical_km * METERS_PER_KM
    };
    let zf = ((2.0 * PI * latitude_radius_m * coslat) / (TILE_SIZE_PX * mpp)).log2();
    let lod = zf + LOD_SHARPNESS_BIAS;
    // Blend between two levels: satellite fades across the whole band, text
    // maps over a short end-of-band window so the switch reads as a quick fade
    // instead of a pop. Both share the same ordering so the blend is continuous
    // when the level advances.
    let f = lod.floor();
    let (zl, frac) = (f, lod - f);
    let z0 = zl.clamp(0.0, maxz) as i32;
    let frac = if (z0 as f64) >= maxz {
        0.0
    } else {
        frac as f32
    };

    let latc = lat.clamp(
        -WEB_MERCATOR_MAX_LATITUDE_DEG,
        WEB_MERCATOR_MAX_LATITUDE_DEG,
    );
    let u = ((lon + 180.0) / 360.0).rem_euclid(1.0);
    let s = if ELLIPSOID && lat == latc {
        normal.z
    } else {
        latc.to_radians().sin()
    };
    let v = 0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI);
    let offset = Vec2::new(x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
    let dist = offset.length_squared();

    let grid = if cfg.grid {
        let st = cfg.grid_step;
        let meridian_m_per_degree = if !ELLIPSOID {
            METERS_PER_LATITUDE_DEGREE
        } else {
            meridional_km * METERS_PER_KM * PI / 180.0
        };
        let thr = mpp / meridian_m_per_degree * GRID_LINE_WIDTH_PX;
        let dlat = (lat / st - (lat / st).round()).abs() * st;
        let grid_latitude_cosine = if ELLIPSOID {
            latitude_cosine
        } else {
            lat.to_radians().cos().abs()
        };
        let dlon = (lon / st - (lon / st).round()).abs() * st * grid_latitude_cosine;
        // Express east-west distance in meridional-degree units before comparing
        // against the shared threshold. The two curvature radii differ on an ellipsoid.
        let dlon = if ELLIPSOID {
            dlon * (prime_vertical_km / meridional_km)
        } else {
            dlon
        };
        dlat < thr || dlon < thr
    } else {
        false
    };
    // Night everywhere is drawn as the night side is. The eye adapts to twilight
    // inside the air, as the sky does (`atmosphere::eye_weight`).
    let daylight = match cfg.sun {
        Some(sun) => {
            let camera = ray.point - ray.direction * ray.distance;
            let height_km =
                (camera.length() - ray.point.length()) * globecraft_core::EARTH_RADIUS_KM;
            Some(Daylight::at(
                normal.dot(sun),
                atmosphere::eye_weight(height_km as f32),
            ))
        }
        None => cfg.night_everywhere().then(|| Daylight::at(-1.0, 0.0)),
    };
    let (shade, night, darkness) = if let Some(daylight) = daylight {
        (
            daylight.exposure,
            NIGHT_BLUE_TINT * daylight.darkness,
            daylight.darkness,
        )
    } else {
        (1.0, 0.0, 0.0)
    };
    Surface {
        u,
        v,
        z0,
        frac,
        dist,
        grid,
        shade,
        daylight,
        night,
        darkness,
        air: None,
        lat,
        lon,
        metres_per_pixel: mpp,
        aurora: cfg.aurora.as_ref().map_or(glam::Vec3::ZERO, |aurora| {
            let camera = ray.point - ray.direction * ray.distance;
            crate::aurora::glow(aurora, cfg.sun, camera, ray.direction, ray.distance)
        }),
        glint: match (cfg.sun, &cfg.glint) {
            (Some(sun), Some(water)) => {
                crate::sun::glint(water, lat, lon, ray.direction, ray.normal, sun)
            }
            _ => 0.0,
        },
    }
}
/// The daylight on the ground where the sun stands `sun_cosine` (the sine of its
/// height) above the horizon, in two parts: the sky's, cool, and the sun's direct
/// light, which ridges and blocks can hide and whose colour the air gives
/// (`atmosphere::Air::sunlight`). Two brightness curves set them, both rising with the
/// sun and meeting below the horizon: `lit_level` for ground the sun lights at least
/// at `DIRECT_FLOOR_COSINE`, the shaded level for ground only the sky lights (`SHADE_DAY`
/// of it by day). The direct part is their difference, so it comes in with the sun and
/// nothing dips across sunset. `eye` (0–1) is how far the eye is adapted to twilight
/// (`atmosphere::eye_weight`). The GPU's `overview.wgsl` does the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Daylight {
    /// Lit ground's brightness: night's floor to one by day.
    pub(crate) exposure: f32,
    /// 0 by day, 1 past twilight (the night lights' share).
    pub(crate) darkness: f32,
    /// The sky's light, coloured, at the shaded level's brightness.
    pub(crate) sky: Vec4,
    /// The direct sun's light where it meets a surface at `DIRECT_FLOOR_COSINE` or
    /// more, before its colour.
    pub(crate) direct: f32,
    /// Flat ground's direct light against that (under 1 with the sun low).
    pub(crate) flat_gain: f32,
    pub(crate) sun_cosine: f64,
}

impl Daylight {
    pub(crate) fn at(sun_cosine: f64, eye: f32) -> Self {
        let lit = lit_level(sun_cosine, eye);
        let shaded = if sun_cosine <= SUN_RISE_START_COSINE {
            lit
        } else {
            let risen = lit_level(SUN_RISE_START_COSINE, eye);
            let day = smooth(
                (sun_cosine - SUN_RISE_START_COSINE) / (SKY_DAY_COSINE - SUN_RISE_START_COSINE),
            );
            (risen + (SHADE_DAY - risen) * day).min(lit)
        };
        let colour = glam::Vec3::from(TWILIGHT_SKY).lerp(
            glam::Vec3::from(DAY_SKY),
            smooth(sun_cosine / SKY_DAY_COSINE),
        );
        // The colour carries no brightness of its own.
        let colour = colour / colour.dot(glam::Vec3::from(LUMINANCE));
        Self {
            exposure: lit,
            darkness: 1.0 - smooth((sun_cosine + TWILIGHT_OFFSET) / TWILIGHT_WIDTH),
            sky: Vec4::from((colour * shaded, 0.0)),
            direct: lit - shaded,
            flat_gain: Self::gain(sun_cosine, sun_cosine),
            sun_cosine,
        }
    }
    /// A surface whose normal meets the sun at `cosine` against flat ground (where it
    /// meets it at `flat`): one on flat ground by day, more on a slope facing a low sun
    /// (up to `MAX_DIRECT_GAIN`), none on one turned away.
    pub(crate) fn gain(cosine: f64, flat: f64) -> f32 {
        (cosine / flat.max(DIRECT_FLOOR_COSINE)).clamp(0.0, MAX_DIRECT_GAIN) as f32
    }
    /// The light on a surface lit by the sun in `sun` colour, `gain` times the direct
    /// light, `seen` the share of the sun not hidden (shadows), facing `sky` of the sky.
    pub(crate) fn light(&self, sun: Vec4, gain: f32, seen: f32, sky: f32) -> Vec4 {
        self.sky * sky + sun * (self.direct * gain * seen)
    }
}

/// Lit ground's brightness where the sun's sine is `sun_cosine`: the eye adapted to
/// twilight (`eye` 1) keeps the blue hour readable; from space (0) twilight is short.
pub(crate) fn lit_level(sun_cosine: f64, eye: f32) -> f32 {
    let short = smooth((sun_cosine + TWILIGHT_OFFSET) / TWILIGHT_WIDTH);
    let adapted = smooth((sun_cosine + EXPOSURE_OFFSET) / EXPOSURE_WIDTH);
    NIGHT_BRIGHTNESS + DAY_BRIGHTNESS_RANGE * (short + (adapted - short) * eye)
}

fn smooth(x: f64) -> f32 {
    let x = x.clamp(0.0, 1.0) as f32;
    x * x * (3.0 - 2.0 * x)
}

/// `surface` with the air in front of it, when the frame has an atmosphere.
pub(super) fn with_air(
    mut surface: Surface,
    ray: &rays::RaySample,
    air: Option<&atmosphere::Frame>,
) -> Surface {
    surface.air = air.map(|air| {
        let view_cosine = ray.incidence as f32;
        (
            air.ground(
                ray.direction,
                (ray.distance * globecraft_core::EARTH_RADIUS_KM) as f32,
                ray.normal,
                view_cosine,
            ),
            atmosphere::ground_share(view_cosine),
        )
    });
    surface
}

/// The colour of a ray that misses the Earth: the sky behind the air on its way,
/// with alpha in `w`. `cc` is the camera's distance from the centre squared and
/// `ray.camera_dot` the ray's reach along it, both in the surface's own measure.
pub(super) fn sky(
    ray: &rays::RaySample,
    rays: &RayCaster,
    air: Option<&atmosphere::Frame>,
    cfg: &RenderCfg,
) -> Vec4 {
    let d = ray.direction;
    let cc = rays.camera_len_squared;
    let air = air.and_then(|air| air.sky(d, (ray.camera_dot / cc.sqrt()) as f32));
    // The sun through the air in front of it, over the sky's glow.
    let sun = cfg.sun.map_or(glam::Vec3::ZERO, |sun| {
        let through = air.map_or(glam::Vec3::ONE, |air| air.transmitted.truncate());
        crate::sun::sun_in_sky(d, sun, through)
    });
    // The aurora glows in the air, behind the clouds seen from below.
    let aurora = cfg.aurora.as_ref().map_or(glam::Vec3::ZERO, |aurora| {
        crate::aurora::glow(aurora, cfg.sun, rays.camera(), d, f64::INFINITY)
    });
    // From below, the clouds are in front of the sky and the sun.
    let clouds = cfg.clouds.as_ref().and_then(|clouds| {
        let height_km = (cc.sqrt() - 1.0) * globecraft_core::EARTH_RADIUS_KM;
        crate::clouds::from_below(clouds, rays.model_geometry(), rays.camera(), height_km, d)
    });
    let (cloud, cover) = clouds.unwrap_or((glam::Vec3::ZERO, 0.0));
    if cfg.sky_behind {
        let glow = air.map_or(Vec4::ZERO, atmosphere::Air::behind);
        // Premultiplied: the sun's light hides as much of the sky behind.
        let open = screen((glow.truncate() + sun).min(glam::Vec3::ONE), aurora);
        let alpha = glow.w.max(sun.max_element());
        return (cloud * cover + open * (1.0 - cover)).extend(cover + alpha * (1.0 - cover));
    }
    let col = cfg.sky.as_ref().map_or(SPACE_COLOR, |sky| sky.sample(d));
    let col = air.map_or(col, |air| air.over_sky(col));
    let open = screen((col.truncate() + sun).min(glam::Vec3::ONE), aurora);
    open.lerp(cloud, cover).extend(1.0)
}

/// `light` (0 to 1) laid over `colour` as a screen blend: it brightens, never past 1.
pub(super) fn screen(colour: glam::Vec3, light: glam::Vec3) -> glam::Vec3 {
    colour + light * (glam::Vec3::ONE - colour)
}

/// Space without the air (0 to 1 as drawn), for a sky drawn behind the frame.
pub fn space_color() -> [f32; 3] {
    SPACE_COLOR.truncate().to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::{camera::Cam, earth_model::EarthModel};

    #[test]
    fn level_of_detail_matches_the_displayed_pixel_density_across_raster_scales() {
        // Interactive rasters render at a reduced scale, but the level of
        // detail must be chosen from the displayed pixel density: a 0.4-scale
        // render at height 40 must pick the same band as a full-resolution
        // render at height 100, otherwise imagery strobes between levels at
        // every motion/idle switch.
        let model = EarthModel::Wgs84;
        let basis = Cam {
            lat: -30.0,
            lon: -60.0,
            heading: 25.0,
            alt: 120.0,
            tilt: 0.0,
            look_up: 0.0,
        }
        .basis_for_model(1.0, model);
        let mut bands = Vec::new();
        for (w, h, lod_scale) in [(1, 40, 0.4), (1, 100, 1.0)] {
            let cfg = RenderCfg {
                sky: None,
                provider: Provider::Satellite,
                rel: 0,
                grid: false,
                grid_step: 10.0,
                atmo: false,
                sun: None,
                terrain: None,
                lod_scale,
                level_blend: true,
                fixed_level: None,
                night_lights: None,
                stars: None,
                sky_behind: false,
                clouds: None,
                glint: None,
                aurora: None,
            };
            let rays = RayCaster::new(w, h, &basis);
            let pixel = surface::<true>(
                0,
                h / 2,
                w,
                h,
                &rays.cast(0, h / 2)[0],
                &cfg,
                model.geometry(),
            );
            bands.push((pixel.z0, pixel.frac));
        }
        assert_eq!(
            bands[0].0, bands[1].0,
            "level band must match the display density"
        );
        assert!(
            (bands[0].1 - bands[1].1).abs() < 1e-3,
            "band fraction must match the display density"
        );
    }
    #[test]
    fn ellipsoid_texture_coordinates_match_geodetic_mercator_reference() {
        let model = EarthModel::Wgs84;
        let cfg = RenderCfg {
            sky: None,
            provider: Provider::Satellite,
            terrain: None,
            rel: 0,
            grid: false,
            grid_step: 10.0,
            atmo: false,
            sun: None,
            lod_scale: 1.0,
            level_blend: true,
            fixed_level: None,
            night_lights: None,
            stars: None,
            sky_behind: false,
            clouds: None,
            glint: None,
            aurora: None,
        };
        for latitude in [
            -90.0_f64,
            -85.05112878,
            -45.0,
            -0.0001,
            0.0,
            45.0,
            85.05112878,
            89.999,
            90.0,
        ] {
            for longitude in [-179.9, 0.0, 179.9] {
                let basis = Cam {
                    lat: latitude,
                    lon: longitude,
                    heading: 25.0,
                    alt: 0.15,
                    tilt: 0.0,
                    look_up: 0.0,
                }
                .basis_for_model(1.0, model);
                let rays = RayCaster::new(1, 1, &basis);
                let pixel =
                    surface::<true>(0, 0, 1, 1, &rays.cast(0, 0)[0], &cfg, model.geometry());
                let expected_u = (longitude + 180.0) / 360.0;
                // Equivalent independent form of the spherical Mercator ordinate.
                let latitude = latitude.clamp(
                    -WEB_MERCATOR_MAX_LATITUDE_DEG,
                    WEB_MERCATOR_MAX_LATITUDE_DEG,
                );
                let expected_v = 0.5 - latitude.to_radians().tan().asinh() / (2.0 * PI);
                assert!((pixel.u - expected_u).abs() < 1e-12);
                assert!((pixel.v - expected_v).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn ellipsoid_meridians_use_east_west_surface_distance_for_grid_width() {
        let model = EarthModel::Wgs84;
        let altitude_km = 1.0;
        let threshold_m =
            altitude_km * METERS_PER_KM * 2.0 * VERTICAL_FOV_HALF_TAN * GRID_LINE_WIDTH_PX;
        let cfg = RenderCfg {
            sky: None,
            provider: Provider::Satellite,
            terrain: None,
            rel: 0,
            grid: true,
            grid_step: 1.0,
            atmo: false,
            sun: None,
            lod_scale: 1.0,
            level_blend: true,
            fixed_level: None,
            night_lights: None,
            stars: None,
            sky_behind: false,
            clouds: None,
            glint: None,
            aurora: None,
        };
        // Stay midway between parallels so only the prime meridian can mark
        // this pixel. Distances straddle its physical line-width threshold.
        for (fraction, expected) in [(0.998, true), (1.002, false)] {
            let (lat, lon) =
                model.destination((0.5, 0.0), 90.0, threshold_m * fraction / METERS_PER_KM);
            let basis = Cam {
                lat,
                lon,
                heading: 0.0,
                alt: altitude_km,
                tilt: 0.0,
                look_up: 0.0,
            }
            .basis_for_model(1.0, model);
            let rays = RayCaster::new(1, 1, &basis);
            let ray = &rays.cast(0, 0)[0];
            let pixel = surface::<true>(0, 0, 1, 1, ray, &cfg, model.geometry());
            assert_eq!(
                pixel.grid, expected,
                "{fraction} of the line-width threshold"
            );
        }
    }
}

#[cfg(test)]
mod daylight_tests {
    use super::Daylight;

    /// Lit and shaded ground both only brighten as the sun rises, from space and with
    /// the eye adapted, so nothing is darker just before sunset than just after (the
    /// band past the terminator, Julien's screenshot of 2026-11-22), and shade never
    /// outshines light.
    #[test]
    fn lit_and_shaded_ground_brighten_with_the_sun() {
        for eye in [0.0, 0.5, 1.0] {
            let mut last: Option<(f32, f32)> = None;
            for step in 0..=400 {
                let sun = (f64::from(step) / 400.0 * 120.0 - 30.0).to_radians().sin();
                let day = Daylight::at(sun, eye);
                let shaded = day
                    .sky
                    .truncate()
                    .dot(glam::Vec3::from(crate::tuning::LUMINANCE));
                let lit = shaded + day.direct;
                assert!(day.direct >= 0.0, "eye {eye}, sun {sun}");
                if let Some((last_lit, last_shaded)) = last {
                    assert!(lit >= last_lit - 1e-5, "lit dips: eye {eye}, sun {sun}");
                    assert!(
                        shaded >= last_shaded - 1e-5,
                        "shade dips: eye {eye}, sun {sun}"
                    );
                }
                last = Some((lit, shaded));
            }
            // By day, shade is lit ground's `SHADE_DAY`: midday contrast kept low.
            let noon = Daylight::at(1.0, eye);
            assert!((noon.exposure - 1.0).abs() < 1e-5);
            assert!((noon.direct - (1.0 - crate::tuning::SHADE_DAY)).abs() < 1e-4);
        }
    }
}
