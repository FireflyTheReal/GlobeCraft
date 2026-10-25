//! Natural Earth's 1:110m countries, land borders and populated places, compiled into
//! the binary: the offline base the border tiles refine.
use super::{StatusLog, log_status};
use std::fmt;

const BUNDLED_COUNTRIES: &str =
    include_str!("../../../../assets/geo/ne_110m_admin_0_countries.geojson");
const BUNDLED_BORDERS: &str =
    include_str!("../../../../assets/geo/ne_110m_admin_0_boundary_lines_land.geojson");
const BUNDLED_PLACES: &str =
    include_str!("../../../../assets/geo/ne_110m_populated_places.geojson");

pub(super) enum FeatureSource {
    Bundled(&'static str),
}

impl fmt::Display for FeatureSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundled(_) => f.write_str("bundled 1:110m"),
        }
    }
}

pub(super) async fn first_features<T, F>(
    log: &StatusLog,
    what: &str,
    sources: &[FeatureSource],
    consume: F,
) -> Option<T>
where
    T: Default + Send + 'static,
    F: Fn(&serde_json::Value, &mut T) + Clone + Send + 'static,
{
    let mut last = String::from("no source configured");
    for source in sources {
        let result = match source {
            FeatureSource::Bundled(text) => {
                let text = *text;
                let consume = consume.clone();
                crate::cpu::run(crate::cpu::Service::Features, move || {
                    crate::features::project(text.as_bytes(), consume)
                })
                .await
            }
        };
        match result {
            Ok(value) => return Some(value),
            Err(error) => last = format!("{source}: {error}"),
        }
    }
    log_status(log, format!("✖ {what}: {last}"));
    None
}

pub(super) fn country_sources() -> Vec<FeatureSource> {
    vec![FeatureSource::Bundled(BUNDLED_COUNTRIES)]
}

/// Land borders without coastlines: only shared land boundaries are outlined.
pub(super) fn border_sources() -> Vec<FeatureSource> {
    vec![FeatureSource::Bundled(BUNDLED_BORDERS)]
}

pub(super) fn place_sources() -> Vec<FeatureSource> {
    vec![FeatureSource::Bundled(BUNDLED_PLACES)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_collections_decode() {
        for text in [BUNDLED_COUNTRIES, BUNDLED_BORDERS, BUNDLED_PLACES] {
            let value: serde_json::Value = serde_json::from_str(text).unwrap();
            assert!(value["features"].as_array().is_some_and(|f| !f.is_empty()));
        }
    }
}
