#![no_main]
//! One JSON document through every feed reader.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::json_feeds(bytes));
