//! Place names close up: GlobeCraft's `places` set holds OpenMapTiles' `place` layer
//! at zoom 13 and 14 (the border tiles stop at 12), with the hamlets, suburbs,
//! neighbourhoods and localities a street-level view names. Streamed for the view like
//! the road tiles; where a tile of it is loaded its names replace the border tiles'
//! names, which hold the same places and fewer.
use super::MapLabel;
use super::osm_tiles::content::{TileBounds, TileKey};
use crate::vector_tiles::{Snapshot, Spec, TileStream, View};
use std::sync::Arc;

/// Tiles across the view: as the border tiles' finer choice, so zoom 13 takes over
/// from the border tiles' zoom 12 where those would go finer.
const TILES_ACROSS_VIEW: f64 = 4.0;
/// Above this the border tiles' names are enough (zoom 13 needs a view a few tiles
/// wide).
pub const MAX_ALTITUDE_KM: f64 = 40.0;

static SPEC: Spec<PlaceTile> = Spec {
    set: "places",
    bands: &[],
    min_zoom: 13,
    max_zoom: 14,
    skip_zoom: None,
    tiles_across_view: TILES_ACROSS_VIEW,
    max_tiles: 64,
    max_floor_tiles: 64,
    max_cached_tiles: 400,
    max_cached_bytes: 16 * 1024 * 1024,
    size_bytes,
    decode,
};

/// One tile's names.
pub struct PlaceTile {
    bounds: TileBounds,
    labels: Vec<MapLabel>,
}

fn size_bytes(tile: &PlaceTile) -> usize {
    std::mem::size_of::<PlaceTile>()
        + tile
            .labels
            .iter()
            .map(|label| {
                std::mem::size_of::<MapLabel>() + label.en.capacity() + label.pt.capacity()
            })
            .sum::<usize>()
}

fn decode(key: TileKey, bytes: &[u8]) -> Option<PlaceTile> {
    Some(PlaceTile {
        bounds: key.bounds(),
        labels: super::osm_tiles::place_labels(key, bytes)?,
    })
}

/// The zoom 13–14 place names around the view.
#[derive(Default)]
pub struct PlaceNameTiles {
    stream: Option<TileStream<PlaceTile>>,
}

impl PlaceNameTiles {
    /// `view`: latitude, longitude, altitude (km) and tilt (degrees).
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        enabled: bool,
        view: (f64, f64, f64, f64),
        repaint: impl Fn() + Send + Sync + Clone + 'static,
    ) {
        let stream = self.stream.get_or_insert_with(|| TileStream::new(&SPEC));
        let (latitude, longitude, altitude_km, tilt_deg) = view;
        if !enabled || altitude_km > MAX_ALTITUDE_KM {
            stream.hide();
            return;
        }
        stream.update(
            runtime,
            View {
                latitude,
                longitude,
                altitude_km,
                tilt_deg,
                floor: SPEC.min_zoom,
            },
            repaint,
        );
    }

    /// The names on show: the same snapshot while the tiles do not change.
    pub fn snapshot(&self) -> Option<Arc<Snapshot<PlaceTile>>> {
        self.stream.as_ref().and_then(TileStream::shared_snapshot)
    }
}

impl super::BoundaryData {
    /// These boundaries with the close-up names of `places`: where a place tile is
    /// loaded its settlement names replace the border tiles' ones; state and bundled
    /// names stay.
    pub fn with_place_names(&self, places: Option<&Snapshot<PlaceTile>>) -> Self {
        let mut data = self.clone();
        let Some(places) = places else {
            return data;
        };
        let covered = |label: &MapLabel| {
            places.tiles.iter().any(|tile| {
                let b = tile.bounds;
                (b.south..b.north).contains(&label.lat) && (b.west..b.east).contains(&label.lon)
            })
        };
        // Only the settlement names are swapped: state names stay with the border tiles.
        let settlement = |label: &MapLabel| label.kind == super::LabelKind::City;
        data.labels
            .retain(|label| !label.from_tiles || !settlement(label) || !covered(label));
        data.labels.extend(
            places
                .tiles
                .iter()
                .flat_map(|tile| tile.labels.iter().filter(|l| settlement(l)).cloned()),
        );
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundaries::{BoundaryData, LabelKind};

    fn label(lat: f64, lon: f64, name: &str, from_tiles: bool) -> MapLabel {
        MapLabel {
            lat,
            lon,
            en: name.into(),
            pt: name.into(),
            kind: LabelKind::City,
            rank: crate::boundaries::PLACE_VILLAGE_RANK,
            big: false,
            from_tiles,
        }
    }

    #[test]
    fn close_up_names_replace_the_border_tiles_names_where_loaded() {
        let key = TileKey::at(13, 48.137, 11.575);
        let inside = key.bounds();
        let (lat, lon) = (
            (inside.south + inside.north) / 2.0,
            (inside.west + inside.east) / 2.0,
        );
        let data = BoundaryData {
            labels: vec![
                label(lat, lon, "Village from zoom 12", true),
                label(lat + 1.0, lon, "Outside", true),
                label(lat, lon, "Bundled", false),
            ],
            ..Default::default()
        };
        let snapshot = Snapshot {
            tiles: vec![Arc::new(PlaceTile {
                bounds: inside,
                labels: vec![label(lat, lon, "Neighbourhood", true)],
            })],
            zoom: 13,
            fades: Vec::new(),
        };
        let names: Vec<String> = data
            .with_place_names(Some(&snapshot))
            .labels
            .into_iter()
            .map(|label| label.en)
            .collect();
        assert_eq!(names, ["Outside", "Bundled", "Neighbourhood"]);
        assert_eq!(data.with_place_names(None).labels.len(), 3);
    }
}

#[cfg(test)]
mod probe {
    use crate::boundaries::osm_tiles::content::TileKey;

    /// What the published zoom 13 and 14 place tiles name around a few places, by
    /// label rank, against the border tiles' zoom 12.
    #[tokio::test]
    #[ignore = "network: reads GlobeCraft's published place tiles"]
    async fn published_close_up_names() {
        let places = [
            ("Munich", 48.14, 11.56),
            ("Lisbon", 38.72, -9.14),
            ("Tokyo", 35.68, 139.76),
        ];
        for (set, zooms) in [("borders", [12u8].as_slice()), ("places", &[13, 14])] {
            let template = crate::map_data::template(set).await.unwrap();
            let archives = template
                .strip_prefix(crate::map_data::PMTILES_PREFIX)
                .unwrap()
                .to_owned();
            for &z in zooms {
                let keys: Vec<TileKey> = places
                    .iter()
                    .map(|&(_, lat, lon)| TileKey::at(z, lat, lon))
                    .collect();
                let wanted: Vec<_> = keys.iter().map(|k| (k.z, k.x, k.y)).collect();
                let parts = crate::map_data::tile_parts_many(&wanted, &archives)
                    .await
                    .unwrap();
                for ((name, _, _), (key, parts)) in places.iter().zip(keys.into_iter().zip(parts)) {
                    let mut bytes = Vec::new();
                    for part in parts {
                        bytes.extend(crate::map_data::gunzip_if_needed(part).unwrap());
                    }
                    let labels =
                        crate::boundaries::osm_tiles::place_labels(key, &bytes).unwrap_or_default();
                    let mut ranks = std::collections::BTreeMap::<u8, usize>::new();
                    for label in &labels {
                        *ranks.entry(label.rank).or_default() += 1;
                    }
                    let sample: Vec<&str> = labels.iter().take(6).map(|l| l.en.as_str()).collect();
                    println!(
                        "{set} z{z} {name}: {} names, by rank {ranks:?}, e.g. {sample:?}",
                        labels.len()
                    );
                }
            }
        }
    }
}
