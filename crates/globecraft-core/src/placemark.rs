#[derive(Clone, Debug, PartialEq)]
pub struct Placemark {
    pub name: String,
    pub desc: String,
    pub lat: f64,
    pub lon: f64,
    /// When the place is (KML `TimeStamp` or `TimeSpan`): it shows on the timeline
    /// then only.
    pub time: Option<crate::time::PlaceTime>,
}

mod tree;
pub use tree::{PlaceItem, PlaceNode, Places};
