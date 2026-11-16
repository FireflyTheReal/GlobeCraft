#![no_main]
//! A GeoJSON, GPX or CSV feature file; the first byte picks the format.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::features(bytes));
