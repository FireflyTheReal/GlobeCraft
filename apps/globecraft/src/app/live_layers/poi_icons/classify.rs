//! Which badge a map-data name gets: its colour group and symbol (roadmap 3.8).
use super::Group;
use super::symbols::Symbol;
use globecraft_data::poi::{PoiPoint, PointKind};

/// The group and symbol of a place of interest: its OpenMapTiles `subclass` when
/// that names something more specific than the class (a bookshop is in the
/// `library` class, a marina in `harbor`), else its `class`.
pub(in crate::app::live_layers) fn classify(class: &str, subclass: &str) -> (Group, Symbol) {
    by_subclass(subclass).unwrap_or_else(|| by_class(class))
}

/// What a name of the map-data layers shows on a badge: places of interest by
/// their kind, peaks, parks, and lakes and other inland waters. Seas, oceans and
/// bays get none: their names stand alone, larger, over the water.
pub(in crate::app::live_layers) fn point_badge(point: &PoiPoint) -> Option<(Group, Symbol)> {
    match point.kind {
        PointKind::Poi => Some(classify(&point.class, &point.subclass)),
        PointKind::Peak => Some((Group::Terrain, Symbol::Mountain)),
        PointKind::Park => Some((Group::Nature, Symbol::Tree)),
        PointKind::Water if is_open_water(&point.class) => None,
        PointKind::Water => Some((Group::Water, Symbol::Water)),
    }
}

/// Water names written large, without a badge.
pub(in crate::app::live_layers) fn is_open_water(class: &str) -> bool {
    matches!(class, "ocean" | "sea" | "bay")
}

/// Subclasses whose symbol differs from their class's.
fn by_subclass(subclass: &str) -> Option<(Group, Symbol)> {
    use Group as G;
    use Symbol as S;
    Some(match subclass {
        "books" | "stationery" => (G::Shopping, S::Book),
        "library" => (G::Education, S::Book),
        "charity" | "second_hand" | "supermarket" | "convenience" | "greengrocer" | "butcher"
        | "kiosk" | "mall" | "department_store" => (G::Shopping, S::Shop),
        "ice_cream" | "bakery" | "coffee" => (G::Food, S::Cafe),
        "pub" | "biergarten" | "nightclub" => (G::Food, S::Bar),
        "cinema" | "theatre" => (G::Culture, S::Theatre),
        "gallery" | "art_gallery" => (G::Culture, S::Museum),
        "dentist" | "clinic" | "hospital" | "doctors" | "veterinary" => (G::Health, S::Plus),
        "atm" | "bank" | "bureau_de_change" => (G::Services, S::Bank),
        "charging_station" | "fuel" => (G::Transport, S::Fuel),
        "bus_stop" | "bus_station" => (G::Transport, S::Bus),
        "station" | "halt" | "tram_stop" | "subway_entrance" | "subway" => (G::Transport, S::Train),
        "marina" | "ferry_terminal" | "dock" | "slipway" => (G::Transport, S::Boat),
        "bicycle_rental" | "bicycle_parking" | "bicycle" => (G::Transport, S::Bicycle),
        "playground" | "garden" | "park" => (G::Nature, S::Tree),
        "peak" | "volcano" => (G::Terrain, S::Mountain),
        _ => return None,
    })
}

/// The group and symbol of an OpenMapTiles `poi` class.
fn by_class(class: &str) -> (Group, Symbol) {
    use Group as G;
    use Symbol as S;
    match class {
        "restaurant" | "fast_food" | "food_court" => (G::Food, S::Restaurant),
        "cafe" | "ice_cream" | "bakery" => (G::Food, S::Cafe),
        "bar" | "beer" | "pub" | "biergarten" | "nightclub" => (G::Food, S::Bar),
        "shop" | "grocery" | "clothing_store" | "alcohol_shop" | "furniture" | "gift"
        | "jewelry" | "shoe" | "laundry" | "bicycle" | "car" | "hairdresser" => {
            (G::Shopping, S::Shop)
        }
        "lodging" | "hotel" | "hostel" | "motel" | "guest_house" => (G::Lodging, S::Bed),
        "museum" | "art_gallery" | "castle" | "monument" | "town_hall" => (G::Culture, S::Museum),
        "attraction" | "information" | "artwork" => (G::Culture, S::Star),
        "theatre" | "cinema" | "music" | "arts_centre" => (G::Culture, S::Theatre),
        "place_of_worship" | "religion" => (G::Worship, S::Worship),
        "hospital" | "doctors" | "doctor" | "dentist" | "clinic" | "veterinary" => {
            (G::Health, S::Plus)
        }
        "pharmacy" | "chemist" => (G::Health, S::Pill),
        "school" | "college" | "university" | "kindergarten" => (G::Education, S::School),
        "library" => (G::Education, S::Book),
        "bank" | "atm" | "bureau_de_change" => (G::Services, S::Bank),
        "fuel" | "charging_station" => (G::Transport, S::Fuel),
        "parking" => (G::Transport, S::Parking),
        "bicycle_parking" | "bicycle_rental" => (G::Transport, S::Bicycle),
        "bus" | "aerialway" => (G::Transport, S::Bus),
        "railway" => (G::Transport, S::Train),
        "harbor" | "ferry_terminal" => (G::Transport, S::Boat),
        "toilets" => (G::Services, S::Toilets),
        "viewpoint" => (G::Nature, S::Viewpoint),
        "campsite" | "shelter" => (G::Nature, S::Tent),
        "park" | "garden" | "zoo" | "dog_park" | "playground" | "golf" => (G::Nature, S::Tree),
        "stadium" | "sports" | "pitch" => (G::Nature, S::Ball),
        "swimming" | "drinking_water" | "water_park" => (G::Nature, S::Water),
        "police" | "fire_station" | "post" | "office" | "townhall" | "prison" => {
            (G::Services, S::Building)
        }
        _ => (G::Services, S::Dot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_classes_get_their_own_symbol_and_unknown_ones_a_dot() {
        assert_eq!(
            classify("restaurant", ""),
            (Group::Food, Symbol::Restaurant)
        );
        assert_eq!(classify("pharmacy", ""), (Group::Health, Symbol::Pill));
        assert_eq!(classify("railway", "").0, Group::Transport);
        assert_eq!(
            classify("something_new", ""),
            (Group::Services, Symbol::Dot)
        );
    }

    #[test]
    fn a_specific_subclass_wins_over_its_class() {
        // OpenMapTiles files bookshops (an Oxfam bookshop) under `library`.
        assert_eq!(
            classify("library", "books"),
            (Group::Shopping, Symbol::Book)
        );
        assert_eq!(
            classify("library", "library"),
            (Group::Education, Symbol::Book)
        );
        assert_eq!(
            classify("harbor", "marina"),
            (Group::Transport, Symbol::Boat)
        );
        assert_eq!(
            classify("railway", "tram_stop"),
            (Group::Transport, Symbol::Train)
        );
        assert_eq!(classify("bus", "bus_stop"), (Group::Transport, Symbol::Bus));
        // Unknown subclasses keep the class's symbol.
        assert_eq!(classify("shop", "hifi"), (Group::Shopping, Symbol::Shop));
    }

    #[test]
    fn peaks_parks_and_inland_waters_get_badges_and_seas_none() {
        let point = |kind, class: &str| PoiPoint {
            kind,
            lat: 0.0,
            lon: 0.0,
            name: "Name".into(),
            class: class.into(),
            subclass: String::new(),
            rank: 0,
            elevation_m: None,
        };
        assert_eq!(
            point_badge(&point(PointKind::Peak, "")),
            Some((Group::Terrain, Symbol::Mountain))
        );
        assert_eq!(
            point_badge(&point(PointKind::Park, "national_park")),
            Some((Group::Nature, Symbol::Tree))
        );
        assert_eq!(
            point_badge(&point(PointKind::Water, "lake")),
            Some((Group::Water, Symbol::Water))
        );
        assert_eq!(point_badge(&point(PointKind::Water, "sea")), None);
    }
}
