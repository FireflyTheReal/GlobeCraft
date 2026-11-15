//! Map services and persistence. Background loaders report values to the desktop app.
#![forbid(unsafe_code)]
pub mod aircraft;
pub mod airports;
pub mod antarctic_claims;
pub mod aurora;
pub mod boundaries;
mod bounded;
mod cpu;
pub mod download_stats;
mod error;
mod features;
pub mod files;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
mod http;
mod json_fields;
pub mod keys;
pub mod kml;
pub mod links;
pub mod live;
pub mod live_maps;
mod map_data;
pub mod marine_names;
pub mod model_http;
mod mvt;
pub mod network;
mod paths;
pub mod planet;
mod pmtiles;
pub mod poi;
#[cfg(feature = "profiling")]
pub mod profiling;
pub mod roads;
pub mod run_log;
pub mod runtime;
pub mod satellites;
pub mod search;
pub mod secret_store;
pub mod settings;
pub mod sky;
#[cfg(test)]
mod test_inputs;
pub mod tiles;
pub mod timezones;
pub mod traffic;
pub mod transport;
pub mod updater;
pub mod user_data;
pub mod vector_tiles;
pub mod vessels;
pub mod view_query;
pub mod wayback;
pub use error::Error;
pub use paths::{
    gpu_start_marker, migrate_cache, migrate_config, migrate_legacy_data, remove_retired_caches,
};
pub use view_query::ViewQuery;
