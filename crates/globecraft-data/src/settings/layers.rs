//! Visible map layers, the basemap and terrain exaggeration, kept across launches.
use crate::tiles::Provider;
use globecraft_core::layers::{LayerSet, MapLayer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const MIN_TERRAIN_EXAGGERATION: f32 = 0.5;
pub const MAX_TERRAIN_EXAGGERATION: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerPreferences {
    /// Layers other than those stored elsewhere for compatibility (stars, the Milky Way and
    /// grid labels live in `sky` and `grid_numbers`; see `Settings::layer_visible`).
    #[serde(
        serialize_with = "serialize_layers",
        deserialize_with = "deserialize_layers"
    )]
    pub visible: LayerSet,
    #[serde(
        serialize_with = "serialize_basemap",
        deserialize_with = "deserialize_basemap"
    )]
    pub basemap: Provider,
    pub terrain_exaggeration: f32,
    /// Aircraft and satellites drawn on the ground below them, not at their height.
    pub flat_live_objects: bool,
    /// The opacity of the layers that have one (`MapLayer::WITH_OPACITY`).
    #[serde(
        serialize_with = "serialize_opacity",
        deserialize_with = "deserialize_opacity"
    )]
    pub opacity: LayerOpacity,
}

/// Opacities from 0 (hidden) to 1, one for each of `MapLayer::WITH_OPACITY`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerOpacity([f32; MapLayer::WITH_OPACITY.len()]);

impl Default for LayerOpacity {
    fn default() -> Self {
        Self([1.0; MapLayer::WITH_OPACITY.len()])
    }
}

impl LayerOpacity {
    /// The layer's opacity; 1 for layers without one.
    pub fn get(&self, layer: MapLayer) -> f32 {
        Self::index(layer).map_or(1.0, |index| self.0[index])
    }
    /// Sets it, clamped to 0..=1; `false` for a layer without an opacity.
    pub fn set(&mut self, layer: MapLayer, opacity: f32) -> bool {
        match Self::index(layer) {
            Some(index) if opacity.is_finite() => {
                self.0[index] = opacity.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }
    /// A saved table of layer code to opacity: entries that name a layer and hold a
    /// number are taken.
    fn apply(&mut self, value: &serde_json::Value) {
        for (code, value) in value.as_object().into_iter().flatten() {
            if let (Some(layer), Some(value)) = (MapLayer::from_code(code), value.as_f64()) {
                self.set(layer, value as f32);
            }
        }
    }
    fn index(layer: MapLayer) -> Option<usize> {
        MapLayer::WITH_OPACITY
            .iter()
            .position(|other| *other == layer)
    }
}

fn serialize_opacity<S: Serializer>(
    opacity: &LayerOpacity,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_map(
        MapLayer::WITH_OPACITY
            .iter()
            .map(|layer| (layer.code(), opacity.get(*layer))),
    )
}

impl Default for LayerPreferences {
    fn default() -> Self {
        Self {
            visible: LayerSet::default(),
            basemap: Provider::Satellite,
            terrain_exaggeration: 1.0,
            flat_live_objects: false,
            opacity: LayerOpacity::default(),
        }
    }
}

fn serialize_layers<S: Serializer>(layers: &LayerSet, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(layers.codes())
}

fn deserialize_layers<'de, D: Deserializer<'de>>(deserializer: D) -> Result<LayerSet, D::Error> {
    let codes = Vec::<String>::deserialize(deserializer)?;
    Ok(LayerSet::from_codes(codes.iter().map(String::as_str)))
}

fn deserialize_basemap<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Provider, D::Error> {
    super::lenient::coded(deserializer, Provider::from_code)
}

fn deserialize_opacity<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<LayerOpacity, D::Error> {
    super::lenient::applied(deserializer, LayerOpacity::apply)
}

fn serialize_basemap<S: Serializer>(basemap: &Provider, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(basemap.code())
}

impl LayerPreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        super::lenient::apply(self, value, &["visible"]);
        self.terrain_exaggeration = self
            .terrain_exaggeration
            .clamp(MIN_TERRAIN_EXAGGERATION, MAX_TERRAIN_EXAGGERATION);
        // Unknown layer names are passed over, and the live layers start hidden.
        if let Some(codes) = value.get("visible").and_then(|v| v.as_array()) {
            self.visible = LayerSet::from_codes(codes.iter().filter_map(|c| c.as_str()))
                .iter()
                .filter(|layer| !layer.starts_hidden())
                .collect();
        }
        if let Some(opacity) = value.get("opacity") {
            self.opacity.apply(opacity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacities_roundtrip_clamped_and_only_for_layers_that_have_one() {
        let mut prefs = LayerPreferences::default();
        assert!(prefs.opacity.set(MapLayer::Clouds, 0.4));
        assert!(prefs.opacity.set(MapLayer::Radar, 7.0));
        assert!(!prefs.opacity.set(MapLayer::CountryBorders, 0.5));
        assert_eq!(prefs.opacity.get(MapLayer::Radar), 1.0);
        assert_eq!(prefs.opacity.get(MapLayer::CountryBorders), 1.0);
        let json = serde_json::to_value(prefs).unwrap();
        assert!((json["opacity"]["clouds"].as_f64().unwrap() - 0.4).abs() < 1e-6);
        let mut loaded = LayerPreferences::default();
        loaded.apply(&json);
        assert_eq!(loaded.opacity, prefs.opacity);
        loaded.apply(&serde_json::json!({"opacity": {"clouds": "x", "nope": 0.1}}));
        assert_eq!(loaded.opacity, prefs.opacity);
    }

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
