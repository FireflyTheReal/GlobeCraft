//! The live clouds as a layer of their own, `CLOUD_LAYER_KM` above the ground: lit by
//! the sun, dark at night, seen with the parallax of their height, and sampled from the
//! full 8192-wide map (user, 2026-11-18: "clouds are only 2K?", "place cloud layer at
//! ~8 km"). Seen from above, the app draws them as a textured mesh over the frame; this
//! module works out where each corner of that mesh lands on the layer and how lit it
//! is. Seen from below, the renderer draws them into the frame's sky (`from_below`),
//! where the relief hides them as it should.
use crate::tuning::{TWILIGHT_OFFSET, TWILIGHT_WIDTH};
use emath::{Pos2, Rect};
use glam::{DVec3, Vec3};
use globecraft_core::{
    EARTH_RADIUS_KM, VERTICAL_FOV_HALF_TAN, camera::Basis, earth_model::SurfaceGeometry,
    math::ll_to_v3,
};
use globecraft_data::live_maps::CloudMap;
use std::sync::Arc;

/// The clouds' height above the surface. Real tops range from 2 to 15 km; one height
/// for all keeps them a layer over the ground: over Everest (8.8 km), and under
/// airliners at cruise, which show above them (user, 2026-11-18).
pub const CLOUD_LAYER_KM: f64 = 10.0;
/// How opaque the thickest cloud is drawn, before the layer's own opacity.
pub const CLOUD_OPACITY: f32 = 0.78;
/// Close to the layer the clouds seen from above fade out: from none at this height
/// of the eye above the surface to all at the second. It meets the view from below
/// (`BELOW_FADE_KM`), so that there are clouds at every height.
const CLOUD_FADE_KM: (f64, f64) = (CLOUD_LAYER_KM + 0.5, CLOUD_LAYER_KM + 12.0);
/// Toward the layer's horizon the clouds thin out: from none where the line of sight
/// meets the layer at this cosine down to all from the second. Seen edge-on they are
/// haze, and the mesh's last cells, cut where the line of sight misses the layer,
/// showed as steps against the sky.
const CLOUD_HORIZON_COSINES: (f64, f64) = (0.06, 0.25);
/// The clouds' colour on the night side: faint and bluish, as under the moon and the
/// sky's own glow. Black, they were lost on the dark ground (user, 2026-11-18).
const CLOUD_NIGHT_COLOUR: Vec3 = Vec3::new(0.16, 0.18, 0.24);
/// A level of the cloud map is used while one of its texels covers at least this share
/// of a screen pixel at the view's centre: finer levels shimmer as the view moves (the
/// textures have no mipmaps).
const MIN_TEXEL_PER_PIXEL: f64 = 0.5;
/// Cells of the mesh toward the layer's horizon (where it shows less than whole) are
/// cut into cells this many points wide: the layer's picture bends there, and laid
/// straight across larger cells it showed in blocks against the sky (user's
/// recording, 2026-11-19).
const HORIZON_STEP_POINTS: f32 = 4.0;
/// Cells whose corners' light differs by more than this (a colour channel, 0 to 1) lie
/// across the day/night edge: they are cut as finely, or the twilight, narrower than
/// a moving view's cells, showed in steps (user, 2026-11-19).
const TERMINATOR_SPLIT_LIGHT: f32 = 0.04;

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
    /// Its colour, 0 to 1 (white by day).
    pub colour: [f32; 3],
    /// How much of the clouds show there, 0 to 1 (less toward the layer's horizon).
    pub opacity: f32,
}

/// The cloud layer as a grid of corners over the screen, row by row; `None` where
/// the line of sight leads away from the layer (clear corners where it passes it).
pub struct CloudShell {
    pub columns: usize,
    pub rows: usize,
    pub corners: Vec<Option<CloudCorner>>,
    /// The mesh's cells, each four corners round it (top left, top right, bottom
    /// right, bottom left): the grid's, finer toward the layer's horizon.
    pub cells: Vec<[CloudCorner; 4]>,
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
    let height_km = eye_height_km(basis);
    let fade = above_fade(height_km);
    if fade <= 0.0 {
        return None;
    }
    let grid = layer_grid(basis, rect, step, CLOUD_LAYER_KM);
    let to_cloud = |corner: LayerCorner| CloudCorner {
        pos: corner.pos,
        uv: [(corner.lon + 180.0) / 360.0, (90.0 - corner.lat) / 180.0],
        colour: cloud_colour(light, ll_to_v3(corner.lat, corner.lon)).to_array(),
        opacity: corner.facing,
    };
    let corners: Vec<Option<CloudCorner>> = grid
        .corners
        .iter()
        .map(|corner| corner.map(to_cloud))
        .collect();
    let row = grid.columns + 1;
    let quad = |x: usize, y: usize| -> Option<[CloudCorner; 4]> {
        let [a, b, c, d] = [
            y * row + x,
            y * row + x + 1,
            (y + 1) * row + x + 1,
            (y + 1) * row + x,
        ]
        .map(|index| corners[index]);
        Some([a?, b?, c?, d?])
    };
    let sight = LayerSight::new(basis, rect, CLOUD_LAYER_KM);
    let pieces = (step / HORIZON_STEP_POINTS).ceil().max(1.0) as usize;
    let cells: Vec<[CloudCorner; 4]> = crate::on_interface_pool(|| {
        use rayon::prelude::*;
        (0..grid.rows * grid.columns)
            .into_par_iter()
            .flat_map_iter(|index| {
                let (x, y) = (index % grid.columns, index / grid.columns);
                let whole = quad(x, y);
                let toward_horizon = whole.is_some_and(|quad| quad.iter().any(|c| c.opacity < 1.0));
                let across_twilight = whole.is_some_and(|quad| {
                    let light = |c: &CloudCorner| Vec3::from(c.colour);
                    let (low, high) = quad.iter().fold(
                        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                        |(low, high), c| (low.min(light(c)), high.max(light(c))),
                    );
                    (high - low).max_element() > TERMINATOR_SPLIT_LIGHT
                });
                if pieces == 1 || !(toward_horizon || across_twilight) {
                    return whole.into_iter().collect::<Vec<_>>();
                }
                // Cut into pieces, each corner found again.
                let [top_left, _, bottom_right, _] = whole.expect("checked above");
                let (from, to) = (top_left.pos, bottom_right.pos);
                let at = |i: usize, j: usize| {
                    sight
                        .corner(Pos2::new(
                            from.x + (to.x - from.x) * i as f32 / pieces as f32,
                            from.y + (to.y - from.y) * j as f32 / pieces as f32,
                        ))
                        .map(to_cloud)
                };
                let fine: Vec<Option<CloudCorner>> = (0..=pieces)
                    .flat_map(|j| (0..=pieces).map(move |i| (i, j)))
                    .map(|(i, j)| at(i, j))
                    .collect();
                let side = pieces + 1;
                (0..pieces * pieces)
                    .filter_map(|piece| {
                        let (i, j) = (piece % pieces, piece / pieces);
                        Some([
                            fine[j * side + i]?,
                            fine[j * side + i + 1]?,
                            fine[(j + 1) * side + i + 1]?,
                            fine[(j + 1) * side + i]?,
                        ])
                    })
                    .collect()
            })
            .collect()
    });
    // At the view's centre: how far the layer is, and so how wide a pixel there.
    let shell = raised(basis.model.geometry(), CLOUD_LAYER_KM);
    let distance = shell
        .intersect(basis.c, basis.ray(rect, rect.center()))
        .map_or(height_km / EARTH_RADIUS_KM, |hit| hit.distance(basis.c));
    let pixel_km = distance * EARTH_RADIUS_KM * 2.0 * VERTICAL_FOV_HALF_TAN
        / f64::from(rect.height() * pixels_per_point).max(1.0);
    Some(CloudShell {
        columns: grid.columns,
        rows: grid.rows,
        corners,
        cells,
        fade,
        pixel_km,
    })
}

/// How high the eye of `basis` is above the surface (km).
pub fn eye_height_km(basis: &Basis) -> f64 {
    let ground = basis.model.geometry();
    let (lat, lon) = ground.coordinates(basis.c);
    (basis.c.length() - ground.position(lat, lon).length()) * EARTH_RADIUS_KM
}

/// Where a line of sight meets a layer over the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerCorner {
    /// On the screen, in points.
    pub pos: Pos2,
    pub lat: f64,
    pub lon: f64,
    /// How much of the layer shows there, 0 to 1: less toward its horizon, where it
    /// is seen edge-on, and none where the line of sight passes by it.
    pub facing: f32,
}

/// Lines of sight over the screen, one every `step` points, and where they meet a
/// layer `height_km` above the surface, from above or from below.
pub struct LayerGrid {
    pub columns: usize,
    pub rows: usize,
    /// Row by row; `None` where the line of sight leads away from the layer, or
    /// meets the ground first.
    pub corners: Vec<Option<LayerCorner>>,
}

/// Lines of sight from a view to a layer over the ground.
pub struct LayerSight<'a> {
    basis: &'a Basis,
    rect: Rect,
    ground: SurfaceGeometry,
    shell: SurfaceGeometry,
    below: bool,
}

impl<'a> LayerSight<'a> {
    pub fn new(basis: &'a Basis, rect: Rect, height_km: f64) -> Self {
        let ground = basis.model.geometry();
        Self {
            basis,
            rect,
            ground,
            shell: raised(ground, height_km),
            below: eye_height_km(basis) < height_km,
        }
    }

    /// Where the line of sight through `pos` (points) meets the layer; `None` where
    /// it leads away from it, or meets the ground first.
    pub fn corner(&self, pos: Pos2) -> Option<LayerCorner> {
        let (basis, ground, shell) = (self.basis, self.ground, self.shell);
        let ray = basis.ray(self.rect, pos);
        let Some(hit) = shell.intersect(basis.c, ray) else {
            // A line of sight past the layer's edge still makes a corner, a clear one
            // where it passes nearest: the cells across the edge fade out instead of
            // being cut off in steps.
            let nearest = -basis.c.dot(ray);
            if nearest <= 0.0 {
                return None;
            }
            let (lat, lon) = shell.coordinates(basis.c + ray * nearest);
            return Some(LayerCorner {
                pos,
                lat,
                lon,
                facing: 0.0,
            });
        };
        // From below, a line toward the ground meets the layer only on the far side
        // of the Earth.
        if self.below
            && ground
                .intersect(basis.c, ray)
                .is_some_and(|floor| floor.distance(basis.c) < hit.distance(basis.c))
        {
            return None;
        }
        let (lat, lon) = shell.coordinates(hit);
        let facing = ((ray.dot(ll_to_v3(lat, lon)).abs() - CLOUD_HORIZON_COSINES.0)
            / (CLOUD_HORIZON_COSINES.1 - CLOUD_HORIZON_COSINES.0))
            .clamp(0.0, 1.0);
        Some(LayerCorner {
            pos,
            lat,
            lon,
            facing: (facing * facing * (3.0 - 2.0 * facing)) as f32,
        })
    }
}

/// The layer `height_km` up seen from `basis` over `rect`, a corner every `step`
/// points.
pub fn layer_grid(basis: &Basis, rect: Rect, step: f32, height_km: f64) -> LayerGrid {
    let sight = LayerSight::new(basis, rect, height_km);
    let (columns, rows) = (
        (rect.width() / step).ceil().max(1.0) as usize,
        (rect.height() / step).ceil().max(1.0) as usize,
    );
    let corner = |pos: Pos2| sight.corner(pos);
    // Some 30,000 lines of sight for a large window at rest: 7.5 ms on one thread. On
    // the interface's own pool: on the shared one this waited behind the rows of a
    // frame being drawn, up to 140 ms (2026-11-18). A moving view takes a coarser grid.
    let corners = crate::on_interface_pool(|| {
        use rayon::prelude::*;
        (0..(rows + 1) * (columns + 1))
            .into_par_iter()
            .map(|index| {
                let (column, row) = (index % (columns + 1), index / (columns + 1));
                corner(Pos2::new(
                    rect.min.x + rect.width() * column as f32 / columns as f32,
                    rect.min.y + rect.height() * row as f32 / rows as f32,
                ))
            })
            .collect()
    });
    LayerGrid {
        columns,
        rows,
        corners,
    }
}

/// The clouds' colour where the layer's vertical is `up`.
fn cloud_colour(light: CloudLight, up: DVec3) -> Vec3 {
    match light {
        CloudLight::Sun(sun) => {
            let k = ((up.dot(sun) + TWILIGHT_OFFSET) / TWILIGHT_WIDTH).clamp(0.0, 1.0) as f32;
            CLOUD_NIGHT_COLOUR.lerp(Vec3::ONE, k * k * (3.0 - 2.0 * k))
        }
        CloudLight::Even => Vec3::ONE,
        CloudLight::Night => CLOUD_NIGHT_COLOUR,
    }
}

/// The clouds for the renderer, which draws them into the sky when the camera is
/// below them.
#[derive(Clone)]
pub struct CloudsCfg {
    pub map: Arc<CloudMap>,
    /// The layer's opacity (0 to 1).
    pub opacity: f32,
    pub light: CloudLight,
}

/// Below the layer the clouds fade in: from none with the eye this far under it (km)
/// to all at the second, so that they do not appear at once as the camera comes down
/// through them. Above it, the view from above fades in (`CLOUD_FADE_KM`).
const BELOW_FADE_KM: (f64, f64) = (0.0, 1.0);
/// Seen from below, distant clouds fade into the sky: by e every this many km.
const BELOW_HAZE_KM: f64 = 150.0;
/// The underside of thick cloud is grey: the thickest is this much darker.
const UNDERSIDE_SHADE: f32 = 0.45;

/// The clouds seen from `eye` (in the surface's units; `height_km` above it) along the
/// sky ray `direction`: their colour (0 to 1) and opacity, or `None` without any.
pub(crate) fn from_below(
    clouds: &CloudsCfg,
    surface: SurfaceGeometry,
    eye: DVec3,
    height_km: f64,
    direction: DVec3,
) -> Option<(Vec3, f32)> {
    let under = below_fade(height_km);
    if under <= 0.0 {
        return None;
    }
    let layer = raised(surface, CLOUD_LAYER_KM);
    let hit = layer.intersect(eye, direction)?;
    let (lat, lon) = layer.coordinates(hit);
    let density = sample(clouds.map.levels.first()?, lat, lon);
    if density <= 0.0 {
        return None;
    }
    let far = hit.distance(eye) * EARTH_RADIUS_KM / BELOW_HAZE_KM;
    let opacity = density * CLOUD_OPACITY * clouds.opacity * under * (-far).exp() as f32;
    let colour = cloud_colour(clouds.light, ll_to_v3(lat, lon)) * (1.0 - UNDERSIDE_SHADE * density);
    Some((colour, opacity))
}

/// How much of the clouds seen from above shows with the eye `height_km` up (0 to 1).
fn above_fade(height_km: f64) -> f32 {
    smooth((height_km - CLOUD_FADE_KM.0) / (CLOUD_FADE_KM.1 - CLOUD_FADE_KM.0))
}

/// How much of the clouds seen from below shows with the eye `height_km` up (0 to 1).
fn below_fade(height_km: f64) -> f32 {
    smooth((CLOUD_LAYER_KM - height_km - BELOW_FADE_KM.0) / (BELOW_FADE_KM.1 - BELOW_FADE_KM.0))
}

/// 0 to 1, eased at both ends.
fn smooth(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

/// How much of a thing `km` above the surface at a place shows through the clouds
/// seen from `basis` (0 to 1): all on the eye's side of the layer, less behind thick
/// cloud on the other. Aircraft below the layer hide under the clouds seen from above,
/// those above them from below (user, 2026-11-18).
pub fn seen_through(clouds: &CloudsCfg, basis: &Basis, lat: f64, lon: f64, km: f64) -> f32 {
    let eye_km = eye_height_km(basis);
    let eye_above = eye_km > CLOUD_LAYER_KM;
    if eye_above == (km > CLOUD_LAYER_KM) {
        return 1.0;
    }
    let fade = if eye_above {
        above_fade(eye_km)
    } else {
        below_fade(eye_km)
    };
    let layer = raised(basis.model.geometry(), CLOUD_LAYER_KM);
    let thing = basis.raised_position(lat, lon, km);
    let Some(crossing) = layer.intersect(basis.c, (thing - basis.c).normalize()) else {
        return 1.0;
    };
    let (lat, lon) = layer.coordinates(crossing);
    let density = clouds
        .map
        .levels
        .first()
        .map_or(0.0, |map| sample(map, lat, lon));
    1.0 - density * CLOUD_OPACITY * clouds.opacity * fade
}

/// An equirectangular map's value (0 to 1) at a place, between its four nearest
/// texels.
pub(crate) fn sample(map: &image::GrayImage, lat: f64, lon: f64) -> f32 {
    let (width, height) = (map.width() as f64, map.height() as f64);
    let x = (lon + 180.0) / 360.0 * width - 0.5;
    let y = ((90.0 - lat) / 180.0 * height - 0.5).clamp(0.0, height - 1.0);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let at = |x: f64, y: f64| {
        let x = x.rem_euclid(width) as u32;
        let y = y.min(height - 1.0) as u32;
        f32::from(map.get_pixel(x, y)[0]) / 255.0
    };
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
    let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
    top * (1.0 - fy) + bottom * fy
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
            look_up: 0.0,
        }
        .basis_for_model(1.5, model)
    }

    #[test]
    fn the_layer_lies_at_its_height_under_the_view() {
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
            // 390 km to the layer, 200 pixels for 2·tan(22.5°) of it.
            let expected = 390.0 * 2.0 * VERTICAL_FOV_HALF_TAN / 200.0;
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
        let half = shell(16.25).unwrap().fade;
        assert!(0.3 < half && half < 0.7, "{half}");
        assert_eq!(shell(25.0).unwrap().fade, 1.0);
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
    fn the_mesh_is_finer_toward_the_layers_horizon() {
        // The whole globe from 12,000 km: its edge crosses the screen.
        let basis = view(12_000.0, 0.0, EarthModel::Wgs84);
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(600.0, 400.0));
        let shell = cloud_shell(&basis, rect, 16.0, 1.0, CloudLight::Even).unwrap();
        let width = |cell: &[CloudCorner; 4]| cell[1].pos.x - cell[0].pos.x;
        let whole = shell.cells.iter().filter(|cell| width(cell) > 15.0).count();
        let fine: Vec<_> = shell
            .cells
            .iter()
            .filter(|cell| width(cell) <= HORIZON_STEP_POINTS + 0.01)
            .collect();
        assert!(
            whole > 0 && !fine.is_empty(),
            "{whole} whole, {} fine",
            fine.len()
        );
        // The whole cells show the layer whole: those toward its edge were cut finer.
        assert!(
            shell
                .cells
                .iter()
                .filter(|cell| width(cell) > 15.0)
                .all(|cell| cell.iter().all(|c| c.opacity == 1.0))
        );
    }

    #[test]
    fn the_mesh_is_finer_across_the_day_and_night_edge() {
        // From 3,000 km over the evening edge: the sun sets 90° west of the view.
        let basis = view(3_000.0, 0.0, EarthModel::Wgs84);
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(600.0, 400.0));
        let sun = ll_to_v3(0.0, -90.0);
        let shell = cloud_shell(&basis, rect, 16.0, 1.0, CloudLight::Sun(sun)).unwrap();
        let width = |cell: &[CloudCorner; 4]| cell[1].pos.x - cell[0].pos.x;
        let light = |c: &CloudCorner| Vec3::from(c.colour);
        let fine = shell
            .cells
            .iter()
            .filter(|cell| width(cell) <= HORIZON_STEP_POINTS + 0.01)
            .count();
        assert!(fine > 0, "the edge is cut finer");
        // No cell left whole spans the edge.
        assert!(
            shell
                .cells
                .iter()
                .filter(|cell| width(cell) > 15.0)
                .all(|cell| {
                    cell.iter().all(|c| {
                        (light(c) - light(&cell[0])).abs().max_element() <= TERMINATOR_SPLIT_LIGHT
                    })
                })
        );
        // Lit evenly, nothing is cut but toward the layer's horizon (none here).
        let even = cloud_shell(&basis, rect, 16.0, 1.0, CloudLight::Even).unwrap();
        assert!(even.cells.iter().all(|cell| width(cell) > 15.0));
    }

    #[test]
    fn clouds_are_lit_by_day_and_faint_but_seen_by_night() {
        let rect = Rect::from_min_size(Pos2::ZERO, emath::vec2(300.0, 200.0));
        let basis = view(1000.0, 0.0, EarthModel::Wgs84);
        let centre = |light| {
            Vec3::from(
                cloud_shell(&basis, rect, 100.0, 1.0, light)
                    .unwrap()
                    .corners[4]
                    .unwrap()
                    .colour,
            )
        };
        let overhead = ll_to_v3(30.0, 179.0);
        assert_eq!(centre(CloudLight::Sun(overhead)), Vec3::ONE);
        assert_eq!(centre(CloudLight::Sun(-overhead)), CLOUD_NIGHT_COLOUR);
        assert_eq!(centre(CloudLight::Night), CLOUD_NIGHT_COLOUR);
        assert_eq!(centre(CloudLight::Even), Vec3::ONE);
        // Faint, but well over a night ground's dark.
        assert!(CLOUD_NIGHT_COLOUR.min_element() > 0.1);
    }

    #[test]
    fn from_below_the_clouds_overhead_show_and_fade_into_the_distance() {
        let surface = EarthModel::Wgs84.geometry();
        let mut map = image::GrayImage::from_pixel(64, 32, image::Luma([255]));
        // A clear band along the equator.
        for x in 0..64 {
            map.put_pixel(x, 15, image::Luma([0]));
            map.put_pixel(x, 16, image::Luma([0]));
        }
        let clouds = CloudsCfg {
            map: Arc::new(CloudMap { levels: vec![map] }),
            opacity: 1.0,
            light: CloudLight::Even,
        };
        let at = |lat: f64, height_km: f64, direction: DVec3| {
            let ground = surface.position(lat, 10.0);
            let eye = ground + ll_to_v3(lat, 10.0) * (height_km / EARTH_RADIUS_KM);
            from_below(&clouds, surface, eye, height_km, direction)
        };
        let up = ll_to_v3(45.0, 10.0);
        // Straight up from 1 km under thick cloud: grey, nearly as opaque as drawn.
        let (colour, opacity) = at(45.0, 1.0, up).unwrap();
        assert!(
            (colour.x - (1.0 - UNDERSIDE_SHADE)).abs() < 1e-3,
            "{colour}"
        );
        assert!(opacity > 0.65, "{opacity}");
        // Toward the horizon they fade into the sky.
        let east = DVec3::new(-10f64.to_radians().sin(), 10f64.to_radians().cos(), 0.0);
        let low = (east + up * 0.05).normalize();
        assert!(at(45.0, 1.0, low).unwrap().1 < 0.5 * opacity);
        // None in the clear band, and none from above the layer.
        assert!(at(0.0, 1.0, ll_to_v3(0.0, 10.0)).is_none());
        assert!(at(45.0, 11.0, up).is_none());
    }

    #[test]
    fn aircraft_hide_behind_thick_cloud_on_the_layers_other_side() {
        let clouds = CloudsCfg {
            map: Arc::new(CloudMap {
                levels: vec![image::GrayImage::from_pixel(64, 32, image::Luma([255]))],
            }),
            opacity: 1.0,
            light: CloudLight::Even,
        };
        let (lat, lon) = (30.0, 179.0);
        let from_above = view(400.0, 0.0, EarthModel::Wgs84);
        let below = seen_through(&clouds, &from_above, lat, lon, 3.0);
        assert!((below - (1.0 - CLOUD_OPACITY)).abs() < 1e-3, "{below}");
        assert_eq!(seen_through(&clouds, &from_above, lat, lon, 11.5), 1.0);
        // From under the layer it is the other way round.
        let from_below = view(1.0, 0.0, EarthModel::Wgs84);
        assert_eq!(seen_through(&clouds, &from_below, lat, lon, 3.0), 1.0);
        assert!(seen_through(&clouds, &from_below, lat, lon, 11.5) < 0.3);
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
