//! Tile sources as data (roadmap 1.2): where a source's tiles come from, what they
//! show, their zoom range and attribution. The built-in sources are read from
//! `sources.json`; a new source is a new entry there, not new code.
use serde::Deserialize;
use std::sync::LazyLock;

/// What a source's tiles are.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Photographs: faded smoothly between zoom levels.
    Imagery,
    /// Drawn maps with text: switched with a short fade at the end of a level.
    Map,
    /// Terrarium-encoded heights.
    Elevation,
    /// Drawn over the basemap (night lights).
    Overlay,
}

/// How a tile's address is built.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Access {
    /// A URL template with `{z}`, `{x}` and `{y}` (XYZ or WMTS REST in Web Mercator).
    Xyz { template: String },
    /// Esri Wayback: the release's own address, from the release list.
    Wayback,
}

/// Whether tiles may be loaded ahead of need.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Prefetch {
    Allowed,
    /// Only what is shown (the OpenStreetMap tile usage policy).
    OnDemand,
}

/// One tile source.
#[derive(Clone, Debug, Deserialize)]
pub struct TileSource {
    /// Stable identifier inside tile keys and the disk cache.
    pub id: u8,
    /// Identifier in settings and commands.
    pub code: String,
    /// Display name, and its translation key when the interface has one.
    pub name: String,
    #[serde(default)]
    pub name_key: Option<String>,
    pub kind: SourceKind,
    /// Offered as a basemap.
    pub basemap: bool,
    pub access: Access,
    #[serde(default)]
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// Pixels per tile side.
    #[serde(default = "default_tile_px")]
    pub tile_px: u16,
    /// Folder of the source's disk cache (inside the tile cache folder).
    pub cache_dir: String,
    pub attribution: String,
    #[serde(default)]
    pub attribution_key: Option<String>,
    pub prefetch: Prefetch,
}

fn default_tile_px() -> u16 {
    256
}

#[derive(Deserialize)]
struct SourceFile {
    sources: Vec<TileSource>,
}

/// Tile keys hold source ids in five bits.
pub(super) const MAX_SOURCES: usize = 32;

/// The built-in sources by id, and the order they are listed in (the file's).
pub(super) struct Registry {
    pub(super) by_id: Vec<Option<TileSource>>,
    pub(super) order: Vec<u8>,
}

pub(super) static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let file: SourceFile = serde_json::from_str(include_str!("sources.json"))
        .expect("the bundled tile sources are valid");
    let mut by_id: Vec<Option<TileSource>> = vec![None; MAX_SOURCES];
    let mut order = Vec::new();
    for source in file.sources {
        let slot = &mut by_id[usize::from(source.id)];
        assert!(slot.is_none(), "tile source id {} is used twice", source.id);
        order.push(source.id);
        *slot = Some(source);
    }
    Registry { by_id, order }
});

impl TileSource {
    /// The address of tile `z/x/y`, for template sources.
    pub fn tile_url(&self, z: u8, x: u32, y: u32) -> Option<String> {
        match &self.access {
            Access::Xyz { template } => Some(
                template
                    .replace("{z}", &z.to_string())
                    .replace("{x}", &x.to_string())
                    .replace("{y}", &y.to_string()),
            ),
            Access::Wayback => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sources_are_complete_and_their_ids_unique() {
        let sources: Vec<&TileSource> = REGISTRY
            .order
            .iter()
            .map(|id| REGISTRY.by_id[usize::from(*id)].as_ref().unwrap())
            .collect();
        assert!(sources.len() >= 6);
        let mut codes: Vec<&str> = sources.iter().map(|s| s.code.as_str()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), sources.len());
        for source in sources {
            assert!(source.max_zoom >= source.min_zoom && source.max_zoom <= 24);
            assert!(source.tile_px.is_power_of_two());
            if let Access::Xyz { template } = &source.access {
                assert!(template.starts_with("https://"), "{}", source.code);
                for part in ["{z}", "{x}", "{y}"] {
                    assert!(template.contains(part), "{}: {part}", source.code);
                }
            }
        }
    }

    #[test]
    fn templates_fill_the_tile_address() {
        let source = TileSource {
            id: 9,
            code: "test".into(),
            name: "Test".into(),
            name_key: None,
            kind: SourceKind::Overlay,
            basemap: false,
            access: Access::Xyz {
                template: "https://example.org/{z}/{y}/{x}.png".into(),
            },
            min_zoom: 0,
            max_zoom: 8,
            tile_px: 256,
            cache_dir: "test".into(),
            attribution: String::new(),
            attribution_key: None,
            prefetch: Prefetch::Allowed,
        };
        assert_eq!(
            source.tile_url(3, 5, 2).as_deref(),
            Some("https://example.org/3/2/5.png")
        );
    }
}
