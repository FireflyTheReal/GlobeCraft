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
    CinematicSky,
    Radar,
    Clouds,
    Flights,
    Vessels,
    Satellites,
    Roads,
    Traffic,
    /// Borders and place names from OpenStreetMap vector tiles instead of the
    /// Natural Earth, municipal and Overpass sources (a prototype to compare).
    OsmBorderTiles,
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
    pub const ALL: [Self; 19] = [
        Self::CountryBorders,
        Self::StateBorders,
        Self::PlaceLabels,
        Self::MunicipalBorders,
        Self::Terrain,
        Self::Grid,
        Self::GridLabels,
        Self::Atmosphere,
        Self::Sunlight,
        Self::Stars,
        Self::CinematicSky,
        Self::Radar,
        Self::Clouds,
        Self::Flights,
        Self::Vessels,
        Self::Satellites,
        Self::Roads,
        Self::Traffic,
        Self::OsmBorderTiles,
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
            Self::CinematicSky => "cinematic_sky",
            Self::Radar => "radar",
            Self::Clouds => "clouds",
            Self::Flights => "flights",
            Self::Vessels => "vessels",
            Self::Satellites => "satellites",
            Self::Roads => "roads",
            Self::Traffic => "traffic",
            Self::OsmBorderTiles => "osm_border_tiles",
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
            Self::Stars => "sky-enabled",
            Self::CinematicSky => "sky-cinematic",
            Self::Radar => "layers-weather-toggle",
            Self::Clouds => "layers-clouds-toggle",
            Self::Flights => "layers-flights-toggle",
            Self::Vessels => "layers-vessels-toggle",
            Self::Satellites => "layers-satellites-toggle",
            Self::Roads => "layers-roads-toggle",
            Self::Traffic => "layers-traffic-toggle",
            Self::OsmBorderTiles => "layers-osm-border-tiles-toggle",
        }
    }

    /// Icon name in the desktop icon set.
    pub fn icon(self) -> &'static str {
        match self {
            Self::CountryBorders | Self::StateBorders => "borders",
            Self::MunicipalBorders | Self::OsmBorderTiles => "borders_fine",
            Self::PlaceLabels => "label",
            Self::Terrain => "terrain",
            Self::Grid | Self::GridLabels => "graticule",
            Self::Atmosphere => "atmosphere",
            Self::Sunlight => "sun",
            Self::Stars | Self::CinematicSky => "stars",
            Self::Radar => "radar",
            Self::Clouds => "cloud",
            Self::Flights => "plane",
            Self::Vessels => "ship",
            Self::Satellites => "satellite",
            Self::Roads => "road",
            Self::Traffic => "traffic",
        }
    }

    pub fn group(self) -> LayerGroup {
        match self {
            Self::CountryBorders
            | Self::StateBorders
            | Self::PlaceLabels
            | Self::MunicipalBorders
            | Self::OsmBorderTiles => LayerGroup::Geography,
            Self::Terrain | Self::Grid | Self::GridLabels => LayerGroup::Globe,
            Self::Atmosphere | Self::Sunlight | Self::Stars | Self::CinematicSky => LayerGroup::Sky,
            Self::Radar | Self::Clouds => LayerGroup::Weather,
            Self::Flights | Self::Vessels | Self::Satellites => LayerGroup::LiveObjects,
            Self::Roads | Self::Traffic => LayerGroup::Transport,
        }
    }

    /// A layer that only refines another: listed indented under it, and inert while it is off.
    pub fn parent(self) -> Option<Self> {
        match self {
            Self::GridLabels => Some(Self::Grid),
            Self::CinematicSky => Some(Self::Stars),
            _ => None,
        }
    }

    /// Whether showing the layer downloads live data from a third-party service.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Radar
                | Self::Clouds
                | Self::Flights
                | Self::Vessels
                | Self::Satellites
                | Self::Roads
                | Self::Traffic
                | Self::Terrain
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
            Self::CountryBorders | Self::StateBorders | Self::PlaceLabels | Self::Atmosphere
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
    fn defaults_match_previous_release() {
        let set = LayerSet::default();
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [
                MapLayer::CountryBorders,
                MapLayer::StateBorders,
                MapLayer::PlaceLabels,
                MapLayer::Atmosphere
            ]
        );
    }
}
