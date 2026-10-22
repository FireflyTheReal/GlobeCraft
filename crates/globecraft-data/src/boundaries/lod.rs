//! Detail level for the world overlays, chosen from camera altitude.
//!
//! Coarse detail loads first and is replaced when the camera moves closer.
//! Altitudes are the camera's distance above the surface in kilometres.

/// Whole-globe views keep the bundled low detail.
pub(super) const LOW_DETAIL_MAX_ALTITUDE_KM: f64 = 10_000.0;
/// Continental views use medium detail below this altitude.
pub(super) const MEDIUM_DETAIL_MAX_ALTITUDE_KM: f64 = 1_500.0;
/// Falling back to a coarser level only after the camera has clearly left the
/// finer level's band (this multiple of its boundary altitude). Without the
/// margin, altitude jitter around a threshold refetches and re-applies whole
/// world layers repeatedly.
const DOWNGRADE_MARGIN: f64 = 1.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DetailLevel {
    Low = 1,
    Medium = 2,
    High = 3,
}

impl DetailLevel {
    pub(super) fn for_altitude(altitude_km: f64) -> Self {
        if !altitude_km.is_finite() || altitude_km > LOW_DETAIL_MAX_ALTITUDE_KM {
            Self::Low
        } else if altitude_km > MEDIUM_DETAIL_MAX_ALTITUDE_KM {
            Self::Medium
        } else {
            Self::High
        }
    }

    /// The level to render for the camera altitude given the level currently
    /// applied: rising toward the surface upgrades immediately, while falling
    /// back waits until the camera clears the finer level's band by
    /// `DOWNGRADE_MARGIN`.
    pub(super) fn with_downgrade_hysteresis(altitude_km: f64, current: Self) -> Self {
        let wanted = Self::for_altitude(altitude_km);
        if wanted >= current {
            return wanted;
        }
        let boundary = match current {
            Self::High => MEDIUM_DETAIL_MAX_ALTITUDE_KM,
            Self::Medium => LOW_DETAIL_MAX_ALTITUDE_KM,
            Self::Low => return Self::Low,
        };
        if altitude_km > boundary * DOWNGRADE_MARGIN {
            wanted
        } else {
            current
        }
    }

    pub(super) fn from_code(code: u8) -> Self {
        match code {
            3 => Self::High,
            2 => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn altitude_selects_a_monotonic_detail_level() {
        assert_eq!(DetailLevel::for_altitude(20_000.0), DetailLevel::Low);
        assert_eq!(DetailLevel::for_altitude(10_001.0), DetailLevel::Low);
        assert_eq!(DetailLevel::for_altitude(10_000.0), DetailLevel::Medium);
        assert_eq!(DetailLevel::for_altitude(1_501.0), DetailLevel::Medium);
        assert_eq!(DetailLevel::for_altitude(1_500.0), DetailLevel::High);
        assert_eq!(DetailLevel::for_altitude(5.0), DetailLevel::High);
        assert_eq!(DetailLevel::for_altitude(f64::NAN), DetailLevel::Low);
        assert!(DetailLevel::Low < DetailLevel::Medium);
        assert!(DetailLevel::Medium < DetailLevel::High);
    }

    #[test]
    fn codes_round_trip() {
        for level in [DetailLevel::Low, DetailLevel::Medium, DetailLevel::High] {
            assert_eq!(DetailLevel::from_code(level as u8), level);
        }
        assert_eq!(DetailLevel::from_code(0), DetailLevel::Low);
        assert_eq!(DetailLevel::from_code(9), DetailLevel::Low);
    }

    #[test]
    fn upgrades_engage_immediately_while_downgrades_lag_behind_the_boundary() {
        use DetailLevel::{High, Low, Medium};
        // Approaching the surface upgrades right away at the same thresholds.
        assert_eq!(DetailLevel::with_downgrade_hysteresis(1_000.0, Low), High);
        assert_eq!(DetailLevel::with_downgrade_hysteresis(2_000.0, Low), Medium);
        // Leaving the high band: still inside its 1.15× margin keeps High.
        assert_eq!(DetailLevel::with_downgrade_hysteresis(1_600.0, High), High);
        assert_eq!(
            DetailLevel::with_downgrade_hysteresis(1_800.0, High),
            Medium
        );
        // Channel straddling changes nothing when the wanted level equals current.
        assert_eq!(
            DetailLevel::with_downgrade_hysteresis(11_000.0, Medium),
            Medium
        );
        // Clearing the medium band by the margin drops to Low.
        assert_eq!(
            DetailLevel::with_downgrade_hysteresis(11_800.0, Medium),
            Low
        );
        // Deep overview falls back in one step straight to the wanted level.
        assert_eq!(DetailLevel::with_downgrade_hysteresis(20_000.0, High), Low);
        // A low camera can never regress.
        assert_eq!(DetailLevel::with_downgrade_hysteresis(100.0, High), High);
    }
}
