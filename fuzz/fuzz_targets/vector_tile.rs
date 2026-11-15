#![no_main]
//! A Mapbox vector tile through every layer decoder; the first nine bytes pick the tile.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::vector_tile(bytes));
