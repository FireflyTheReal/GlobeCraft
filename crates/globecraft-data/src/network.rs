//! The network policy: every built-in service, the hosts it uses, the user agent
//! sent to it and the request limit applied to it (roadmap 10.6).
//!
//! The table is deliberately data-owned: the desktop only formats it. Tile sources
//! keep their hosts in `tiles/sources.json`; [`tile_services`] reads those entries so
//! a new source cannot silently miss the Settings page. User-added layers and the
//! assistant provider are dynamic, so the app lists their hosts from the saved
//! addresses instead of pretending they are built in.

use crate::tiles::{Access, Prefetch, Provider};
use globecraft_core::language::Lang;

/// The public user agents. Keeping them here means the table and the HTTP clients
/// cannot drift apart.
pub const TILE_USER_AGENT: &str =
    "GlobeCraft/0.2 (educational 3D globe viewer; +https://github.com/FireflyTheReal/GlobeCraft)";
pub(crate) const BOUNDARY_USER_AGENT: &str = "GlobeCraft/0.1 (map boundary overlay)";
pub(crate) const WAYBACK_USER_AGENT: &str = "GlobeCraft/0.2 (wayback release list)";
pub(crate) const SEARCH_USER_AGENT: &str =
    "GlobeCraft/0.2 (educational 3D globe viewer; +https://github.com/FireflyTheReal/GlobeCraft)";
pub(crate) const SUGGEST_USER_AGENT: &str = "GlobeCraft/0.2 (search suggestions)";
pub(crate) const CLOUD_USER_AGENT: &str = "GlobeCraft/1.0 (attribution: Matt Eason, EUMETSAT)";
pub(crate) const RADAR_USER_AGENT: &str = "GlobeCraft/1.0 radar viewer";
pub(crate) const TRAFFIC_USER_AGENT: &str = "GlobeCraft/1.0 (traffic; TomTom)";
pub(crate) const AIRCRAFT_USER_AGENT: &str =
    "GlobeCraft/1.0 (live aircraft viewer; OpenSky / ADSB.lol)";
pub(crate) const VESSELS_USER_AGENT: &str =
    "GlobeCraft/1.0 (live vessel viewer; OpenSeaFeed / Digitraffic)";
pub(crate) const AURORA_USER_AGENT: &str = "GlobeCraft/1.0 (aurora forecast; NOAA SWPC)";
pub(crate) const SATELLITES_USER_AGENT: &str = "GlobeCraft/1.0 (satellite orbits; CelesTrak)";
pub const USER_DATA_USER_AGENT: &str =
    "GlobeCraft/0.3 (3D globe; user-added layer; +https://github.com/FireflyTheReal/GlobeCraft)";
pub const ASSISTANT_USER_AGENT: &str =
    concat!("GlobeCraft/", env!("CARGO_PKG_VERSION"), " (AI assistant)");

/// The host of Esri Wayback's tile service; its release list lives on S3.
const WAYBACK_TILE_HOST: &str = "wayback.maptiles.arcgis.com";

/// How often, or how many at once, a service is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestLimit {
    /// At most this many requests in flight.
    Parallel(u8),
    /// At most one request every this many seconds.
    Every(u64),
    /// Only when the user asks, or when the layer becomes visible.
    OnDemand,
    /// Once per application session.
    Once,
    /// Set by the user, for example a live feed's reload interval.
    UserControlled,
}

/// Where the Settings page groups a service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Basemap,
    MapData,
    Search,
    Live,
    Updates,
    UserData,
    Assistant,
}
impl Category {
    pub const ALL: [Self; 7] = [
        Self::Basemap,
        Self::MapData,
        Self::Search,
        Self::Live,
        Self::Updates,
        Self::UserData,
        Self::Assistant,
    ];
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Basemap => "settings-network-group-basemaps",
            Self::MapData => "settings-network-group-map-data",
            Self::Search => "settings-network-group-search",
            Self::Live => "settings-network-group-live",
            Self::Updates => "settings-network-group-updates",
            Self::UserData => "settings-network-group-user",
            Self::Assistant => "settings-network-group-assistant",
        }
    }
}

/// One built-in service that is not a tile source.
#[derive(Clone, Copy, Debug)]
pub struct Service {
    /// Stable identifier for tests and documentation.
    pub id: &'static str,
    /// English product name, or the fallback when `name_key` is set.
    pub name: &'static str,
    /// Fluent id for names that are prose rather than a product name.
    pub name_key: Option<&'static str>,
    pub category: Category,
    pub hosts: &'static [&'static str],
    /// `None` when the transport sets its own user agent (the updater crate).
    pub user_agent: Option<&'static str>,
    pub limit: RequestLimit,
}
impl Service {
    pub fn name(&self, lang: Lang) -> &'static str {
        self.name_key.map_or(self.name, |key| lang.text(key))
    }
}

/// The built-in services other than tile sources. Tile hosts live in
/// `tiles/sources.json` and are exposed by [`tile_services`].
pub static SERVICES: &[Service] = &[
    Service {
        id: "wayback-config",
        name: "Esri Wayback release list",
        name_key: Some("settings-network-service-wayback"),
        category: Category::MapData,
        hosts: &["s3-us-west-2.amazonaws.com"],
        user_agent: Some(WAYBACK_USER_AGENT),
        limit: RequestLimit::Once,
    },
    Service {
        id: "map-data",
        name: "GlobeCraft map data",
        name_key: Some("settings-network-service-map-data"),
        category: Category::MapData,
        hosts: &[
            "github.com",
            "objects.githubusercontent.com",
            "release-assets.githubusercontent.com",
        ],
        user_agent: Some(BOUNDARY_USER_AGENT),
        limit: RequestLimit::OnDemand,
    },
    Service {
        id: "nominatim",
        name: "OpenStreetMap Nominatim",
        name_key: None,
        category: Category::Search,
        hosts: &["nominatim.openstreetmap.org"],
        user_agent: Some(SEARCH_USER_AGENT),
        limit: RequestLimit::OnDemand,
    },
    Service {
        id: "photon",
        name: "Photon (Komoot)",
        name_key: None,
        category: Category::Search,
        hosts: &["photon.komoot.io"],
        user_agent: Some(SUGGEST_USER_AGENT),
        limit: RequestLimit::OnDemand,
    },
    Service {
        id: "opensky",
        name: "OpenSky Network",
        name_key: None,
        category: Category::Live,
        hosts: &["opensky-network.org"],
        user_agent: Some(AIRCRAFT_USER_AGENT),
        limit: RequestLimit::Every(17 * 60),
    },
    Service {
        id: "adsb-lol",
        name: "ADSB.lol",
        name_key: None,
        category: Category::Live,
        hosts: &["api.adsb.lol", "globe.adsb.lol"],
        user_agent: Some(AIRCRAFT_USER_AGENT),
        limit: RequestLimit::Every(10),
    },
    Service {
        id: "openseafeed",
        name: "OpenSeaFeed AIS",
        name_key: None,
        category: Category::Live,
        hosts: &["api.openseafeed.com"],
        user_agent: Some(VESSELS_USER_AGENT),
        limit: RequestLimit::Every(120),
    },
    Service {
        id: "digitraffic",
        name: "Digitraffic AIS",
        name_key: None,
        category: Category::Live,
        hosts: &["meri.digitraffic.fi"],
        user_agent: Some(VESSELS_USER_AGENT),
        limit: RequestLimit::Every(120),
    },
    Service {
        id: "noaa-swpc",
        name: "NOAA SWPC",
        name_key: None,
        category: Category::Live,
        hosts: &["services.swpc.noaa.gov"],
        user_agent: Some(AURORA_USER_AGENT),
        limit: RequestLimit::Every(10 * 60),
    },
    Service {
        id: "celestrak",
        name: "CelesTrak",
        name_key: None,
        category: Category::Live,
        hosts: &["celestrak.org"],
        user_agent: Some(SATELLITES_USER_AGENT),
        limit: RequestLimit::Every(6 * 60 * 60),
    },
    Service {
        id: "rainviewer",
        name: "RainViewer",
        name_key: None,
        category: Category::Live,
        hosts: &["api.rainviewer.com", "tilecache.rainviewer.com"],
        user_agent: Some(RADAR_USER_AGENT),
        limit: RequestLimit::Every(10 * 60),
    },
    Service {
        id: "clouds",
        name: "Live Cloud Maps (Matt Eason, EUMETSAT)",
        name_key: Some("settings-network-service-clouds"),
        category: Category::Live,
        hosts: &["clouds.matteason.co.uk"],
        user_agent: Some(CLOUD_USER_AGENT),
        limit: RequestLimit::Every(3 * 60 * 60),
    },
    Service {
        id: "tomtom",
        name: "TomTom Traffic",
        name_key: None,
        category: Category::Live,
        hosts: &["api.tomtom.com"],
        user_agent: Some(TRAFFIC_USER_AGENT),
        limit: RequestLimit::Every(120),
    },
    Service {
        id: "updates",
        name: "GlobeCraft updates",
        name_key: Some("settings-network-service-updates"),
        category: Category::Updates,
        hosts: &[
            "api.github.com",
            "github.com",
            "objects.githubusercontent.com",
            "release-assets.githubusercontent.com",
        ],
        user_agent: None,
        limit: RequestLimit::Once,
    },
];

/// One tile source, as shown in the Settings page.
#[derive(Clone, Copy, Debug)]
pub struct TileService {
    pub provider: Provider,
    /// `None` only if a future access kind has no host.
    pub host: Option<&'static str>,
    pub user_agent: &'static str,
    pub limit: RequestLimit,
    /// The source's prefetch policy: on-demand sources request only visible tiles.
    pub on_demand: bool,
}
impl TileService {
    pub fn name(&self, lang: Lang) -> &'static str {
        self.provider.name(lang)
    }
}

/// Every built-in tile source, in listing order.
pub fn tile_services() -> impl Iterator<Item = TileService> {
    Provider::all().map(|provider| {
        let source = provider.source();
        let host = match &source.access {
            Access::Xyz { template } => host_of_url(template),
            Access::Wayback => Some(WAYBACK_TILE_HOST),
        };
        TileService {
            provider,
            host,
            user_agent: TILE_USER_AGENT,
            limit: RequestLimit::Parallel(source.max_parallel),
            on_demand: source.prefetch == Prefetch::OnDemand,
        }
    })
}

/// The authority of an HTTP(S) address, with a port when one is given, or `None`
/// for anything that is not an absolute lower-case HTTP(S) URL. This is display
/// logic, not URL validation: layer loading applies its own checks.
pub fn host_of_url(url: &str) -> Option<&str> {
    let url = url.trim();
    let scheme_len = if url.len() >= 8
        && url
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        8
    } else if url.len() >= 7
        && url
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
    {
        7
    } else {
        return None;
    };
    let rest = &url[scheme_len..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    (!host.is_empty() && !host.starts_with(':') && !host.contains(char::is_whitespace))
        .then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn services_have_unique_ids_hosts_and_globe_craft_agents() {
        let mut ids = HashSet::new();
        for service in SERVICES {
            assert!(ids.insert(service.id), "duplicate id {}", service.id);
            assert!(!service.name.is_empty(), "{}", service.id);
            assert!(!service.hosts.is_empty(), "{}", service.id);
            for host in service.hosts {
                assert!(valid_host(host), "{}: {host}", service.id);
            }
            if let Some(agent) = service.user_agent {
                assert!(agent.starts_with("GlobeCraft/"), "{}: {agent}", service.id);
                assert!(!agent.contains(['\r', '\n']), "{}", service.id);
            }
        }
    }

    #[test]
    fn every_settings_group_has_something_to_list() {
        for category in Category::ALL {
            match category {
                Category::Basemap => assert!(tile_services().next().is_some()),
                Category::UserData | Category::Assistant => {}
                _ => assert!(
                    SERVICES.iter().any(|service| service.category == category),
                    "{category:?} has no service"
                ),
            }
        }
    }

    #[test]
    fn every_tile_source_has_a_host_and_its_parallel_limit() {
        let sources: Vec<_> = tile_services().collect();
        assert_eq!(sources.len(), Provider::all().count());
        for service in sources {
            assert!(
                service.host.is_some(),
                "{} has no host",
                service.provider.code()
            );
            assert_eq!(
                service.limit,
                RequestLimit::Parallel(service.provider.source().max_parallel)
            );
            assert!(service.user_agent.starts_with("GlobeCraft/"));
        }
    }

    #[test]
    fn host_extraction_keeps_ports_and_drops_paths_queries_and_credentials() {
        assert_eq!(
            host_of_url("https://tiles.example.org/{z}/{x}/{y}.png"),
            Some("tiles.example.org")
        );
        assert_eq!(
            host_of_url("https://user:pass@example.org:8443/wms?x=1"),
            Some("example.org:8443")
        );
        assert_eq!(
            host_of_url("http://localhost:11434/v1"),
            Some("localhost:11434")
        );
        assert_eq!(host_of_url("https://[::1]:8080/v1"), Some("[::1]:8080"));
        assert_eq!(host_of_url("https://example.org?x=1"), Some("example.org"));
        assert_eq!(host_of_url("HTTPS://EXAMPLE.ORG/a"), Some("EXAMPLE.ORG"));
        assert_eq!(host_of_url("https://example .org/a"), None);
        assert_eq!(host_of_url("file:///tmp/a.geojson"), None);
        assert_eq!(host_of_url("https://"), None);
        assert_eq!(host_of_url("example.org/a"), None);
    }

    #[test]
    fn table_hosts_match_the_addresses_the_services_use() {
        let wayback = SERVICES
            .iter()
            .find(|service| service.id == "wayback-config")
            .unwrap();
        assert_eq!(
            host_of_url(crate::wayback::WAYBACK_CONFIG_URL),
            Some(wayback.hosts[0])
        );
        let map_data = SERVICES
            .iter()
            .find(|service| service.id == "map-data")
            .unwrap();
        assert!(map_data.hosts.contains(&"github.com"));
        assert_eq!(
            host_of_url(crate::map_data::MANIFEST_URL),
            Some(map_data.hosts[0])
        );
        assert_eq!(
            host_of_url(crate::updater::RELEASES_URL),
            Some("github.com")
        );
    }

    fn valid_host(host: &str) -> bool {
        !host.is_empty()
            && !host.contains(['/', '?', '#', ' ', '\t', '\r', '\n'])
            && !host.contains("://")
    }
}
