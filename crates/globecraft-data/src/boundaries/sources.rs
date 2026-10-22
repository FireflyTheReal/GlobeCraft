//! Where a world overlay's feature collection comes from.
//!
//! Low detail ships with the binary so a cold start draws the world without a
//! network request. Higher detail is fetched from upstream and replaces it.
use super::lod::DetailLevel;
use super::{StatusLog, log_status};
use std::fmt;

const NE: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson";

/// Natural Earth 1:110m, public domain. Bundled from `assets/geo`.
const BUNDLED_COUNTRIES: &str =
    include_str!("../../../../assets/geo/ne_110m_admin_0_countries.geojson");
const BUNDLED_BORDERS: &str =
    include_str!("../../../../assets/geo/ne_110m_admin_0_boundary_lines_land.geojson");
const BUNDLED_PLACES: &str =
    include_str!("../../../../assets/geo/ne_110m_populated_places.geojson");

pub(super) enum FeatureSource {
    Bundled(&'static str),
    Url(String),
}

impl fmt::Display for FeatureSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundled(_) => f.write_str("bundled 1:110m"),
            Self::Url(url) => f.write_str(url),
        }
    }
}

/// Try each source in order and keep the first collection that decodes.
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
            FeatureSource::Url(url) => crate::features::fetch(url, consume.clone()).await,
        };
        match result {
            Ok(value) => return Some(value),
            Err(error) => last = format!("{source}: {error}"),
        }
    }
    log_status(log, format!("✖ {what}: {last}"));
    None
}

pub(super) fn natural_earth(name: &str) -> FeatureSource {
    FeatureSource::Url(format!("{NE}/{name}.geojson"))
}

pub(super) fn country_sources(level: DetailLevel) -> Vec<FeatureSource> {
    match level {
        DetailLevel::Low => vec![FeatureSource::Bundled(BUNDLED_COUNTRIES)],
        DetailLevel::Medium => vec![
            natural_earth("ne_50m_admin_0_countries"),
            FeatureSource::Bundled(BUNDLED_COUNTRIES),
        ],
        DetailLevel::High => vec![
            natural_earth("ne_10m_admin_0_countries"),
            natural_earth("ne_50m_admin_0_countries"),
            FeatureSource::Bundled(BUNDLED_COUNTRIES),
        ],
    }
}

/// Country land borders without coastlines, matching the clean look of a globe
/// where only shared land boundaries are outlined. The polygon rings stay in
/// `regions` for country lookup but are no longer drawn.
pub(super) fn border_sources(level: DetailLevel) -> Vec<FeatureSource> {
    match level {
        DetailLevel::Low => vec![FeatureSource::Bundled(BUNDLED_BORDERS)],
        DetailLevel::Medium => vec![
            natural_earth("ne_50m_admin_0_boundary_lines_land"),
            FeatureSource::Bundled(BUNDLED_BORDERS),
        ],
        DetailLevel::High => vec![
            natural_earth("ne_10m_admin_0_boundary_lines_land"),
            natural_earth("ne_50m_admin_0_boundary_lines_land"),
            FeatureSource::Bundled(BUNDLED_BORDERS),
        ],
    }
}

pub(super) fn place_sources(level: DetailLevel) -> Vec<FeatureSource> {
    match level {
        DetailLevel::Low => vec![FeatureSource::Bundled(BUNDLED_PLACES)],
        DetailLevel::Medium => vec![
            natural_earth("ne_50m_populated_places"),
            FeatureSource::Bundled(BUNDLED_PLACES),
        ],
        DetailLevel::High => vec![natural_earth("ne_10m_populated_places")],
    }
}

/// Disputed linework, drawn on top of the country outlines.
pub(super) fn map_unit_sources(level: DetailLevel) -> Vec<FeatureSource> {
    match level {
        DetailLevel::Low => Vec::new(),
        DetailLevel::Medium => vec![natural_earth("ne_50m_admin_0_map_units")],
        DetailLevel::High => vec![
            natural_earth("ne_10m_admin_0_map_units"),
            natural_earth("ne_50m_admin_0_map_units"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_detail_is_bundled_and_needs_no_network() {
        assert!(matches!(
            country_sources(DetailLevel::Low).as_slice(),
            [FeatureSource::Bundled(_)]
        ));
        assert!(matches!(
            place_sources(DetailLevel::Low).as_slice(),
            [FeatureSource::Bundled(_)]
        ));
        assert!(matches!(
            border_sources(DetailLevel::Low).as_slice(),
            [FeatureSource::Bundled(_)]
        ));
        assert!(map_unit_sources(DetailLevel::Low).is_empty());
    }

    #[test]
    fn higher_detail_prefers_finer_upstream_with_coarse_fallbacks() {
        let high = country_sources(DetailLevel::High);
        assert!(
            matches!(&high[0], FeatureSource::Url(url) if url.ends_with("ne_10m_admin_0_countries.geojson"))
        );
        assert!(
            matches!(&high[1], FeatureSource::Url(url) if url.ends_with("ne_50m_admin_0_countries.geojson"))
        );
        assert!(matches!(high.last(), Some(FeatureSource::Bundled(_))));
    }

    #[test]
    fn bundled_collections_decode() {
        #[derive(Default)]
        struct Count(usize);
        let count = crate::features::project(BUNDLED_COUNTRIES.as_bytes(), |_, out: &mut Count| {
            out.0 += 1
        })
        .unwrap();
        assert_eq!(count.0, 177);

        let borders =
            crate::features::project(BUNDLED_BORDERS.as_bytes(), |_, out: &mut Count| out.0 += 1)
                .unwrap();
        assert_eq!(borders.0, 331);

        let places =
            crate::features::project(BUNDLED_PLACES.as_bytes(), |_, out: &mut Count| out.0 += 1)
                .unwrap();
        assert_eq!(places.0, 243);
    }
}
