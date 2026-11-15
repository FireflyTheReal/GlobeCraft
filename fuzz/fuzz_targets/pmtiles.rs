#![no_main]
//! A PMTiles header and directory, and tile ids for any zoom byte.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    globecraft_data::fuzzing::pmtiles(bytes);
    if let [z, x0, x1, x2, x3, y0, y1, y2, y3, ..] = *bytes {
        globecraft_data::fuzzing::pmtiles_tile_id(
            z,
            u32::from_le_bytes([x0, x1, x2, x3]),
            u32::from_le_bytes([y0, y1, y2, y3]),
        );
    }
});
