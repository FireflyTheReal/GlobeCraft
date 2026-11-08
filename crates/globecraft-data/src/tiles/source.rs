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
    /// Downloads from this source at once.
    #[serde(default = "default_max_parallel")]
    pub max_parallel: u8,
    /// Tiles kept on disk longer than this are shown and asked for again in the
    /// background, with their `ETag` or date so an unchanged tile costs no download
    /// (roadmap 1.3). None for sources whose tiles never change (dated editions,
    /// Wayback releases, elevation).
    #[serde(default)]
    pub revalidate_days: Option<u32>,
    /// Dated editions (`YYYY-MM-DD`, newest first) that `{date}` in the template
    /// stands for; a tile key's release index picks one. Empty for undated sources.
    #[serde(default)]
    pub editions: Vec<String>,
}

fn default_max_parallel() -> u8 {
    4
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

/// Downloads are limited per host as well as per source (roadmap 1.3): sources on one
/// host share the largest of their limits, and a throttling answer pauses all of them.
pub(super) struct HostSlots {
    /// By source id: the smallest id among the sources on its host.
    pub(super) group: [u8; MAX_SOURCES],
    /// By source id: the downloads its host takes at once.
    pub(super) limit: [usize; MAX_SOURCES],
    /// By source id: another source shares its host.
    pub(super) shared: [bool; MAX_SOURCES],
}

pub(super) static HOST_SLOTS: LazyLock<HostSlots> = LazyLock::new(|| {
    let mut slots = HostSlots {
        group: std::array::from_fn(|id| id as u8),
        limit: [1; MAX_SOURCES],
        shared: [false; MAX_SOURCES],
    };
    let sources: Vec<&TileSource> = REGISTRY.by_id.iter().flatten().collect();
    for source in &sources {
        let id = usize::from(source.id);
        let peers: Vec<&&TileSource> = sources
            .iter()
            .filter(|other| other.host().is_some() && other.host() == source.host())
            .collect();
        slots.group[id] = peers.iter().map(|peer| peer.id).min().unwrap_or(source.id);
        slots.limit[id] = peers
            .iter()
            .map(|peer| usize::from(peer.max_parallel))
            .max()
            .unwrap_or(usize::from(source.max_parallel));
        slots.shared[id] = peers.len() > 1;
    }
    slots
});

impl TileSource {
    /// The host its tiles come from (with a port when the template names one).
    pub fn host(&self) -> Option<&str> {
        match &self.access {
            Access::Xyz { template } => crate::network::host_of_url(template),
            Access::Wayback => Some(crate::network::WAYBACK_TILE_HOST),
        }
    }

    /// The address of tile `z/x/y`, for template sources.
    /// The edition shown on `date` (`YYYY-MM-DD`): the newest on or before it, the
    /// oldest for days before all of them; 0 for undated sources.
    pub fn edition_for(&self, date: &str) -> u16 {
        let index = self
            .editions
            .iter()
            .position(|edition| edition.as_str() <= date)
            .unwrap_or(self.editions.len().saturating_sub(1));
        u16::try_from(index).unwrap_or(0)
    }

    /// The year of an edition, for credits (`None` for undated sources).
    pub fn edition_year(&self, edition: u16) -> Option<&str> {
        self.editions
            .get(usize::from(edition))
            .map(|date| &date[..4])
    }

    pub fn tile_url(&self, z: u8, x: u32, y: u32, edition: u16) -> Option<String> {
        match &self.access {
            Access::Xyz { template } => {
                let mut url = template
                    .replace("{z}", &z.to_string())
                    .replace("{x}", &x.to_string())
                    .replace("{y}", &y.to_string());
                if let Some(date) = self.editions.get(usize::from(edition)) {
                    url = url.replace("{date}", date);
                }
                Some(url)
            }
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
            assert!(source.max_parallel >= 1);
            if let Access::Xyz { template } = &source.access {
                assert!(template.starts_with("https://"), "{}", source.code);
                for part in ["{z}", "{x}", "{y}"] {
                    assert!(template.contains(part), "{}: {part}", source.code);
                }
            }
        }
    }

    #[test]
    fn sources_on_one_host_share_its_downloads() {
        let sources: Vec<&TileSource> = REGISTRY.by_id.iter().flatten().collect();
        for a in &sources {
            for b in &sources {
                let (ia, ib) = (usize::from(a.id), usize::from(b.id));
                assert_eq!(
                    HOST_SLOTS.group[ia] == HOST_SLOTS.group[ib],
                    a.host() == b.host(),
                    "{} and {}",
                    a.code,
                    b.code
                );
            }
            let id = usize::from(a.id);
            assert!(HOST_SLOTS.limit[id] >= usize::from(a.max_parallel));
        }
        // NASA GIBS serves the night lights and both polar caps.
        let gibs: Vec<_> = sources
            .iter()
            .filter(|source| source.host() == Some("gibs.earthdata.nasa.gov"))
            .collect();
        assert!(gibs.len() >= 3);
        assert!(
            gibs.iter()
                .all(|source| HOST_SLOTS.shared[usize::from(source.id)])
        );
        let satellite = usize::from(crate::tiles::Provider::Satellite.id() as u8);
        assert!(!HOST_SLOTS.shared[satellite]);
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
            max_parallel: 4,
            revalidate_days: None,
            editions: Vec::new(),
        };
        assert_eq!(
            source.tile_url(3, 5, 2, 0).as_deref(),
            Some("https://example.org/3/2/5.png")
        );
    }

    #[test]
    fn dated_sources_pick_the_edition_for_the_day() {
        let lights = super::super::Provider::NightLights.source();
        assert_eq!(lights.editions, ["2016-01-01", "2012-01-01"]);
        assert_eq!(lights.edition_for("2026-11-04"), 0);
        assert_eq!(lights.edition_for("2016-01-01"), 0);
        assert_eq!(lights.edition_for("2015-12-31"), 1);
        assert_eq!(lights.edition_for("1990-06-01"), 1);
        assert_eq!(lights.edition_year(1), Some("2012"));
        let url = lights.tile_url(2, 1, 2, 1).unwrap();
        assert!(
            url.contains("/2012-01-01/GoogleMapsCompatible_Level8/2/2/1.png"),
            "{url}"
        );
        assert!(!lights.tile_url(2, 1, 2, 0).unwrap().contains("{date}"));
    }
}
