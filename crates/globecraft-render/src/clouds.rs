//! The live clouds as a layer of their own, `CLOUD_LAYER_KM` above the ground: lit by
//! the sun, dark at night, seen with the parallax of their height, and sampled from the
//! full 8192-wide map (user, 2026-11-18: "clouds are only 2K?", "place cloud layer at
//! ~8 km"). The app draws them as a textured mesh over the frame; this module works
//! out where each corner of that mesh lands on the layer and how lit it is.
use crate::tuning::{TWILIGHT_OFFSET, TWILIGHT_WIDTH};
use emath::{Pos2, Rect};
use glam::DVec3;
use globecraft_core::{
    EARTH_RADIUS_KM, VERTICAL_FOV_HALF_TAN, camera::Basis, earth_model::SurfaceGeometry,
    math::ll_to_v3,
};

/// The clouds' height above the surface. Real tops range from 2 to 15 km; one height
/// for all keeps them a layer over the ground.
pub const CLOUD_LAYER_KM: f64 = 8.0;
/// How opaque the thickest cloud is drawn, before the layer's own opacity.
pub const CLOUD_OPACITY: f32 = 0.78;
/// Close to the layer the clouds fade out: from none at this height of the eye above
/// the surface to all at the second. Below them the camera would be inside a layer
/// one texel (about 5 km) coarse, and mountains would show through.
const CLOUD_FADE_KM: (f64, f64) = (12.0, 30.0);
/// Toward the layer's horizon the clouds thin out: from none where the line of sight
/// meets the layer at this cosine down to all from the second. Seen edge-on they are
/// haze, and the mesh's last cells, cut where the line of sight misses the layer,
/// showed as steps against the sky.
const CLOUD_HORIZON_COSINES: (f64, f64) = (0.06, 0.25);
/// The clouds' light on the night side, as a share of daylight.
const CLOUD_NIGHT_LIGHT: f32 = 0.06;
/// A level of the cloud map is used while one of its texels covers at least this share
/// of a screen pixel at the view's centre: finer levels shimmer as the view moves (the
/// textures have no mipmaps).
const MIN_TEXEL_PER_PIXEL: f64 = 0.5;

/// How the clouds are lit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CloudLight {
    /// By the sun in this direction (Earth-fixed, unit).
    Sun(DVec3),
    /// Evenly, as the globe without Sunlight.
    Even,
    /// As at night everywhere (Night Lights without Sunlight).
    Night,
}

/// One corner of the clouds' mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudCorner {
    /// On the screen, in points.
    pub pos: Pos2,
    /// In the cloud map: 0 to 1 from 180° W eastward, 0 to 1 from the north pole.
    pub uv: [f64; 2],
    /// Its light, 0 (night) to 1 (day).
    pub light: f32,
    /// How much of the clouds show there, 0 to 1 (less toward the layer's horizon).
    pub opacity: f32,
}

/// The cloud layer as a grid of corners over the screen, row by row; `None` where
/// the line of sight misses the layer.
pub struct CloudShell {
    pub columns: usize,
    pub rows: usize,
    pub corners: Vec<Option<CloudCorner>>,
    /// How much of the layer shows at this height (0 to 1).
    pub fade: f32,
    /// Kilometres one screen pixel spans on the layer at the view's centre.
    pub pixel_km: f64,
}

/// The cloud layer seen from `basis` over `rect`, with a corner every `step` points
/// and `pixels_per_point` screen pixels per point; `None` when the camera is too
/// close to it to show it.
pub fn cloud_shell(
    basis: &Basis,
    rect: Rect,
    step: f32,
    pixels_per_point: f32,
    light: CloudLight,
) -> Option<CloudShell> {
    let shell = raised(basis.model.geometry(), CLOUD_LAYER_KM);
    let ground = basis.model.geometry();
    let (lat, lon) = ground.coordinates(basis.c);
    let height_km = (basis.c.length() - ground.position(lat, lon).length()) * EARTH_RADIUS_KM;
    let t = ((height_km - CLOUD_FADE_KM.0) / (CLOUD_FADE_KM.1 - CLOUD_FADE_KM.0)).clamp(0.0, 1.0);
    let fade = (t * t * (3.0 - 2.0 * t)) as f32;
    if fade <= 0.0 {
        return None;
    }
    let (columns, rows) = (
        (rect.width() / step).ceil().max(1.0) as usize,
        (rect.height() / step).ceil().max(1.0) as usize,
    );
    let corner = |pos: Pos2| {
        let ray = basis.ray(rect, pos);
        let hit = shell.intersect(basis.c, ray)?;
        let (lat, lon) = shell.coordinates(hit);
        let facing = (-ray.dot(ll_to_v3(lat, lon)) - CLOUD_HORIZON_COSINES.0)
            / (CLOUD_HORIZON_COSINES.1 - CLOUD_HORIZON_COSINES.0);
        let facing = facing.clamp(0.0, 1.0);
        let light = match light {
            CloudLight::Sun(sun) => {
                let k = ((ll_to_v3(lat, lon).dot(sun) + TWILIGHT_OFFSET) / TWILIGHT_WIDTH)
                    .clamp(0.0, 1.0) as f32;
                let k = k * k * (3.0 - 2.0 * k);
                CLOUD_NIGHT_LIGHT + (1.0 - CLOUD_NIGHT_LIGHT) * k
            }
            CloudLight::Even => 1.0,
            CloudLight::Night => CLOUD_NIGHT_LIGHT,
        };
        Some(CloudCorner {
            pos,
            uv: [(lon + 180.0) / 360.0, (90.0 - lat) / 180.0],
            light,
            opacity: (facing * facing * (3.0 - 2.0 * facing)) as f32,
        })
    };
    let corners = (0..=rows)
        .flat_map(|row| (0..=columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            corner(Pos2::new(
                rect.min.x + rect.width() * column as f32 / columns as f32,
                rect.min.y + rect.height() * row as f32 / rows as f32,
            ))
        })
        .collect();
    // At the view's centre: how far the layer is, and so how wide a pixel there.
    let centre_ray = basis.ray(rect, rect.center());
    let distance = shell
        .intersect(basis.c, centre_ray)
        .map_or(height_km / EARTH_RADIUS_KM, |hit| hit.distance(basis.c));
    let pixel_km = distance * EARTH_RADIUS_KM * 2.0 * VERTICAL_FOV_HALF_TAN
        / f64::from(rect.height() * pixels_per_point).max(1.0);
    Some(CloudShell {
        columns,
        rows,
        corners,
        fade,
        pixel_km,
    })
}

/// Which level of a cloud map `widths` wide (finest first) to draw a view whose
/// pixels span `pixel_km`: the finest whose texels are not much smaller than a pixel.
pub fn cloud_level(widths: &[u32], pixel_km: f64) -> usize {
    let circumference_km = std::f64::consts::TAU * EARTH_RADIUS_KM;
    widths
        .iter()
        .position(|&width| circumference_km / f64::from(width) >= MIN_TEXEL_PER_PIXEL * pixel_km)
        .unwrap_or(widths.len().saturating_sub(1))
}

/// `surface` raised `km` along its normal (a sphere's radius, an ellipsoid's axes).
fn raised(surface: SurfaceGeometry, km: f64) -> SurfaceGeometry {
    match surface {
        SurfaceGeometry::Sphere { radius_km } => SurfaceGeometry::Sphere {
            radius_km: radius_km + km,
        },
        SurfaceGeometry::Ellipsoid {
            semi_major_km,
            flattening,
        } => {
            let semi_minor_km = semi_major_km * (1.0 - flattening);
            SurfaceGeometry::Ellipsoid {
                semi_major_km: semi_major_km + km,
                flattening: 1.0 - (semi_minor_km + km) / (semi_major_km + km),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::{camera::Cam, earth_model::EarthModel};

    fn view(alt: f64, tilt: f64, model: EarthModel) -> Basis {
        Cam {
            lat: 30.0,
            lon: 179.0,
            heading: 0.0,
            alt,
            tilt,
        }
        .basis_for_model(1.5, model)
    }

    #[test]
    fn the_layer_lies_eight_kilometres_up_under_the_view() {
        for model in [EarthModel::PerfectSphere, EarthModel::Wgs84] {
            let basis = view(400.0, 0.0, model);
            let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(300.0, 200.0));
            let shell = cloud_shell(&basis, rect, 10.0, 1.0, CloudLight::Even).unwrap();
            assert_eq!(shell.fade, 1.0);
            assert_eq!(shell.corners.len(), 31 * 21);
            // Straight down from 400 km: the centre of the picture is over the
            // camera's point, 8 km above the ground.
            let centre = shell.corners[10 * 31 + 15].unwrap();
            assert!((centre.uv[0] - (179.0 + 180.0) / 360.0).abs() < 1e-6);
            assert!((centre.uv[1] - (90.0 - 30.0) / 180.0).abs() < 1e-6);
            assert_eq!(centre.opacity, 1.0);
            let ground = model.geometry();
            let hit = raised(ground, CLOUD_LAYER_KM)
                .intersect(basis.c, basis.ray(rect, centre.pos))
                .unwrap();
            let (lat, lon) = ground.coordinates(hit);
            let up_km = (hit.length() - ground.position(lat, lon).length()) * EARTH_RADIUS_KM;
            assert!((up_km - CLOUD_LAYER_KM).abs() < 0.05, "{model:?}: {up_km}");
            // 392 km to the layer, 200 pixels for 2·tan(22.5°) of it.
            let expected = 392.0 * 2.0 * VERTICAL_FOV_HALF_TAN / 200.0;
            assert!(
                (shell.pixel_km - expected).abs() < 0.5,
                "{}",
                shell.pixel_km
            );
        }
    }

    #[test]
    fn the_layer_fades_out_as_the_camera_comes_close() {
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(300.0, 200.0));
        let shell = |alt| {
            cloud_shell(
                &view(alt, 0.0, EarthModel::Wgs84),
                rect,
                50.0,
                1.0,
                CloudLight::Even,
            )
        };
        assert!(shell(10.0).is_none());
        let half = shell(21.0).unwrap().fade;
        assert!(0.3 < half && half < 0.7, "{half}");
        assert_eq!(shell(40.0).unwrap().fade, 1.0);
    }

    #[test]
    fn clouds_thin_out_toward_the_layers_horizon() {
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(300.0, 200.0));
        let shell = cloud_shell(
            &view(300.0, 80.0, EarthModel::Wgs84),
            rect,
            10.0,
            1.0,
            CloudLight::Even,
        )
        .unwrap();
        // Down the middle column from the bottom up: thinner and thinner, then none.
        let column: Vec<f32> = (0..=shell.rows)
            .rev()
            .filter_map(|row| shell.corners[row * (shell.columns + 1) + shell.columns / 2])
            .map(|corner| corner.opacity)
            .collect();
        assert_eq!(column[0], 1.0);
        assert!(
            column.windows(2).all(|pair| pair[1] <= pair[0]),
            "{column:?}"
        );
        assert!(*column.last().unwrap() < 0.05, "{column:?}");
    }

    #[test]
    fn clouds_are_lit_by_day_and_dark_by_night() {
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(300.0, 200.0));
        let basis = view(1000.0, 0.0, EarthModel::Wgs84);
        let centre = |light| {
            cloud_shell(&basis, rect, 100.0, 1.0, light)
                .unwrap()
                .corners[4]
                .unwrap()
                .light
        };
        let overhead = ll_to_v3(30.0, 179.0);
        assert_eq!(centre(CloudLight::Sun(overhead)), 1.0);
        assert_eq!(centre(CloudLight::Sun(-overhead)), CLOUD_NIGHT_LIGHT);
        assert_eq!(centre(CloudLight::Night), CLOUD_NIGHT_LIGHT);
        assert_eq!(centre(CloudLight::Even), 1.0);
    }

    #[test]
    fn far_views_use_coarser_levels() {
        let widths = [8192, 4096, 2048];
        // A texel of the full map is about 4.9 km.
        assert_eq!(cloud_level(&widths, 1.0), 0);
        assert_eq!(cloud_level(&widths, 9.0), 0);
        assert_eq!(cloud_level(&widths, 12.0), 1);
        assert_eq!(cloud_level(&widths, 30.0), 2);
        assert_eq!(cloud_level(&widths, 1000.0), 2);
    }
}
