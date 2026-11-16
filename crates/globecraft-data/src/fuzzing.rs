//! Entry points for the fuzz targets in `fuzz/` (roadmap 10.5): every decoder of data
//! GlobeCraft does not control — files a user opens, tiles and feeds from the network,
//! archives named in a layer. Each function feeds its input to the decoders exactly as
//! the application does and returns nothing: a panic, a hang or a runaway allocation is
//! the finding. Where a cheap property must hold for any input (coordinates in range,
//! an encoding that reads back), it is asserted here.
//!
//! Compiled only with the `fuzzing` feature; see `fuzz/README.md`.
use crate::boundaries::osm_tiles::content::TileKey;
use globecraft_core::language::Lang;

/// Splits a selector byte off the input, for targets that cover several decoders.
fn selector(data: &[u8]) -> (u8, &[u8]) {
    data.split_first()
        .map_or((0, data), |(first, rest)| (*first, rest))
}

/// A tile key taken from the first bytes: zoom 0–24 and a column and row inside it,
/// as the tile streams only ever ask for valid tiles.
fn tile_key(data: &[u8]) -> (TileKey, &[u8]) {
    if data.len() < 9 {
        return (TileKey { z: 0, x: 0, y: 0 }, data);
    }
    let z = data[0] % 25;
    let side = TileKey::tiles_per_side(z);
    let word = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    (
        TileKey {
            z,
            x: word(1) % side,
            y: word(5) % side,
        },
        &data[9..],
    )
}

fn finite_position(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

/// A KML document's text (File › Open, and the restored session).
pub fn kml(text: &str) {
    for lang in Lang::ALL {
        let _ = crate::kml::parse_tree(text, lang);
    }
}

/// A KMZ archive (a zip holding a KML document).
pub fn kmz(bytes: &[u8]) {
    let _ = crate::kml::parse_kmz(bytes, Lang::En);
}

/// A GeoJSON, GPX or CSV feature file or feed (Add Data); the first byte picks the
/// format and whether a value field is named.
pub fn features(data: &[u8]) {
    use crate::user_data::FeatureFormat;
    let (pick, bytes) = selector(data);
    let format = match pick % 3 {
        0 => FeatureFormat::GeoJson,
        1 => FeatureFormat::Gpx,
        _ => FeatureFormat::Csv,
    };
    let value_field = (pick & 0x80 != 0).then_some("mag");
    if let Ok(set) = crate::user_data::parse_features(format, bytes, value_field) {
        for point in &set.points {
            assert!(finite_position(point.lat, point.lon), "point out of range");
        }
        for line in &set.lines {
            for &(lat, lon) in line.iter() {
                assert!(finite_position(lat, lon), "line vertex out of range");
            }
        }
    }
}

/// A WMS `GetCapabilities` answer (Add Data's layer list).
pub fn wms_capabilities(bytes: &[u8]) {
    let _ = crate::user_data::capabilities::parse(bytes);
}

/// A PMTiles archive's first bytes: the header, then the rest as a directory in the
/// header's compression, searched for a few tile ids.
pub fn pmtiles(data: &[u8]) {
    use crate::pmtiles;
    let header = pmtiles::Header::parse(data);
    if let Ok(header) = &header {
        let _ = header.root_in(data);
        let _ = crate::user_data::archive::pmtiles_info(header.zooms(), header.center());
        // Any zoom byte and column, also ones no map has.
        for (z, x) in [
            (0, 0),
            (5, 17),
            (14, 8_717),
            (31, 1),
            (32, 0),
            (255, u32::MAX),
        ] {
            let _ = header.may_hold(z, x);
        }
    }
    let (pick, rest) = selector(data);
    let compression = if pick & 1 == 0 {
        pmtiles::Compression::None
    } else {
        pmtiles::Compression::Gzip
    };
    let Ok(bytes) = pmtiles::decompress(rest.to_vec(), compression) else {
        return;
    };
    let Ok(entries) = pmtiles::parse_directory(&bytes) else {
        return;
    };
    for id in [0, 1, 1_000, u64::from(u32::MAX), u64::MAX - 1, u64::MAX] {
        if let Some(entry) = pmtiles::find(&entries, id)
            && let Ok(header) = &header
        {
            let _ = header.tile_location(entry);
            let _ = header.leaf_location(entry);
        }
    }
}

/// Tile coordinates as the PMTiles reader numbers them (any zoom byte, column and
/// row): a tile that exists has an id in its zoom level's block.
pub fn pmtiles_tile_id(z: u8, x: u32, y: u32) {
    if let Some(id) = crate::pmtiles::tile_id(z, x, y) {
        // A level has 4^z tiles, after the (4^z - 1) / 3 tiles of the levels above.
        let level_tiles = 1u128 << (2 * u32::from(z));
        let first = (level_tiles - 1) / 3;
        assert!(
            (first..first + level_tiles).contains(&u128::from(id)),
            "a tile id outside its zoom level"
        );
    }
}

/// A Mapbox vector tile, through the generic decoder and every layer built on it
/// (borders and places, close-up place names, roads, transport, places of interest).
pub fn vector_tile(data: &[u8]) {
    let (key, bytes) = tile_key(data);
    let _ = crate::mvt::decode(bytes, |_| true);
    let _ = crate::poi::decode(key, bytes);
    let _ = crate::transport::decode(key, bytes);
    let _ = crate::roads::tiles::decode(key, bytes);
    let _ = crate::boundaries::place_names::decode(key, bytes);
    if let Some(kept) = crate::boundaries::osm_tiles::compact::Filtered::from_mvt(bytes) {
        // What is kept of a tile is stored and must read back as it was.
        let stored = kept.encode();
        assert!(
            crate::boundaries::osm_tiles::compact::Filtered::decode(&stored).as_ref()
                == Some(&kept),
            "a stored border tile does not read back"
        );
    }
}

/// A stored border tile (the compact format in the border cache).
pub fn compact_tile(bytes: &[u8]) {
    use crate::boundaries::osm_tiles::compact::Filtered;
    if let Some(tile) = Filtered::decode(bytes) {
        let stored = tile.encode();
        assert!(
            Filtered::decode(&stored).as_ref() == Some(&tile),
            "a stored border tile does not read back"
        );
    }
}

/// An image tile: imagery, elevation (Terrarium) and a user layer's tile.
pub fn image_tile(data: &[u8]) {
    let (pick, bytes) = selector(data);
    match pick % 3 {
        0 => {
            if let Ok(tile) = crate::tiles::Tile::decode(bytes) {
                let (width, height, pixels) = tile.rgba();
                assert!(pixels.len() == width * height * 4, "tile pixel count");
            }
        }
        1 => {
            if let Ok(tile) = crate::tiles::Tile::decode_elevation(bytes)
                && let Some(elevation) = tile.elevation()
            {
                assert!(elevation.max_m().is_finite(), "elevation maximum");
            }
        }
        _ => {
            let _ = crate::user_data::raster::decode(bytes);
        }
    }
}

/// A saved planet (`.planet` JSON with embedded images) and the star map.
pub fn planet(data: &[u8]) {
    let (pick, bytes) = selector(data);
    if pick & 1 == 0 {
        // A version 1 file names its texture; the fuzzer supplies that file's bytes too.
        let texture = bytes.get(bytes.len() / 2..).unwrap_or_default().to_vec();
        let _ = crate::planet::decode_planet(bytes, move |_| Ok(texture));
    } else {
        let _ = crate::sky::SkyImage::decode(bytes);
    }
}

/// One JSON document through every feed and file reader that takes JSON: aircraft,
/// vessels, satellites, aurora, time zones, sea names, search answers, imagery dates,
/// the map-data manifest, border GeoJSON, saved layers and flight routes.
pub fn json_feeds(bytes: &[u8]) {
    let Ok(json) = crate::aircraft::route::json_body(bytes) else {
        return;
    };
    let now = 1_800_000_000.0;
    for aircraft in crate::aircraft::decode::parse_opensky(&json, now)
        .iter()
        .chain(&crate::aircraft::decode::parse_adsb_lol(&json, now))
    {
        assert!(
            finite_position(aircraft.latitude, aircraft.longitude),
            "aircraft out of range"
        );
    }
    let _ = crate::aircraft::route::parse_trace(&json);
    let _ = crate::aircraft::route::parse_route(&json, None);
    for vessel in crate::vessels::parse_world_snapshot(&json, now)
        .iter()
        .chain(&crate::vessels::parse_ais_geojson(&json, now))
    {
        assert!(
            finite_position(vessel.lat, vessel.lon),
            "vessel out of range"
        );
    }
    for group in crate::satellites::SatelliteGroup::ALL {
        let _ = crate::satellites::decode::parse_omm(&json, now, group);
    }
    let _ = crate::aurora::AuroraGrid::parse(&json);
    let _ = crate::timezones::TimeZones::parse(&json);
    let _ = crate::marine_names::MarineNames::parse(&json);
    let _ = crate::search::parse_geocode_response(json.clone());
    let _ = crate::search::parse_photon_response(json.clone());
    let _ = crate::wayback::parse_wayback_config(&json);
    let _ = crate::map_data::manifest_template(&json);
    let mut lines = Vec::new();
    crate::boundaries::geojson::collect_geojson_lines(&json, &mut lines);
    let _ = crate::user_data::layers_from_json(&json);
}

/// A settings file, an exported settings file and an exported source list.
pub fn settings(bytes: &[u8]) {
    if let Ok(settings) = crate::settings::Settings::import(bytes) {
        // What was read must be writable and read back to the same settings.
        let exported = settings.export().expect("settings serialize");
        let again = crate::settings::Settings::import(&exported).expect("exported settings import");
        assert!(
            again == settings,
            "settings change when exported and imported"
        );
    }
    let _ = crate::user_data::import_sources(bytes);
}

/// The published airport list (text) and the gzip-compressed downloads.
pub fn compressed(data: &[u8]) {
    let (pick, bytes) = selector(data);
    match pick % 5 {
        0 => {
            let _ = crate::map_data::gunzip_if_needed(bytes.to_vec());
        }
        1 => {
            let _ = crate::timezones::TimeZones::from_gzip(bytes);
        }
        2 => {
            let _ = crate::marine_names::MarineNames::from_gzip(bytes);
        }
        3 => {
            let _ = crate::airports::Airports::from_gzip(bytes);
        }
        _ => {
            let _ = crate::pmtiles::decompress(bytes.to_vec(), crate::pmtiles::Compression::Gzip);
        }
    }
}

/// The airport list's text.
pub fn airports(text: &str) {
    let _ = crate::airports::Airports::parse(text);
}

/// A colour as typed in the style editor or stored in a settings file.
pub fn colour(text: &str) {
    let _ = crate::settings::map_style::Rgba::parse_hex(text);
}

/// One row of an MBTiles file's `tiles` table, as any SQLite file may hold it.
#[derive(Clone, Debug)]
pub struct MbtilesRow {
    pub zoom: i64,
    pub column: i64,
    pub row: i64,
    pub data: Vec<u8>,
}

/// An MBTiles file with these rows, read as a user's map layer reads it: described
/// (Add Data's "Test"), then asked for a few tiles.
pub fn mbtiles_rows(rows: &[MbtilesRow], ask: (u8, u32, u32)) {
    use crate::user_data::archive;
    let db = rusqlite::Connection::open_in_memory().expect("in-memory SQLite");
    db.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (zoom_level, tile_column, tile_row, tile_data);",
    )
    .expect("MBTiles tables");
    for row in rows {
        db.execute(
            "INSERT INTO tiles VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![row.zoom, row.column, row.row, row.data],
        )
        .expect("MBTiles row");
    }
    if let Ok(info) = archive::mbtiles_info(&db) {
        let side = u64::from(TileKey::tiles_per_side(info.sample.z));
        assert!(
            info.min_zoom <= info.max_zoom
                && info.sample.z == info.min_zoom
                && u64::from(info.sample.x.max(info.sample.y)) < side,
            "the sample is a tile that exists"
        );
        let Ok(Some(bytes)) = archive::mbtiles_tile(&db, info.sample) else {
            panic!("the sample tile cannot be read");
        };
        let _ = crate::user_data::raster::decode(&bytes);
    }
    // Any key, also one no map has.
    let key = TileKey {
        z: ask.0,
        x: ask.1,
        y: ask.2,
    };
    let _ = archive::mbtiles_tile(&db, key);
}
