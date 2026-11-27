//! Roofs under other blocks (D3). Imagery shows from above only the highest roof at
//! each place: a stadium's stands under its canopy took the canopy's photograph
//! (Julien, 2026-11-27). Each roof triangle whose middle lies inside a taller block
//! of the tile is covered; a block's roof triangles are ordered open first, and
//! `Building::open_roof` counts them, so only those are drawn as imagery.
use super::Building;

/// A block counts as covering another's roof when it rises this far above it
/// (metres): parts that only meet at their tops cover nothing.
const COVER_MARGIN_M: f32 = 0.5;
/// The tile's blocks are found through a grid of this many cells a side.
const GRID_CELLS: usize = 64;

/// Orders each block's roof triangles open to the sky first and counts them.
pub(super) fn open_roofs(buildings: &mut [Building]) {
    let boxes: Vec<[f64; 4]> = buildings.iter().map(bounds).collect();
    let Some(whole) = boxes.iter().copied().reduce(|a, b| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    }) else {
        return;
    };
    let size = [
        (whole[2] - whole[0]).max(f64::MIN_POSITIVE),
        (whole[3] - whole[1]).max(f64::MIN_POSITIVE),
    ];
    let cell_of = |lat: f64, lon: f64| {
        let index = |value: f64, low: f64, size: f64| {
            (((value - low) / size * GRID_CELLS as f64) as usize).min(GRID_CELLS - 1)
        };
        [index(lat, whole[0], size[0]), index(lon, whole[1], size[1])]
    };
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); GRID_CELLS * GRID_CELLS];
    for (index, area) in boxes.iter().enumerate() {
        let (low, high) = (cell_of(area[0], area[1]), cell_of(area[2], area[3]));
        for row in low[0]..=high[0] {
            for column in low[1]..=high[1] {
                grid[row * GRID_CELLS + column].push(index as u32);
            }
        }
    }
    let covered = |index: usize, (lat, lon): (f64, f64)| {
        let [row, column] = cell_of(lat, lon);
        grid[row * GRID_CELLS + column].iter().any(|&other| {
            let other = other as usize;
            let area = boxes[other];
            other != index
                && buildings[other].top_m > buildings[index].top_m + COVER_MARGIN_M
                && (area[0]..=area[2]).contains(&lat)
                && (area[1]..=area[3]).contains(&lon)
                && inside(&buildings[other].rings, (lat, lon))
        })
    };
    // Decided for every block first: the blocks are reordered after.
    let open: Vec<Vec<bool>> = buildings
        .iter()
        .enumerate()
        .map(|(index, building)| {
            middles(building)
                .into_iter()
                .map(|middle| !covered(index, middle))
                .collect()
        })
        .collect();
    for (building, open) in buildings.iter_mut().zip(open) {
        let triangles = match building.shaped.as_deref_mut() {
            Some(shaped) => &mut shaped.triangles,
            None => &mut building.roof,
        };
        // Exactly as long as before: tiles are held by their bytes.
        let (mut first, mut last) = (Vec::with_capacity(triangles.len()), Vec::new());
        for (triangle, &open) in triangles.chunks_exact(3).zip(&open) {
            if open { &mut first } else { &mut last }.extend_from_slice(triangle);
        }
        building.open_roof = (first.len() / 3) as u32;
        first.extend(last);
        *triangles = first;
    }
}

/// A block's outline's box: south, west, north, east.
fn bounds(building: &Building) -> [f64; 4] {
    building.rings.iter().flatten().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[south, west, north, east], &(lat, lon)| {
            [south.min(lat), west.min(lon), north.max(lat), east.max(lon)]
        },
    )
}

/// The middle of each of a block's roof triangles.
fn middles(building: &Building) -> Vec<(f64, f64)> {
    let middle = |corners: [(f64, f64); 3]| {
        (
            (corners[0].0 + corners[1].0 + corners[2].0) / 3.0,
            (corners[0].1 + corners[1].1 + corners[2].1) / 3.0,
        )
    };
    match building.shaped.as_deref() {
        Some(shaped) => {
            let Some(&(lat, lon)) = building.rings.first().and_then(|ring| ring.first()) else {
                return Vec::new();
            };
            shaped
                .triangles
                .chunks_exact(3)
                .map(|triangle| {
                    middle([0, 1, 2].map(|corner| {
                        let [dlat, dlon, _] = shaped.points[triangle[corner] as usize];
                        (lat + f64::from(dlat), lon + f64::from(dlon))
                    }))
                })
                .collect()
        }
        None => {
            let points: Vec<(f64, f64)> = building.rings.iter().flatten().copied().collect();
            building
                .roof
                .chunks_exact(3)
                .map(|triangle| middle([0, 1, 2].map(|corner| points[triangle[corner] as usize])))
                .collect()
        }
    }
}

/// Whether `point` lies inside `rings` (an outline and its courtyards): an odd
/// number of edges crossed going north.
fn inside(rings: &[Vec<(f64, f64)>], (lat, lon): (f64, f64)) -> bool {
    let mut odd = false;
    for ring in rings {
        for (index, &(a_lat, a_lon)) in ring.iter().enumerate() {
            let (b_lat, b_lon) = ring[(index + 1) % ring.len()];
            if (a_lon > lon) != (b_lon > lon) {
                let crossing = a_lat + (lon - a_lon) / (b_lon - a_lon) * (b_lat - a_lat);
                if crossing > lat {
                    odd = !odd;
                }
            }
        }
    }
    odd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(south: f64, west: f64, size: f64, base_m: f32, top_m: f32) -> Building {
        Building {
            rings: vec![vec![
                (south, west),
                (south, west + size),
                (south + size, west + size),
                (south + size, west),
            ]],
            walls: vec![vec![true; 4]],
            roof: vec![0, 1, 2, 0, 2, 3],
            base_m,
            top_m,
            roof_colour: None,
            wall_colour: None,
            shaped: None,
            open_roof: 0,
        }
    }

    #[test]
    fn only_the_highest_roof_is_open_to_the_sky() {
        // Stands under a canopy that covers their eastern half, and a tower beside
        // them that covers nothing of theirs.
        let mut blocks = vec![
            block(0.0, 0.0, 1.0, 0.0, 15.0),
            block(0.0, 0.5, 0.5, 15.0, 25.0),
            block(2.0, 2.0, 0.2, 0.0, 60.0),
        ];
        open_roofs(&mut blocks);
        // The stands' triangle (0, 1, 2) has its middle under the canopy; (0, 2, 3)
        // is open, and is now first.
        assert_eq!(blocks[0].open_roof, 1);
        assert_eq!(blocks[0].roof, [0, 2, 3, 0, 1, 2]);
        assert_eq!((blocks[1].open_roof, blocks[2].open_roof), (2, 2));
    }

    #[test]
    fn points_in_a_courtyard_are_outside() {
        let rings = vec![
            vec![(0.0, 0.0), (0.0, 3.0), (3.0, 3.0), (3.0, 0.0)],
            vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
        ];
        assert!(inside(&rings, (0.5, 0.5)));
        assert!(!inside(&rings, (1.5, 1.5)));
        assert!(!inside(&rings, (4.0, 1.0)));
    }
}
