//! Visible map layers, the basemap and terrain exaggeration, kept across launches.
use crate::tiles::Provider;
use globecraft_core::layers::LayerSet;
use serde::{Serialize, Serializer};

pub const MIN_TERRAIN_EXAGGERATION: f32 = 0.5;
pub const MAX_TERRAIN_EXAGGERATION: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LayerPreferences {
    /// Layers other than those stored elsewhere for compatibility (stars, cinematic sky and grid
    /// labels live in `sky` and `grid_numbers`; see `Settings::layer_visible`).
    #[serde(serialize_with = "serialize_layers")]
    pub visible: LayerSet,
    #[serde(serialize_with = "serialize_basemap")]
    pub basemap: Provider,
    pub terrain_exaggeration: f32,
}

impl Default for LayerPreferences {
    fn default() -> Self {
        Self {
            visible: LayerSet::default(),
            basemap: Provider::Satellite,
            terrain_exaggeration: 1.0,
        }
    }
}

fn serialize_layers<S: Serializer>(layers: &LayerSet, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(layers.codes())
}

fn serialize_basemap<S: Serializer>(basemap: &Provider, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(basemap.code())
}

impl LayerPreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        if let Some(codes) = value.get("visible").and_then(|v| v.as_array()) {
            self.visible = LayerSet::from_codes(codes.iter().filter_map(|c| c.as_str()))
                .iter()
                .filter(|layer| !layer.starts_hidden())
                .collect();
        }
        if let Some(basemap) = value
            .get("basemap")
            .and_then(|v| v.as_str())
            .and_then(Provider::from_code)
        {
            self.basemap = basemap;
        }
        if let Some(value) = value
            .get("terrain_exaggeration")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
        {
            self.terrain_exaggeration =
                (value as f32).clamp(MIN_TERRAIN_EXAGGERATION, MAX_TERRAIN_EXAGGERATION);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::layers::MapLayer;

    #[test]
    fn layers_roundtrip_and_ignore_invalid_fields() {
        let mut prefs = LayerPreferences::default();
        prefs.visible.set(MapLayer::Terrain, true);
        prefs.basemap = Provider::Topo;
        prefs.terrain_exaggeration = 2.0;
        let json = serde_json::to_value(prefs).unwrap();
        let mut restored = LayerPreferences::default();
        restored.apply(&json);
        assert_eq!(restored, prefs);
        restored.apply(
            &serde_json::json!({"visible": 3, "basemap": "mars", "terrain_exaggeration": 99}),
        );
        assert_eq!(restored.visible, prefs.visible);
        assert_eq!(restored.basemap, Provider::Topo);
        assert_eq!(restored.terrain_exaggeration, MAX_TERRAIN_EXAGGERATION);
    }

    #[test]
    fn live_feeds_start_hidden_on_launch() {
        let mut prefs = LayerPreferences::default();
        for layer in [MapLayer::Flights, MapLayer::Radar, MapLayer::Terrain] {
            prefs.visible.set(layer, true);
        }
        let mut restored = LayerPreferences::default();
        restored.apply(&serde_json::to_value(prefs).unwrap());
        assert!(!restored.visible.contains(MapLayer::Flights));
        assert!(!restored.visible.contains(MapLayer::Radar));
        assert!(restored.visible.contains(MapLayer::Terrain));
    }
}
