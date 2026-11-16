//! Shared geographic decisions and exact deterministic sky colors.
use super::*;

fn hash3(a: i64, b: i64, c: i64) -> u64 {
    let mut h = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^ (h >> 33)
}

#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub(super) u: f64,
    pub(super) v: f64,
    pub(super) z0: i32,
    pub(super) frac: f32,
    pub(super) dist: f32,
    pub(super) grid: bool,
    pub(super) shade: f32,
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
    let (shade, night, darkness) = if let Some(sun) = cfg.sun {
        let k = ((normal.dot(sun) + TWILIGHT_OFFSET) / TWILIGHT_WIDTH).clamp(0.0, 1.0) as f32;
        let k = k * k * (3.0 - 2.0 * k);
        (
            NIGHT_BRIGHTNESS + DAY_BRIGHTNESS_RANGE * k,
            NIGHT_BLUE_TINT * (1.0 - k),
            1.0 - k,
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
        night,
        darkness,
        air: None,
        lat,
        lon,
        metres_per_pixel: mpp,
    }
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

/// The colour of a ray that misses the Earth: the sky behind the air on its way.
/// `cc` is the camera's distance from the centre squared and `ray.camera_dot` the
/// ray's reach along it, both in the surface's own measure.
pub(super) fn sky(
    ray: &rays::RaySample,
    cc: f64,
    air: Option<&atmosphere::Frame>,
    cfg: &RenderCfg,
    raster_height: usize,
) -> Vec4 {
    let d = ray.direction;
    let mut col = cfg.sky.as_ref().map_or(SPACE_COLOR, |sky| sky.sample(d));
    if cfg.stars {
        let br = if cfg.lod_scale < 1.0 {
            star_across_pixel(d, raster_height)
        } else {
            star(d)
        };
        if let Some(br) = br {
            col = col.max(Vec4::new(br, br, br, 0.0));
        }
    }
    match air.and_then(|air| air.sky(d, (ray.camera_dot / cc.sqrt()) as f32)) {
        Some(air) => air.over_sky(col),
        None => col,
    }
}

/// The brightest star four rays across the pixel around `d` meet, in a raster
/// `raster_height` pixels tall. A motion frame's pixel is wider than a star's cell,
/// so its one ray met a star in one frame and missed it in the next: the stars
/// flickered while the view moved (user, 2026-11-16).
fn star_across_pixel(d: DVec3, raster_height: usize) -> Option<f32> {
    let quarter = 0.5 * VERTICAL_FOV_HALF_TAN / raster_height as f64;
    let across = d.any_orthonormal_vector();
    let up = d.cross(across);
    [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
        .into_iter()
        .filter_map(|(x, y)| star(d + (across * x + up * y) * quarter))
        .reduce(f32::max)
}

/// The brightness of the star in direction `d`'s cell of the sky, if it has one.
fn star(d: DVec3) -> Option<f32> {
    let hs = hash3(
        (d.x * STAR_GRID_RESOLUTION).floor() as i64,
        (d.y * STAR_GRID_RESOLUTION).floor() as i64,
        (d.z * STAR_GRID_RESOLUTION).floor() as i64,
    );
    hs.is_multiple_of(STAR_OCCURRENCE_MODULUS).then(|| {
        STAR_MIN_BRIGHTNESS
            + ((hs >> STAR_BRIGHTNESS_SHIFT) & STAR_BRIGHTNESS_MASK) as f32
                / STAR_BRIGHTNESS_DIVISOR
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::{camera::Cam, earth_model::EarthModel};

    #[test]
    fn a_motion_pixel_shows_its_star_wherever_the_star_lies_in_it() {
        // A star near straight up, where the cells lie square to the view.
        let cell = 1.0 / STAR_GRID_RESOLUTION;
        let centre = (0..200)
            .flat_map(|x| (0..200).map(move |y| (x, y)))
            .map(|(x, y)| {
                DVec3::new(x as f64 + 0.5, y as f64 + 0.5, STAR_GRID_RESOLUTION + 0.5) * cell
            })
            .find(|&d| star(d).is_some())
            .expect("a star among 40,000 cells");
        // A motion raster whose pixels are 1.8 cells wide: the star's cell is in
        // the pixel whenever the pixel's centre is within 0.8 cells of it.
        let height = (2.0 * VERTICAL_FOV_HALF_TAN * STAR_GRID_RESOLUTION / 1.8) as usize;
        let mut missed_by_one_ray = 0;
        for x in -4..=4 {
            for y in -4..=4 {
                let offset = DVec3::new(f64::from(x), f64::from(y), 0.0) * 0.2 * cell;
                assert!(star_across_pixel(centre + offset, height).is_some());
                missed_by_one_ray += usize::from(star(centre + offset).is_none());
            }
        }
        assert!(missed_by_one_ray > 0, "one ray would do");
    }

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
                stars: true,
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
            stars: true,
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
            stars: true,
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
