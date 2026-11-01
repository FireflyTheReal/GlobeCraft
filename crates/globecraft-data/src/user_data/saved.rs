//! The Browser pane's memory (roadmap 2.5): sources the user starred and the ones
//! added lately, kept as layer templates so they can be added again in one click,
//! saved in the settings. A source is identified by where its data comes from, not by
//! its name.
use super::{UserLayer, UserSource};
use serde::Serialize;

/// Recently added sources kept.
pub const MAX_RECENT: usize = 12;
/// Starred sources kept.
pub const MAX_FAVORITES: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SavedSources {
    /// Starred, in the order they were starred.
    pub favorites: Vec<UserLayer>,
    /// Added lately, newest first.
    pub recent: Vec<UserLayer>,
}

/// A layer as a template: no id, shown, at the opacity it was added with.
fn template(layer: &UserLayer) -> UserLayer {
    UserLayer {
        id: 0,
        visible: true,
        ..layer.clone()
    }
}

impl SavedSources {
    /// Notes that `layer` was added: it moves to the front of the recent list.
    pub fn remember(&mut self, layer: &UserLayer) {
        self.recent.retain(|entry| entry.source != layer.source);
        self.recent.insert(0, template(layer));
        self.recent.truncate(MAX_RECENT);
    }

    pub fn is_favorite(&self, source: &UserSource) -> bool {
        self.favorites.iter().any(|entry| &entry.source == source)
    }

    /// Stars `layer`'s source, or unstars it; returns whether it is starred now.
    pub fn toggle_favorite(&mut self, layer: &UserLayer) -> bool {
        if self.is_favorite(&layer.source) {
            self.favorites.retain(|entry| entry.source != layer.source);
            false
        } else {
            if self.favorites.len() < MAX_FAVORITES {
                self.favorites.push(template(layer));
            }
            true
        }
    }

    /// Reads the saved lists, keeping every entry that still parses, once per source.
    pub(crate) fn apply(&mut self, value: &serde_json::Value) {
        let list = |key: &str, max: usize| -> Option<Vec<UserLayer>> {
            let entries = value.get(key)?.as_array()?;
            let mut layers: Vec<UserLayer> = Vec::new();
            for entry in entries {
                let Ok(layer) = serde_json::from_value::<UserLayer>(entry.clone()) else {
                    continue;
                };
                if layers.len() < max && !layers.iter().any(|l| l.source == layer.source) {
                    layers.push(template(&layer));
                }
            }
            Some(layers)
        };
        if let Some(favorites) = list("favorites", MAX_FAVORITES) {
            self.favorites = favorites;
        }
        if let Some(recent) = list("recent", MAX_RECENT) {
            self.recent = recent;
        }
    }
}

/// The format tag of an exported source list.
const EXPORT_FORMAT: &str = "globecraft-sources";
const EXPORT_VERSION: u64 = 1;

/// A source list as a JSON document others can import: favorites shared between
/// computers or people.
pub fn export_sources(sources: &[UserLayer]) -> Vec<u8> {
    let sources: Vec<UserLayer> = sources.iter().map(template).collect();
    serde_json::to_vec_pretty(&serde_json::json!({
        "format": EXPORT_FORMAT,
        "version": EXPORT_VERSION,
        "sources": sources,
    }))
    .expect("source lists serialize")
}

/// The sources of an exported list (or a bare array of layers), each once; entries
/// that do not parse are left out.
pub fn import_sources(bytes: &[u8]) -> Result<Vec<UserLayer>, crate::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let list = match &value {
        serde_json::Value::Array(_) => &value,
        serde_json::Value::Object(map) => {
            if let Some(format) = map.get("format").and_then(serde_json::Value::as_str)
                && format != EXPORT_FORMAT
            {
                return Err(format!("not a GlobeCraft source list ({format})").into());
            }
            map.get("sources")
                .ok_or("not a GlobeCraft source list (no sources)")?
        }
        _ => return Err("not a GlobeCraft source list".into()),
    };
    let mut saved = SavedSources::default();
    saved.apply(&serde_json::json!({ "favorites": list }));
    Ok(saved.favorites)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(url: &str) -> UserLayer {
        UserLayer {
            id: 9,
            name: url.into(),
            source: UserSource::Tiles {
                template: url.into(),
                min_zoom: 0,
                max_zoom: 10,
                tile_px: 256,
            },
            visible: false,
            opacity: 0.5,
            attribution: String::new(),
            color: "#ffffff".into(),
            preset: None,
        }
    }

    #[test]
    fn recent_sources_are_newest_first_once_each_and_bounded() {
        let mut saved = SavedSources::default();
        for i in 0..20 {
            saved.remember(&layer(&format!("https://t{i}/{{z}}/{{x}}/{{y}}")));
        }
        saved.remember(&layer("https://t15/{z}/{x}/{y}"));
        assert_eq!(saved.recent.len(), MAX_RECENT);
        assert_eq!(saved.recent[0].name, "https://t15/{z}/{x}/{y}");
        assert_eq!(saved.recent[1].name, "https://t19/{z}/{x}/{y}");
        assert!(saved.recent.iter().all(|l| l.id == 0 && l.visible));
    }

    #[test]
    fn favorites_toggle_by_source_and_survive_a_round_trip() {
        let mut saved = SavedSources::default();
        let a = layer("https://a/{z}/{x}/{y}");
        assert!(saved.toggle_favorite(&a));
        let mut renamed = a.clone();
        renamed.name = "Other name".into();
        assert!(saved.is_favorite(&renamed.source));
        saved.remember(&a);
        let json = serde_json::to_value(&saved).unwrap();
        let mut back = SavedSources::default();
        back.apply(&json);
        assert_eq!(back, saved);
        assert!(!back.toggle_favorite(&renamed));
        assert!(back.favorites.is_empty());
        // Broken entries are skipped, not fatal.
        back.apply(&serde_json::json!({"favorites": [{"name": "broken"}, json["recent"][0]]}));
        assert_eq!(back.favorites.len(), 1);
    }

    #[test]
    fn exported_lists_import_again_and_other_json_is_refused() {
        let sources = vec![
            layer("https://a/{z}/{x}/{y}"),
            layer("https://b/{z}/{x}/{y}"),
        ];
        let bytes = export_sources(&sources);
        let back = import_sources(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].source, sources[0].source);
        let bare = serde_json::to_vec(&sources).unwrap();
        assert_eq!(import_sources(&bare).unwrap().len(), 2);
        assert!(import_sources(br#"{"format":"geojson","sources":[]}"#).is_err());
        assert!(import_sources(br#"{"type":"FeatureCollection"}"#).is_err());
        assert!(import_sources(b"not json").is_err());
    }
}
