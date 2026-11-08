//! Independent surface ruler paths; input and drawing stay in frontends.
#[cfg(test)]
use crate::math::haversine_km;
use std::collections::VecDeque;
const MAX_HISTORY_STEPS: usize = 100;
type RulerPaths = Vec<Vec<(f64, f64)>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulerPointId {
    pub path: usize,
    pub point: usize,
}

#[derive(Default)]
pub struct Ruler {
    paths: RulerPaths,
    undo: VecDeque<RulerPaths>,
    redo: Vec<RulerPaths>,
    move_before: Option<RulerPaths>,
    /// Paths of three points or more are closed shapes: their last point joins
    /// their first, and their area is measured (roadmap 1.10, "measuring area").
    closed: bool,
}

impl Ruler {
    pub fn add_point(&mut self, position: (f64, f64), start_path: bool) {
        self.end_move();
        self.record(self.paths.clone());
        if self
            .paths
            .last()
            .is_none_or(|path| start_path && !path.is_empty())
        {
            self.paths.push(Vec::new());
        }
        self.paths.last_mut().unwrap().push(position);
    }
    pub fn paths(&self) -> impl Iterator<Item = &[(f64, f64)]> {
        self.paths.iter().map(Vec::as_slice)
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn set_closed(&mut self, closed: bool) {
        self.closed = closed;
    }
    /// Whether a path is drawn and measured as a closed shape.
    fn shape(&self, path: &[(f64, f64)]) -> bool {
        self.closed && path.len() >= 3
    }
    /// The paths as measured: a closed shape ends back at its first point.
    pub fn measured_paths(&self) -> Vec<Vec<(f64, f64)>> {
        self.paths
            .iter()
            .map(|path| {
                let mut measured = path.clone();
                if self.shape(path) {
                    measured.push(path[0]);
                }
                measured
            })
            .collect()
    }
    /// Every leg measured, the closing legs of shapes included.
    pub fn legs(&self) -> Vec<[(f64, f64); 2]> {
        self.measured_paths()
            .iter()
            .flat_map(|path| path.array_windows::<2>().copied().collect::<Vec<_>>())
            .collect()
    }
    /// The area enclosed by the closed shapes, km² (on the model's surface: the
    /// ellipsoid's geodesic polygons, or the sphere's). `None` when nothing is closed.
    pub fn area_km2(&self, model: crate::earth_model::EarthModel) -> Option<f64> {
        use geographiclib_rs::{Geodesic, PolygonArea, Winding};
        let geodesic = match model {
            crate::earth_model::EarthModel::Wgs84 => Geodesic::wgs84(),
            crate::earth_model::EarthModel::PerfectSphere => {
                Geodesic::new(crate::EARTH_RADIUS_KM * 1000.0, 0.0)
            }
        };
        let shapes: Vec<&Vec<(f64, f64)>> =
            self.paths.iter().filter(|path| self.shape(path)).collect();
        if shapes.is_empty() {
            return None;
        }
        let area_m2: f64 = shapes
            .iter()
            .map(|path| {
                let mut polygon = PolygonArea::new(&geodesic, Winding::CounterClockwise);
                for (lat, lon) in path.iter() {
                    polygon.add_point(*lat, *lon);
                }
                // Signed, so a shape drawn clockwise is not the rest of the Earth.
                polygon.compute(true).1.abs()
            })
            .sum();
        Some(area_m2 / 1e6)
    }
    pub fn points(&self) -> impl Iterator<Item = &(f64, f64)> {
        self.paths.iter().flatten()
    }
    pub fn point_count(&self) -> usize {
        self.paths.iter().map(Vec::len).sum()
    }
    pub fn point(&self, id: RulerPointId) -> Option<(f64, f64)> {
        self.paths.get(id.path)?.get(id.point).copied()
    }
    /// Capture a whole pointer drag as one edit, without cloning on each movement.
    pub fn begin_move(&mut self) {
        self.end_move();
        self.move_before = Some(self.paths.clone());
    }
    pub fn move_point(&mut self, id: RulerPointId, position: (f64, f64)) {
        if self
            .paths
            .get(id.path)
            .and_then(|path| path.get(id.point))
            .is_none_or(|old| *old == position)
        {
            return;
        }
        if self.move_before.is_none() {
            self.record(self.paths.clone());
        }
        self.paths[id.path][id.point] = position;
    }
    pub fn end_move(&mut self) {
        if let Some(before) = self.move_before.take()
            && before != self.paths
        {
            self.record(before);
        }
    }
    fn record(&mut self, before: RulerPaths) {
        if self.undo.len() == MAX_HISTORY_STEPS {
            self.undo.pop_front();
        }
        self.undo.push_back(before);
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
            || self
                .move_before
                .as_ref()
                .is_some_and(|before| *before != self.paths)
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
            && self
                .move_before
                .as_ref()
                .is_none_or(|before| *before == self.paths)
    }
    pub fn undo(&mut self) -> bool {
        self.end_move();
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.paths, previous));
        true
    }
    pub fn redo(&mut self) -> bool {
        self.end_move();
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push_back(std::mem::replace(&mut self.paths, next));
        true
    }
    pub fn remove_point(&mut self, id: RulerPointId) {
        self.end_move();
        if self
            .paths
            .get(id.path)
            .and_then(|path| path.get(id.point))
            .is_none()
        {
            return;
        }
        self.record(self.paths.clone());
        if let Some(path) = self.paths.get_mut(id.path)
            && id.point < path.len()
        {
            path.remove(id.point);
            // Retain an empty active path so the next click cannot silently
            // reconnect to an older measurement after deleting its last point.
            if path.is_empty() && id.path + 1 < self.paths.len() {
                self.paths.remove(id.path);
            }
        }
    }
    pub fn clear(&mut self) {
        self.end_move();
        if self.paths.is_empty() {
            return;
        }
        let before = std::mem::take(&mut self.paths);
        self.record(before);
    }
    pub fn angular_distance_degrees(&self, model: crate::earth_model::EarthModel) -> f64 {
        self.legs()
            .iter()
            .map(|pair| model.central_angle_degrees(pair[0], pair[1]))
            .sum()
    }
    pub fn total_distance_km(&self) -> f64 {
        self.distance_km(crate::earth_model::EarthModel::PerfectSphere)
    }
    pub fn distance_km(&self, model: crate::earth_model::EarthModel) -> f64 {
        self.legs()
            .iter()
            .map(|pair| model.distance_km(pair[0], pair[1]))
            .sum()
    }
    pub fn last_leg(&self) -> Option<[(f64, f64); 2]> {
        let path = self.paths.last()?;
        let pair = path.array_windows::<2>().last()?;
        Some(*pair)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_closed_shape_measures_its_closing_leg_and_its_area() {
        use crate::earth_model::EarthModel;
        let mut ruler = Ruler::default();
        for point in [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)] {
            ruler.add_point(point, false);
        }
        let open = ruler.distance_km(EarthModel::PerfectSphere);
        assert!(ruler.area_km2(EarthModel::Wgs84).is_none());
        ruler.set_closed(true);
        let closed = ruler.distance_km(EarthModel::PerfectSphere);
        assert!((closed - open - 111.195080234).abs() < 1e-6);
        // A one-degree square at the equator: 12,308 km² on WGS 84.
        let area = ruler.area_km2(EarthModel::Wgs84).unwrap();
        assert!((area - 12_308.8).abs() < 1.0, "{area}");
        // Drawn the other way round, the same area (not the rest of the Earth).
        let mut reverse = Ruler::default();
        for point in [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)] {
            reverse.add_point(point, false);
        }
        reverse.set_closed(true);
        assert!((reverse.area_km2(EarthModel::Wgs84).unwrap() - area).abs() < 1e-6);
        // Two points make no shape.
        let mut line = Ruler::default();
        line.add_point((0.0, 0.0), false);
        line.add_point((0.0, 1.0), false);
        line.set_closed(true);
        assert!(line.area_km2(EarthModel::Wgs84).is_none());
    }
    #[test]
    fn model_switch_remeasures_existing_paths_without_changing_history() {
        use crate::earth_model::EarthModel;
        let mut ruler = Ruler::default();
        ruler.add_point((0.0, 0.0), false);
        ruler.add_point((0.0, 1.0), false);
        let sphere = ruler.distance_km(EarthModel::PerfectSphere);
        let realistic = ruler.distance_km(EarthModel::Wgs84);
        assert!((sphere - 111.195080234).abs() < 1e-8);
        assert!((realistic - 111.319490793).abs() < 1e-8);
        assert!((ruler.angular_distance_degrees(EarthModel::Wgs84) - 1.0).abs() < 1e-12);
        assert!(ruler.undo());
        assert_eq!(ruler.point_count(), 1);
        assert_eq!(ruler.distance_km(EarthModel::Wgs84), 0.0);
    }
    #[test]
    fn undo_redo_restore_add_delete_clear_and_disconnected_paths() {
        let mut ruler = Ruler::default();
        let snapshot = |ruler: &Ruler| ruler.paths().map(<[_]>::to_vec).collect::<Vec<_>>();
        let mut states = vec![snapshot(&ruler)];
        assert!(!ruler.can_undo() && !ruler.can_redo());
        ruler.add_point((0.0, 0.0), false);
        states.push(snapshot(&ruler));
        ruler.add_point((0.0, 1.0), false);
        states.push(snapshot(&ruler));
        ruler.add_point((20.0, 30.0), true);
        states.push(snapshot(&ruler));
        ruler.remove_point(RulerPointId { path: 1, point: 0 });
        states.push(snapshot(&ruler));
        ruler.clear();
        states.push(snapshot(&ruler));
        for state in states[..states.len() - 1].iter().rev() {
            assert!(ruler.can_undo());
            assert!(ruler.undo());
            assert_eq!(&snapshot(&ruler), state);
        }
        assert!(!ruler.undo());
        for state in &states[1..] {
            assert!(ruler.can_redo());
            assert!(ruler.redo());
            assert_eq!(&snapshot(&ruler), state);
        }
        assert!(!ruler.redo());
    }
    #[test]
    fn one_drag_is_one_step_and_noop_drag_preserves_redo() {
        let mut ruler = Ruler::default();
        ruler.add_point((0.0, 0.0), false);
        let id = RulerPointId { path: 0, point: 0 };
        ruler.begin_move();
        for index in 1..=50 {
            ruler.move_point(id, (index as f64, 0.0));
        }
        ruler.end_move();
        assert!(ruler.undo());
        assert_eq!(ruler.points().copied().collect::<Vec<_>>(), [(0.0, 0.0)]);
        ruler.begin_move();
        ruler.move_point(id, (0.0, 0.0));
        ruler.end_move();
        assert!(ruler.can_redo());
        assert!(ruler.redo());
        assert_eq!(ruler.points().copied().collect::<Vec<_>>(), [(50.0, 0.0)]);
        assert!(ruler.undo());
        ruler.add_point((1.0, 1.0), true);
        assert!(!ruler.can_redo());
        assert!(ruler.undo());
        assert!(ruler.undo());
        assert_eq!(ruler.point_count(), 0);
    }
    #[test]
    fn history_is_bounded_and_invalid_edits_do_not_create_steps() {
        let mut ruler = Ruler::default();
        ruler.clear();
        ruler.remove_point(RulerPointId { path: 0, point: 0 });
        assert!(!ruler.can_undo());
        for index in 0..MAX_HISTORY_STEPS + 10 {
            ruler.add_point((0.0, index as f64), false);
        }
        let mut count = 0;
        while ruler.undo() {
            count += 1;
        }
        assert_eq!(count, MAX_HISTORY_STEPS);
        assert_eq!(ruler.point_count(), 10);
        let mut count = 0;
        while ruler.redo() {
            count += 1;
        }
        assert_eq!(count, MAX_HISTORY_STEPS);
    }
    #[test]
    fn disconnected_paths_exclude_gaps_and_start_without_a_bearing() {
        let mut ruler = Ruler::default();
        ruler.add_point((0.0, 0.0), true);
        ruler.add_point((0.0, 1.0), false);
        let first_distance = ruler.total_distance_km();
        ruler.add_point((40.0, 80.0), true);
        assert!(ruler.last_leg().is_none());
        assert_eq!(ruler.total_distance_km(), first_distance);
        ruler.add_point((41.0, 80.0), false);
        assert_eq!(ruler.last_leg(), Some([(40.0, 80.0), (41.0, 80.0)]));
        assert_eq!(
            ruler.total_distance_km(),
            first_distance + haversine_km((40.0, 80.0), (41.0, 80.0))
        );
        assert_eq!(ruler.point_count(), 4);
        ruler.move_point(RulerPointId { path: 1, point: 1 }, (42.0, 80.0));
        assert_eq!(ruler.last_leg(), Some([(40.0, 80.0), (42.0, 80.0)]));
    }
    #[test]
    fn deleting_and_clearing_preserve_path_boundaries() {
        let mut ruler = Ruler::default();
        ruler.add_point((0.0, 0.0), false);
        ruler.add_point((0.0, 1.0), false);
        ruler.add_point((20.0, 30.0), true);
        ruler.add_point((21.0, 30.0), false);
        ruler.remove_point(RulerPointId { path: 1, point: 0 });
        assert_eq!(
            ruler.total_distance_km(),
            haversine_km((0.0, 0.0), (0.0, 1.0))
        );
        ruler.remove_point(RulerPointId { path: 0, point: 0 });
        assert_eq!(ruler.total_distance_km(), 0.0);
        assert_eq!(ruler.paths().count(), 2);
        ruler.remove_point(RulerPointId { path: 1, point: 0 });
        assert_eq!(ruler.paths().count(), 2);
        ruler.add_point((50.0, 50.0), false);
        assert_eq!(ruler.total_distance_km(), 0.0);
        ruler.clear();
        assert_eq!(ruler.point_count(), 0);
        assert!(ruler.last_leg().is_none());
        ruler.add_point((10.0, 20.0), false);
        assert_eq!(ruler.paths().count(), 1);
    }
}
