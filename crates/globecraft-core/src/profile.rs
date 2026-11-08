//! The ground's height along the ruler (roadmap 1.10): where to sample the paths,
//! and what the heights add up to. Reading the heights is the frontend's job.
use crate::earth_model::EarthModel;

/// One sample: how far along the ruler, on which of its paths, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfilePoint {
    pub path: usize,
    pub distance_km: f64,
    pub lat: f64,
    pub lon: f64,
}

/// About `samples` points along every leg of every path, spread by length (each leg
/// gets one at least, and both its ends). Distances run on across paths, as the
/// ruler's total does.
pub fn sample<'a>(
    paths: impl Iterator<Item = &'a [(f64, f64)]>,
    model: EarthModel,
    samples: usize,
) -> Vec<ProfilePoint> {
    let paths: Vec<&[(f64, f64)]> = paths.filter(|path| path.len() >= 2).collect();
    let total: f64 = paths
        .iter()
        .flat_map(|path| path.array_windows::<2>())
        .map(|pair| model.distance_km(pair[0], pair[1]))
        .sum();
    let mut points = Vec::new();
    if total <= 0.0 {
        return points;
    }
    let geometry = model.geometry();
    let mut travelled = 0.0;
    for (index, path) in paths.iter().enumerate() {
        for (leg, pair) in path.array_windows::<2>().enumerate() {
            let length = model.distance_km(pair[0], pair[1]);
            let segments = ((length / total * samples as f64).round() as usize).max(1);
            let route = model.path(pair[0], pair[1], segments);
            let count = route.len().saturating_sub(1).max(1) as f64;
            for (step, at) in route.iter().enumerate() {
                // A leg's start is the previous leg's end.
                if leg > 0 && step == 0 {
                    continue;
                }
                let (lat, lon) = geometry.coordinates(*at);
                points.push(ProfilePoint {
                    path: index,
                    distance_km: travelled + length * step as f64 / count,
                    lat,
                    lon,
                });
            }
            travelled += length;
        }
    }
    points
}

/// The lowest and highest point, and the climbing and descending along the way
/// (within each path; samples without a height are skipped).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileStats {
    pub min_m: f64,
    pub max_m: f64,
    pub ascent_m: f64,
    pub descent_m: f64,
}

pub fn stats(points: &[ProfilePoint], heights: &[Option<f32>]) -> Option<ProfileStats> {
    let mut stats: Option<ProfileStats> = None;
    let mut last: Option<(usize, f64)> = None;
    for (point, height) in points.iter().zip(heights) {
        let Some(height) = height.map(f64::from) else {
            continue;
        };
        let current = stats.get_or_insert(ProfileStats {
            min_m: height,
            max_m: height,
            ..ProfileStats::default()
        });
        current.min_m = current.min_m.min(height);
        current.max_m = current.max_m.max(height);
        if let Some((path, before)) = last
            && path == point.path
        {
            let change = height - before;
            if change > 0.0 {
                current.ascent_m += change;
            } else {
                current.descent_m -= change;
            }
        }
        last = Some((point.path, height));
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_spread_by_length_and_run_on_across_paths() {
        let first: &[(f64, f64)] = &[(0.0, 0.0), (0.0, 1.0), (0.0, 3.0)];
        let second: &[(f64, f64)] = &[(10.0, 0.0), (10.0, 1.0)];
        let points = sample([first, second].into_iter(), EarthModel::PerfectSphere, 40);
        let first_points: Vec<_> = points.iter().filter(|p| p.path == 0).collect();
        // The 1° leg and the 2° leg share 30 samples about 1:2, the start not repeated.
        assert_eq!(first_points.len(), 1 + 10 + 20);
        assert!(
            first_points
                .windows(2)
                .all(|w| w[1].distance_km > w[0].distance_km)
        );
        let end = first_points.last().unwrap();
        assert!((end.lon - 3.0).abs() < 1e-9 && (end.distance_km - 333.585).abs() < 0.01);
        let second_start = points.iter().find(|p| p.path == 1).unwrap();
        assert!((second_start.distance_km - end.distance_km).abs() < 1e-9);
        let second_length = EarthModel::PerfectSphere.distance_km((10.0, 0.0), (10.0, 1.0));
        let total = end.distance_km + second_length;
        assert!((points.last().unwrap().distance_km - total).abs() < 1e-9);
    }

    #[test]
    fn a_path_of_one_point_has_no_profile() {
        let single: &[(f64, f64)] = &[(0.0, 0.0)];
        assert!(sample([single].into_iter(), EarthModel::Wgs84, 100).is_empty());
    }

    #[test]
    fn climbing_counts_within_a_path_and_skips_unknown_heights() {
        let at = |path, distance_km| ProfilePoint {
            path,
            distance_km,
            lat: 0.0,
            lon: 0.0,
        };
        let points = [
            at(0, 0.0),
            at(0, 1.0),
            at(0, 2.0),
            at(0, 3.0),
            at(1, 4.0),
            at(1, 5.0),
        ];
        let heights = [
            Some(100.0),
            None,
            Some(300.0),
            Some(250.0),
            Some(1000.0),
            Some(900.0),
        ];
        let stats = stats(&points, &heights).unwrap();
        assert_eq!(stats.min_m, 100.0);
        assert_eq!(stats.max_m, 1000.0);
        // 100 → 300 up, 300 → 250 down; the jump to the second path does not count.
        assert_eq!(stats.ascent_m, 200.0);
        assert_eq!(stats.descent_m, 150.0);
        assert!(super::stats(&points, &[None; 6]).is_none());
    }
}
