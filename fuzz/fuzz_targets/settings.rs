#![no_main]
//! A settings file and an exported source list.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::settings(bytes));
