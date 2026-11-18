//! The switch for the live feeds: aircraft, vessels, satellites, the aurora forecast,
//! traffic, clouds and radar. Their data comes from tracking, weather and traffic
//! services and changes by the minute.
//!
//! A build without the `live` feature keeps the feeds' types and decoders (the
//! renderer and the interface are written against them) but never starts a download:
//! every place that would start one asks [`ENABLED`] first.

/// Whether this build has the live feeds.
pub const ENABLED: bool = cfg!(feature = "live");

#[cfg(all(test, not(feature = "live")))]
mod tests {
    use crate::runtime::ServiceRuntime;

    /// Every feed, asked to load in a build without them, starts nothing: no status
    /// turns to loading or to an error, and nothing arrives.
    #[test]
    fn a_build_without_live_feeds_starts_no_download() {
        let runtime = ServiceRuntime::new().unwrap();
        let lang = globecraft_core::language::Lang::En;
        let mut vessels = crate::vessels::LiveVessels::default();
        let mut satellites = crate::satellites::LiveSatellites::default();
        let mut aurora = crate::aurora::LiveAurora::default();
        let mut maps = crate::live_maps::LiveMaps::default();
        let before = (
            vessels.status(lang),
            satellites.status(lang),
            aurora.status(lang),
            maps.generation(),
        );
        for _ in 0..3 {
            vessels.update(&runtime, true, || {});
            satellites.update(&runtime, true, || {});
            aurora.update(&runtime, true, || {});
            maps.update(&runtime, true, true, true, || {});
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(vessels.snapshot().is_empty() && satellites.snapshot().is_empty());
        assert!(
            aurora.grid().is_none()
                && maps.cloud().is_none()
                && maps.radar().is_none()
                && maps.water().is_none()
        );
        let after = (
            vessels.status(lang),
            satellites.status(lang),
            aurora.status(lang),
            maps.generation(),
        );
        assert_eq!(before, after);
    }
}
