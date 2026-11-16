#![no_main]
//! An imagery, elevation or user-layer tile image; the first byte picks which.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::image_tile(bytes));
