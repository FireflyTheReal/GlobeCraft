//! Roads from OpenStreetMap, motorways to service roads, from GlobeCraft's own road
//! tiles (`tiles`: zoom 0–12, with zoom 13 and 14 as bands of their own). No
//! account/API key. OSM is NOT a source of live vehicle speeds or jams.
mod tiles;
pub use tiles::{MAX_ALTITUDE_KM, RoadTiles};

#[derive(Clone, Debug)]
pub struct Road {
    pub name: String,
    pub kind: String,
    /// Under construction (OpenMapTiles `*_construction`).
    pub restricted: bool,
    pub points: Vec<(f64, f64)>,
}
