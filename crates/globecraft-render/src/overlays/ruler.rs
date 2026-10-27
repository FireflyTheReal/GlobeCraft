//! Great-circle ruler geometry shared by frontends.
use super::{Ground, ground_position};
use emath::{Pos2, Rect};
use globecraft_core::{
    camera::Basis,
    ruler::{Ruler, RulerPointId},
};
const ARC_SEGMENTS: usize = 48;
pub struct RulerLeg {
    pub segments: Vec<[Pos2; 2]>,
    pub label_position: Option<Pos2>,
    pub distance_km: f64,
    pub angle_degrees: f64,
}
/// The ruler's legs on screen, raised onto `ground` when relief is drawn.
pub fn project_ruler(
    basis: &Basis,
    rect: Rect,
    points: &Ruler,
    ground: Option<&dyn Ground>,
) -> Vec<RulerLeg> {
    let geometry = basis.model.geometry();
    let raised = |point: glam::DVec3| {
        let (lat, lon) = geometry.coordinates(point);
        ground_position(basis, ground, lat, lon)
    };
    points
        .paths()
        .flat_map(|path| path.array_windows::<2>())
        .map(|pair| {
            let projected: Vec<_> = basis
                .model
                .path(pair[0], pair[1], ARC_SEGMENTS)
                .into_iter()
                .map(|point| basis.project_precise(raised(point), rect))
                .collect();
            // Clipped in f64: a leg passing beside a tilted close-up camera projects
            // millions of points off screen (roadmap 1.9).
            let clip = rect.expand(super::SEGMENT_CLIP_MARGIN_POINTS);
            let segments = projected
                .array_windows::<2>()
                .filter_map(|p| match (p[0], p[1]) {
                    (Some(a), Some(b)) => super::clip_segment([a, b], clip),
                    _ => None,
                })
                .collect();
            let end = |(lat, lon): (f64, f64)| {
                basis.project_precise(ground_position(basis, ground, lat, lon), rect)
            };
            let label_position = end(pair[0])
                .zip(end(pair[1]))
                .map(|(a, b)| (a + b) / 2.0)
                .map(|mid| Pos2::new(mid.x as f32, mid.y as f32));
            RulerLeg {
                segments,
                label_position,
                distance_km: basis.model.distance_km(pair[0], pair[1]),
                angle_degrees: basis.model.central_angle_degrees(pair[0], pair[1]),
            }
        })
        .collect()
}

/// Nearest visible ruler handle within a screen-space hit radius.
pub fn point_at(
    basis: &Basis,
    rect: Rect,
    points: &Ruler,
    pointer: Pos2,
    radius_points: f32,
    ground: Option<&dyn Ground>,
) -> Option<RulerPointId> {
    points
        .paths()
        .enumerate()
        .flat_map(|(path, points)| {
            points
                .iter()
                .enumerate()
                .map(move |(point, position)| (RulerPointId { path, point }, position))
        })
        .filter_map(|(index, point)| {
            let screen = basis.project(ground_position(basis, ground, point.0, point.1), rect)?;
            let distance = screen.distance_sq(pointer);
            (rect.contains(screen) && distance <= radius_points * radius_points)
                .then_some((index, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emath::Vec2;
    use globecraft_core::camera::Cam;

    #[test]
    fn separate_paths_have_no_projected_leg_between_them() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 14000.0,
            tilt: 0.0,
        }
        .basis(1.0);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(600.0));
        let mut ruler = Ruler::default();
        ruler.add_point((0.0, 0.0), false);
        ruler.add_point((0.0, 1.0), false);
        ruler.add_point((20.0, 20.0), true);
        ruler.add_point((20.0, 21.0), false);
        let legs = project_ruler(&basis, rect, &ruler, None);
        assert_eq!(legs.len(), 2);
        assert_eq!(
            legs.iter().map(|leg| leg.distance_km).sum::<f64>(),
            ruler.total_distance_km()
        );
        let handle = basis
            .project(basis.surface_position(20.0, 21.0), rect)
            .unwrap();
        assert_eq!(
            point_at(&basis, rect, &ruler, handle, 10.0, None),
            Some(RulerPointId { path: 1, point: 1 })
        );
    }

    #[test]
    fn handles_are_nearest_visible_and_within_hit_radius() {
        let basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: 14000.0,
            tilt: 0.0,
        }
        .basis(1.0);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(600.0));
        let mut points = Ruler::default();
        for point in [(0.0, 0.0), (0.0, 2.0), (0.0, 180.0)] {
            points.add_point(point, false);
        }
        let second = basis
            .project(basis.surface_position(0.0, 2.0), rect)
            .unwrap();
        assert_eq!(
            point_at(&basis, rect, &points, second, 10.0, None),
            Some(RulerPointId { path: 0, point: 1 })
        );
        assert_eq!(
            point_at(&basis, rect, &points, Pos2::ZERO, 10.0, None),
            None
        );
        assert_eq!(
            point_at(
                &basis,
                rect,
                &{
                    let mut hidden = Ruler::default();
                    hidden.add_point((0.0, 180.0), false);
                    hidden
                },
                rect.center(),
                10.0,
                None
            ),
            None
        );
        assert_eq!(
            point_at(&basis, rect, &Ruler::default(), second, 10.0, None),
            None
        );
    }
}
