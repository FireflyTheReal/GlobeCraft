#![no_main]
//! A KMZ archive.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::kmz(bytes));
