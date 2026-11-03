use super::*;
use emath::{pos2, vec2};

#[test]
fn label_occupancy_preserves_touching_edges_containment_and_first_admission() {
    let mut index = LabelIndex::default();
    let first = Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0));
    assert!(index.try_insert(first));
    for area in [
        first,
        Rect::from_min_max(pos2(2.0, 2.0), pos2(4.0, 4.0)),
        Rect::from_min_max(pos2(-1.0, -1.0), pos2(11.0, 11.0)),
        Rect::from_min_max(pos2(10.0, 1.0), pos2(12.0, 3.0)),
        Rect::from_min_max(pos2(10.0, 10.0), pos2(12.0, 12.0)),
        Rect::from_min_max(pos2(5.0, 5.0), pos2(5.0, 5.0)),
    ] {
        assert!(!index.try_insert(area));
    }
    assert_eq!(index.len(), 1);
    assert!(index.try_insert(Rect::from_min_size(pos2(11.0, 0.0), vec2(3.0, 3.0))));
    assert_eq!(index.len(), 2);
}

#[test]
fn indexed_decisions_match_egui_intersection_for_sparse_and_dense_rectangles() {
    let mut seed = 42u64;
    for span in [100.0, 2000.0] {
        let mut index = LabelIndex::default();
        let mut accepted: Vec<Rect> = Vec::new();
        for _ in 0..10_000 {
            let mut next = || {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                (seed >> 32) as f32 / u32::MAX as f32
            };
            let area = Rect::from_min_size(
                pos2(next() * span, next() * span),
                vec2(5.0 + next() * 60.0, 5.0 + next() * 20.0),
            );
            let expected = !accepted.iter().any(|rect| rect.intersects(area));
            assert_eq!(index.try_insert(area), expected);
            if expected {
                accepted.push(area);
            }
        }
        assert_eq!(index.len(), accepted.len());
    }
}

#[test]
fn label_space_answers_across_layers_and_its_key_changes_only_with_the_rectangles() {
    let city = Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 12.0));
    let street = Rect::from_min_size(pos2(100.0, 0.0), vec2(60.0, 10.0));
    let mut places = PlacedLabels::default();
    let mut streets = PlacedLabels::default();
    places.replace(vec![city]);
    streets.replace(vec![street]);
    let mut space = LabelSpace::default();
    space.add(&places);
    let after_places = space.key();
    assert!(!space.is_free(Rect::from_min_size(pos2(30.0, 5.0), vec2(20.0, 10.0))));
    assert!(space.is_free(Rect::from_min_size(pos2(110.0, 2.0), vec2(5.0, 5.0))));
    space.add(&streets);
    assert!(!space.is_free(Rect::from_min_size(pos2(110.0, 2.0), vec2(5.0, 5.0))));
    // Touching edges overlap, as in `LabelIndex`.
    assert!(!space.is_free(Rect::from_min_size(pos2(40.0, 0.0), vec2(5.0, 5.0))));
    assert!(space.is_free(Rect::from_min_size(pos2(41.0, 0.0), vec2(5.0, 5.0))));

    // Rebuilding a layer with the same names keeps the key; other names change it.
    let before = space.key();
    places.replace(vec![city]);
    let mut again = LabelSpace::default();
    again.add(&places);
    again.add(&streets);
    assert_eq!(again.key(), before);
    places.replace(vec![city, street]);
    let mut moved = LabelSpace::default();
    moved.add(&places);
    assert_ne!(moved.key(), after_places);
    places.clear();
    let mut empty = LabelSpace::default();
    empty.add(&places);
    assert!(empty.is_free(city));
}
