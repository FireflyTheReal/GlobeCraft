#![no_main]
//! A saved planet file or the star map; the first byte picks which.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::planet(bytes));
