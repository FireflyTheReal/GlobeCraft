#![no_main]
//! A WMS GetCapabilities answer.
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| globecraft_data::fuzzing::wms_capabilities(bytes));
