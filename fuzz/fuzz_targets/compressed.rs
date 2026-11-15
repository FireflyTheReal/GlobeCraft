#![no_main]
//! Gzip-compressed downloads; the first byte picks the reader.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::compressed(bytes));
