//! Screen-space occupancy, preserving the caller's ordered label admission.
//!
//! Map names come from several layers (place names, street names, airports, places of
//! interest), each built and cached on its own. They share one screen: each layer
//! publishes the rectangles it took as `PlacedLabels`, and the layers after it in
//! priority order place their names only where a `LabelSpace` of those rectangles is
//! free.
use emath::Rect;
use rstar::{AABB, RTree, RTreeObject};
use std::cell::OnceCell;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct LabelRect(Rect);

impl RTreeObject for LabelRect {
    type Envelope = AABB<[f32; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.0.min.x, self.0.min.y], [self.0.max.x, self.0.max.y])
    }
}

#[derive(Default)]
pub struct LabelIndex {
    tree: RTree<LabelRect>,
}
impl LabelIndex {
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }
    pub fn len(&self) -> usize {
        self.tree.size()
    }
    pub fn try_insert(&mut self, rect: Rect) -> bool {
        let entry = LabelRect(rect);
        // Internal traversal short-circuits without allocating an iterator stack.
        // AABB and egui both treat touching edges as intersection.
        let overlap = self
            .tree
            .locate_in_envelope_intersecting_int(entry.envelope(), |_| ControlFlow::Break(()))
            .is_break();
        if overlap {
            return false;
        }
        self.tree.insert(entry);
        true
    }
}

/// Source of `PlacedLabels` revisions, unique across layers.
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// The screen rectangles one name layer took, and a revision that changes only when
/// they do (not on every rebuild), so later layers place again only when needed.
#[derive(Clone, Debug, Default)]
pub struct PlacedLabels {
    rects: Arc<[Rect]>,
    revision: u64,
}
impl PlacedLabels {
    /// Replaces the rectangles; the revision changes when they differ.
    pub fn replace(&mut self, rects: Vec<Rect>) {
        if *self.rects != *rects {
            self.rects = rects.into();
            self.revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn clear(&mut self) {
        self.replace(Vec::new());
    }
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }
}

/// Identifies what a `LabelSpace` holds: equal keys mean equal taken rectangles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelSpaceKey(Vec<u64>);

/// The room the names of higher priority took this frame, layer by layer. The
/// spatial index is built on the first query, so a frame where every layer kept
/// its names costs nothing beyond collecting the shared rectangle lists.
#[derive(Default)]
pub struct LabelSpace {
    layers: Vec<PlacedLabels>,
    index: OnceCell<RTree<LabelRect>>,
}
impl LabelSpace {
    /// Adds the names of the next layer in priority order.
    /// This space with `placed` taken too.
    pub fn with(&self, placed: &PlacedLabels) -> Self {
        let mut space = Self {
            layers: self.layers.clone(),
            index: OnceCell::new(),
        };
        space.add(placed);
        space
    }
    pub fn add(&mut self, placed: &PlacedLabels) {
        self.layers.push(placed.clone());
        self.index = OnceCell::new();
    }
    pub fn key(&self) -> LabelSpaceKey {
        LabelSpaceKey(self.layers.iter().map(|layer| layer.revision).collect())
    }
    /// Whether `rect` touches none of the rectangles taken (touching edges count
    /// as overlap, as in `LabelIndex`).
    pub fn is_free(&self, rect: Rect) -> bool {
        let index = self.index.get_or_init(|| {
            RTree::bulk_load(
                self.layers
                    .iter()
                    .flat_map(|layer| layer.rects.iter())
                    .filter(|rect| rect.is_positive())
                    .map(|rect| LabelRect(*rect))
                    .collect(),
            )
        });
        let probe = LabelRect(rect);
        index
            .locate_in_envelope_intersecting_int(probe.envelope(), |_| ControlFlow::Break(()))
            .is_continue()
    }
}

#[cfg(test)]
mod tests;
