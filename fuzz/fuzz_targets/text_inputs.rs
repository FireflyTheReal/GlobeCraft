#![no_main]
//! Text a person types or pastes, or that arrives in a link or a feed: view links,
//! coordinates, dates, web addresses, colours and the airport table.
libfuzzer_sys::fuzz_target!(|text: &str| {
    if let Ok(link) = globecraft_core::view_link::ViewLink::parse(text) {
        let _ = link.web_addresses();
    }
    let _ = globecraft_core::coordinates::parse_coordinate_query(text);
    if let Some((latitude, longitude)) = text.split_once(',') {
        let _ = globecraft_core::coordinates::parse_coordinates(latitude, longitude);
    }
    let _ = globecraft_core::time::parse_iso(text);
    if let Some(host) = globecraft_core::web_address::host(text) {
        let _ = globecraft_core::web_address::is_local_host(&host);
    }
    globecraft_data::fuzzing::colour(text);
    globecraft_data::fuzzing::airports(text);
});
