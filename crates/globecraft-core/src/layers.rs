//! Toggleable map layers: one identity for the ribbon, the Layers pane, persistence and the
//! assistant. Frontends own the drawing; this module only names layers and groups them.

/// A layer the user can show or hide on the Earth globe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MapLayer {
    CountryBorders,
    StateBorders,
    PlaceLabels,
    MunicipalBorders,
    Terrain,
    Grid,
    GridLabels,
    Atmosphere,
    Sunlight,
    Stars,
    /// NASA's Milky Way map behind the globe.
    MilkyWay,
    Radar,
    Clouds,
    Flights,
    Vessels,
    Satellites,
    Roads,
    /// Buildings as blocks close up (Overture Maps: OpenStreetMap, Microsoft, Google).
    Buildings,
    Traffic,
    /// City lights at night (NASA Black Marble), on the night side with Sunlight and
    /// over the whole globe, as at night, without it.
    NightLights,
    /// Railways, subways and trams (GlobeCraft's transport tiles, OpenStreetMap).
    Railways,
    /// Ferry routes.
    Ferries,
    /// Airports: names and codes, runways and taxiways close up.
    Airports,
    /// Named places of interest (museums, stations, shops…) close up.
    PointsOfInterest,
    /// Mountain peaks with their heights.
    Peaks,
    /// Rivers and canals, and the names of seas, lakes and bays.
    Waters,
    /// Parks, nature reserves and protected areas.
    Parks,
    /// NOAA's aurora forecast, glowing on the night side. Last, so the bits of saved
    /// layer sets keep their meaning.
    Aurora,
    /// Time zone borders, and the local time under the cursor.
    TimeZones,
    /// Names of the oceans, seas, exclusive economic zones and undersea features
    /// (Marine Regions).
    SeaNames,
}

/// Where a layer is listed in the Layers pane and the ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerGroup {
    Geography,
    Globe,
    Sky,
    Weather,
    LiveObjects,
    Transport,
}

impl LayerGroup {
    pub const ALL: [Self; 6] = [
        Self::Geography,
        Self::Globe,
        Self::Sky,
        Self::Weather,
        Self::LiveObjects,
        Self::Transport,
    ];
    /// Fluent key of the group heading.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Geography => "sidebar-geography-title",
            Self::Globe => "layers-globe-title",
            Self::Sky => "sidebar-sky-title",
            Self::Weather => "sidebar-weather-title",
            Self::LiveObjects => "sidebar-live-objects-title",
            Self::Transport => "sidebar-transport-title",
        }
    }
}

impl MapLayer {
    pub const ALL: [Self; 30] = [
        Self::CountryBorders,
        Self::StateBorders,
        Self::PlaceLabels,
        Self::MunicipalBorders,
        Self::Terrain,
        Self::Grid,
        Self::GridLabels,
        Self::TimeZones,
        Self::Atmosphere,
        Self::Sunlight,
        Self::Stars,
        Self::MilkyWay,
        Self::Radar,
        Self::Clouds,
        Self::Aurora,
        Self::Flights,
        Self::Vessels,
        Self::Satellites,
        Self::Roads,
        Self::Buildings,
        Self::Traffic,
        Self::NightLights,
        Self::Railways,
        Self::Ferries,
        Self::Airports,
        Self::PointsOfInterest,
        Self::Peaks,
        Self::Waters,
        Self::SeaNames,
        Self::Parks,
    ];

    /// Stable identifier used in settings files and assistant commands.
    pub fn code(self) -> &'static str {
        match self {
            Self::CountryBorders => "country_borders",
            Self::StateBorders => "state_borders",
            Self::PlaceLabels => "place_labels",
            Self::MunicipalBorders => "municipal_borders",
            Self::Terrain => "terrain",
            Self::Grid => "grid",
            Self::GridLabels => "grid_labels",
            Self::Atmosphere => "atmosphere",
            Self::Sunlight => "sunlight",
            Self::Stars => "stars",
            Self::MilkyWay => "milky_way",
            Self::Radar => "radar",
            Self::Clouds => "clouds",
            Self::Aurora => "aurora",
            Self::TimeZones => "time_zones",
            Self::SeaNames => "sea_names",
            Self::Flights => "flights",
            Self::Vessels => "vessels",
            Self::Satellites => "satellites",
            Self::Roads => "roads",
            Self::Buildings => "buildings",
            Self::Traffic => "traffic",
            Self::NightLights => "night_lights",
            Self::Railways => "railways",
            Self::Ferries => "ferries",
            Self::Airports => "airports",
            Self::PointsOfInterest => "points_of_interest",
            Self::Peaks => "peaks",
            Self::Waters => "waters",
            Self::Parks => "parks",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.code() == code)
    }

    /// Fluent key of the layer's checkbox label.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::CountryBorders => "layers-country-borders-toggle",
            Self::StateBorders => "layers-state-borders-toggle",
            Self::PlaceLabels => "layers-place-labels-toggle",
            Self::MunicipalBorders => "layers-municipal-borders-toggle",
            Self::Terrain => "layers-terrain-toggle",
            Self::Grid => "layers-grid-toggle",
            Self::GridLabels => "layers-grid-numbers-toggle",
            Self::Atmosphere => "layers-atmosphere-toggle",
            Self::Sunlight => "layers-sunlight-toggle",
            Self::Stars => "layers-stars-toggle",
            Self::MilkyWay => "layers-milky-way-toggle",
            Self::Radar => "layers-weather-toggle",
            Self::Clouds => "layers-clouds-toggle",
            Self::Aurora => "layers-aurora-toggle",
            Self::TimeZones => "layers-timezones-toggle",
            Self::SeaNames => "layers-sea-names-toggle",
            Self::Flights => "layers-flights-toggle",
            Self::Vessels => "layers-vessels-toggle",
            Self::Satellites => "layers-satellites-toggle",
            Self::Roads => "layers-roads-toggle",
            Self::Buildings => "layers-buildings-toggle",
            Self::Traffic => "layers-traffic-toggle",
            Self::NightLights => "layers-night-lights-toggle",
            Self::Railways => "layers-railways-toggle",
            Self::Ferries => "layers-ferries-toggle",
            Self::Airports => "layers-airports-toggle",
            Self::PointsOfInterest => "layers-poi-toggle",
            Self::Peaks => "layers-peaks-toggle",
            Self::Waters => "layers-waters-toggle",
            Self::Parks => "layers-parks-toggle",
        }
    }

    /// Icon name in the desktop icon set.
    pub fn icon(self) -> &'static str {
        match self {
            Self::CountryBorders => "borders",
            Self::StateBorders => "borders_states",
            Self::MunicipalBorders => "borders_fine",
            Self::PlaceLabels => "label",
            Self::Terrain => "terrain",
            Self::Grid | Self::GridLabels => "graticule",
            Self::Atmosphere => "atmosphere",
            Self::Sunlight => "sun",
            Self::Stars => "stars",
            Self::MilkyWay => "planet",
            Self::Radar => "radar",
            Self::Clouds => "cloud",
            Self::Aurora => "aurora",
            Self::TimeZones => "time_zones",
            Self::Flights => "plane",
            Self::Vessels => "ship",
            Self::Satellites => "satellite",
            Self::Roads => "road",
            Self::Buildings => "building",
            Self::Traffic => "traffic",
            Self::NightLights => "moon",
            Self::Railways => "railway",
            Self::Ferries => "ferry",
            Self::Airports => "airport",
            Self::PointsOfInterest => "poi",
            Self::Peaks => "peak",
            Self::Waters => "river",
            Self::SeaNames => "river",
            Self::Parks => "park",
        }
    }

    pub fn group(self) -> LayerGroup {
        match self {
            Self::CountryBorders
            | Self::StateBorders
            | Self::PlaceLabels
            | Self::MunicipalBorders
            | Self::PointsOfInterest
            | Self::Peaks
            | Self::Waters
            | Self::SeaNames
            | Self::Parks => LayerGroup::Geography,
            Self::Terrain | Self::Grid | Self::GridLabels | Self::TimeZones => LayerGroup::Globe,
            Self::Atmosphere
            | Self::Sunlight
            | Self::NightLights
            | Self::Stars
            | Self::MilkyWay => LayerGroup::Sky,
            Self::Radar | Self::Clouds | Self::Aurora => LayerGroup::Weather,
            Self::Flights | Self::Vessels | Self::Satellites => LayerGroup::LiveObjects,
            Self::Roads
            | Self::Buildings
            | Self::Traffic
            | Self::Railways
            | Self::Ferries
            | Self::Airports => LayerGroup::Transport,
        }
    }

    /// A layer that only refines another: listed indented under it, and inert while it is off.
    pub fn parent(self) -> Option<Self> {
        match self {
            Self::GridLabels => Some(Self::Grid),
            _ => None,
        }
    }

    /// Whether showing the layer downloads live data from a third-party service.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Radar
                | Self::Clouds
                | Self::Aurora
                | Self::Flights
                | Self::Vessels
                | Self::Satellites
                | Self::Roads
                | Self::Buildings
                | Self::Traffic
                | Self::Terrain
        )
    }

    /// Whether the layer is a live feed: data that changes by the minute, from a
    /// tracking, weather or traffic service. A build without the feeds does not offer
    /// these layers.
    pub fn is_feed(self) -> bool {
        matches!(
            self,
            Self::Radar
                | Self::Clouds
                | Self::Aurora
                | Self::Flights
                | Self::Vessels
                | Self::Satellites
                | Self::Traffic
        )
    }

    /// Live feeds from tracking and traffic services start hidden on every launch, as they always
    /// did: opening GlobeCraft never contacts them until the user asks. Terrain is a view
    /// preference and is remembered.
    pub fn starts_hidden(self) -> bool {
        self.is_live() && self != Self::Terrain
    }

    /// Visibility for a first launch, matching earlier GlobeCraft defaults.
    pub fn default_visible(self) -> bool {
        matches!(
            self,
            Self::CountryBorders
                | Self::StateBorders
                | Self::PlaceLabels
                | Self::Atmosphere
                // The sun's light and the sun in the sky (user, 2026-11-18).
                | Self::Sunlight
        )
    }

    /// Whether the layer has its own opacity (roadmap 6.1): the ones laid over the
    /// imagery as a whole picture, where seeing the map through them helps.
    pub fn has_opacity(self) -> bool {
        Self::WITH_OPACITY.contains(&self)
    }

    /// The layers with an opacity, in the order settings keep their values.
    pub const WITH_OPACITY: [Self; 5] = [
        Self::Clouds,
        Self::Radar,
        Self::Aurora,
        Self::TimeZones,
        Self::Traffic,
    ];

    /// Whether "solo" (show only this layer) hides the layer when another is soloed:
    /// the map's content, not the globe's look (relief, atmosphere, light, sky).
    pub fn soloable(self) -> bool {
        !matches!(
            self,
            Self::Terrain
                | Self::Atmosphere
                | Self::Sunlight
                | Self::Stars
                | Self::MilkyWay
                | Self::GridLabels
        )
    }

    fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// The visible set of [`MapLayer`]s, copied by value into frame inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerSet(u32);

impl Default for LayerSet {
    fn default() -> Self {
        MapLayer::ALL
            .into_iter()
            .filter(|layer| layer.default_visible())
            .collect()
    }
}

impl LayerSet {
    pub const EMPTY: Self = Self(0);

    pub fn contains(self, layer: MapLayer) -> bool {
        self.0 & layer.bit() != 0
    }

    pub fn set(&mut self, layer: MapLayer, visible: bool) {
        if visible {
            self.0 |= layer.bit();
        } else {
            self.0 &= !layer.bit();
        }
    }

    pub fn toggle(&mut self, layer: MapLayer) {
        self.0 ^= layer.bit();
    }

    /// Whether the layer is shown, counting a hidden parent as hiding it.
    pub fn effective(self, layer: MapLayer) -> bool {
        self.contains(layer) && layer.parent().is_none_or(|parent| self.contains(parent))
    }

    pub fn iter(self) -> impl Iterator<Item = MapLayer> {
        MapLayer::ALL
            .into_iter()
            .filter(move |layer| self.contains(*layer))
    }

    /// Stable codes, for settings files.
    pub fn codes(self) -> Vec<&'static str> {
        self.iter().map(MapLayer::code).collect()
    }

    /// Builds a set from codes; unknown codes are ignored so newer files load in older builds.
    pub fn from_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Self {
        codes.into_iter().filter_map(MapLayer::from_code).collect()
    }
}

impl FromIterator<MapLayer> for LayerSet {
    fn from_iter<T: IntoIterator<Item = MapLayer>>(iter: T) -> Self {
        let mut set = Self::EMPTY;
        for layer in iter {
            set.set(layer, true);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The feeds are the layers a build without the `live` feature leaves out: each
    /// asks a third-party service and starts hidden, and none is on by default.
    #[test]
    fn feeds_are_live_layers_that_start_hidden() {
        let feeds: Vec<_> = MapLayer::ALL.into_iter().filter(|l| l.is_feed()).collect();
        assert_eq!(feeds.len(), 7);
        for layer in feeds {
            assert!(layer.is_live() && layer.starts_hidden(), "{layer:?}");
            assert!(!layer.default_visible(), "{layer:?}");
        }
        // Map tiles are live data but not feeds.
        assert!(!MapLayer::Roads.is_feed() && !MapLayer::Terrain.is_feed());
    }

    #[test]
    fn codes_are_unique_and_roundtrip() {
        for layer in MapLayer::ALL {
            assert_eq!(MapLayer::from_code(layer.code()), Some(layer));
        }
        let mut codes: Vec<_> = MapLayer::ALL.iter().map(|l| l.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), MapLayer::ALL.len());
        assert!(
            MapLayer::ALL.len() <= 32,
            "LayerSet stores one bit per layer"
        );
    }

    #[test]
    fn set_operations_and_parents() {
        let mut set = LayerSet::EMPTY;
        set.set(MapLayer::GridLabels, true);
        assert!(set.contains(MapLayer::GridLabels));
        assert!(
            !set.effective(MapLayer::GridLabels),
            "grid off hides its labels"
        );
        set.toggle(MapLayer::Grid);
        assert!(set.effective(MapLayer::GridLabels));
        let restored = LayerSet::from_codes(set.codes().into_iter().chain(["unknown"]));
        assert_eq!(restored, set);
    }

    #[test]
    fn defaults_are_borders_labels_the_air_and_the_sun() {
        // Sunlight joined the defaults after v0.4.0 (user, 2026-11-18).
        let set = LayerSet::default();
        let mut shown = set.iter().collect::<Vec<_>>();
        shown.sort_by_key(|layer| layer.code());
        let mut expected = [
            MapLayer::CountryBorders,
            MapLayer::StateBorders,
            MapLayer::PlaceLabels,
            MapLayer::Atmosphere,
            MapLayer::Sunlight,
        ];
        expected.sort_by_key(|layer| layer.code());
        assert_eq!(shown, expected);
    }
}
