#![no_main]
//! A KML document's text.
libfuzzer_sys::fuzz_target!(|text: &str| globecraft_data::fuzzing::kml(text));
