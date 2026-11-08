//! "Solo" (roadmap 6.1): show only one layer of the map's content for a moment, and
//! then everything that was visible before. Alt-click (Option-click) on a layer in
//! the Layers pane, or `layer.solo`. The globe's look (relief, atmosphere, light,
//! sky) is left as it is.
use super::App;
use globecraft_core::layers::{LayerSet, MapLayer};
use serde_json::{Value, json};

/// The layers to show while `only` is soloed: it (and its parent) among the map's
/// content, everything else unchanged.
fn soloed(visible: LayerSet, only: MapLayer) -> LayerSet {
    let mut set = visible;
    for layer in MapLayer::ALL.into_iter().filter(|layer| layer.soloable()) {
        set.set(layer, layer == only || only.parent() == Some(layer));
    }
    set
}

/// What was visible before, back, for the map's content only.
fn restored(visible: LayerSet, before: LayerSet) -> LayerSet {
    let mut set = visible;
    for layer in MapLayer::ALL.into_iter().filter(|layer| layer.soloable()) {
        set.set(layer, before.contains(layer));
    }
    set
}

impl App {
    /// `layer.solo`: shows only `layer`; the same layer again, or none, shows what
    /// was visible before.
    pub(super) fn solo_layer(&mut self, layer: Option<String>) -> Result<Value, String> {
        let layer = match layer {
            Some(code) => Some(
                MapLayer::from_code(&code)
                    .filter(|layer| layer.soloable())
                    .ok_or("unknown layer, or one that is not map content (see layer.show)")?,
            ),
            None => None,
        };
        let visible = self.settings.visible_layers();
        let next = match (self.solo, layer) {
            (Some((current, before)), Some(layer)) if current != layer => {
                self.solo = Some((layer, before));
                soloed(visible, layer)
            }
            (Some((_, before)), _) => {
                self.solo = None;
                restored(visible, before)
            }
            (None, Some(layer)) => {
                self.solo = Some((layer, visible));
                soloed(visible, layer)
            }
            (None, None) => visible,
        };
        for layer in MapLayer::ALL {
            if self.settings.layer_visible(layer) != next.contains(layer) {
                self.settings.set_layer_visible(layer, next.contains(layer));
            }
        }
        Ok(json!({"solo": self.solo.map(|(layer, _)| layer.code())}))
    }

    /// The layer shown alone, if one is.
    pub(super) fn soloed_layer(&self) -> Option<MapLayer> {
        self.solo.map(|(layer, _)| layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_shows_one_layer_of_content_and_restores_the_rest() {
        let mut before = LayerSet::EMPTY;
        for layer in [
            MapLayer::CountryBorders,
            MapLayer::PlaceLabels,
            MapLayer::Clouds,
            MapLayer::Atmosphere,
            MapLayer::Terrain,
        ] {
            before.set(layer, true);
        }
        let solo = soloed(before, MapLayer::Radar);
        assert!(solo.contains(MapLayer::Radar));
        assert!(!solo.contains(MapLayer::CountryBorders) && !solo.contains(MapLayer::Clouds));
        // The globe's look stays.
        assert!(solo.contains(MapLayer::Atmosphere) && solo.contains(MapLayer::Terrain));
        // A refinement brings its parent.
        assert!(soloed(before, MapLayer::GridLabels).contains(MapLayer::Grid));
        assert_eq!(restored(solo, before), before);
    }
}
