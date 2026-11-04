//! Points that would cover each other on screen, gathered into clusters (roadmap
//! 4.I): the most important point leads, and every other point within `radius` of a
//! leader joins the nearest one. Deterministic, and linear in the number of points
//! through a grid of `radius`-sized cells.
use emath::Pos2;
use std::collections::HashMap;

/// A point to cluster: where it is on screen and how important it is (larger first;
/// ties keep their order).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterPoint {
    pub position: Pos2,
    pub weight: f64,
}

/// A cluster: its leader (an index into the input) and every member, the leader
/// first, then by weight. Clusters come in the order of their leaders' weights.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub leader: usize,
    pub members: Vec<usize>,
}

impl Cluster {
    pub fn len(&self) -> usize {
        self.members.len()
    }
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

pub fn cluster(points: &[ClusterPoint], radius: f32) -> Vec<Cluster> {
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by(|&a, &b| {
        points[b]
            .weight
            .total_cmp(&points[a].weight)
            .then(a.cmp(&b))
    });
    if radius <= 0.0 {
        return order
            .into_iter()
            .map(|i| Cluster {
                leader: i,
                members: vec![i],
            })
            .collect();
    }
    let cell = |p: Pos2| ((p.x / radius).floor() as i64, (p.y / radius).floor() as i64);
    // The leaders in each cell, as indices into `clusters`.
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut clusters: Vec<Cluster> = Vec::new();
    let reach = radius * radius;
    for i in order {
        let at = points[i].position;
        let (cx, cy) = cell(at);
        let mut nearest: Option<(f32, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &c in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let distance = points[clusters[c].leader].position.distance_sq(at);
                    if distance <= reach && nearest.is_none_or(|(best, _)| distance < best) {
                        nearest = Some((distance, c));
                    }
                }
            }
        }
        match nearest {
            Some((_, c)) => clusters[c].members.push(i),
            None => {
                grid.entry((cx, cy)).or_default().push(clusters.len());
                clusters.push(Cluster {
                    leader: i,
                    members: vec![i],
                });
            }
        }
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;
    use emath::pos2;

    fn point(x: f32, y: f32, weight: f64) -> ClusterPoint {
        ClusterPoint {
            position: pos2(x, y),
            weight,
        }
    }

    #[test]
    fn the_heaviest_point_leads_and_close_ones_join_it() {
        let points = [
            point(100.0, 100.0, 2.5),
            point(110.0, 100.0, 6.1),
            point(400.0, 100.0, 3.0),
            point(118.0, 108.0, 4.0),
        ];
        let clusters = cluster(&points, 20.0);
        assert_eq!(
            clusters,
            [
                Cluster {
                    leader: 1,
                    members: vec![1, 3, 0]
                },
                Cluster {
                    leader: 2,
                    members: vec![2]
                },
            ]
        );
    }

    #[test]
    fn a_point_joins_the_nearest_leader_and_far_points_stay_alone() {
        // Two leaders 30 apart; a point between them, nearer the second.
        let points = [
            point(0.0, 0.0, 9.0),
            point(30.0, 0.0, 8.0),
            point(18.0, 0.0, 1.0),
        ];
        let clusters = cluster(&points, 20.0);
        assert_eq!(clusters[1].members, vec![1, 2]);
        assert_eq!(cluster(&points, 0.0).len(), 3);
        assert!(cluster(&[], 20.0).is_empty());
    }

    #[test]
    fn many_points_cluster_in_linear_time_with_every_point_once() {
        let points: Vec<ClusterPoint> = (0..20_000)
            .map(|i| {
                point(
                    (i % 200) as f32 * 3.0,
                    (i / 200) as f32 * 3.0,
                    (i % 7) as f64,
                )
            })
            .collect();
        let clusters = cluster(&points, 24.0);
        let mut seen: Vec<usize> = clusters.iter().flat_map(|c| c.members.clone()).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..points.len()).collect::<Vec<_>>());
        for c in &clusters {
            for &m in &c.members {
                assert!(points[c.leader].position.distance(points[m].position) <= 24.0);
            }
        }
    }
}
