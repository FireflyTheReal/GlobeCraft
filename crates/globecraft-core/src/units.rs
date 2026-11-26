//! Display units only. Geographic calculations and persisted coordinates stay metric.
use crate::{
    METERS_PER_KM,
    math::{fmt_alt, fmt_dist},
};
mod altitude;
pub use altitude::AltitudeUnit;
mod ruler;
pub use ruler::RulerUnit;

/// International mile in kilometres; shared with altitude inputs in the UI.
pub const KM_PER_MILE: f64 = 1.609_344;
const FEET_PER_MILE: f64 = 5280.0;
const METERS_PER_FOOT: f64 = 0.3048;
/// A knot (a nautical mile an hour), km/h: aircraft and ships report in knots.
pub const KMH_PER_KNOT: f64 = 1.852;
/// A foot a minute, metres a second: aircraft report climbs in feet a minute.
pub const MS_PER_FOOT_PER_MINUTE: f64 = METERS_PER_FOOT / 60.0;
// GlobeCraft's regional display policy includes mixed/traditional mile usage.
// This deliberately extends CLDR's general length defaults (US/GB); it does not
// describe statutory measurement systems. Users can override it in Settings.
const MILE_DEFAULT_REGIONS: &[&str] = &[
    "US", "GB", "LR", "MM", "BZ", "BS", "VI", "GU", "PR", "KY", "GG", "JE", "IM",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceUnits {
    Metric,
    Imperial,
}
impl DistanceUnits {
    /// Resolve the app's regional display default; language alone has no region.
    /// See the regional unit policy in docs/ARCHITECTURE.md.
    pub fn for_locale(locale: &str) -> Self {
        if locale_region(locale)
            .is_some_and(|region| MILE_DEFAULT_REGIONS.contains(&region.as_str()))
        {
            Self::Imperial
        } else {
            Self::Metric
        }
    }
    pub fn distance(self, km: f64) -> String {
        match self {
            Self::Metric => fmt_dist(km),
            Self::Imperial => {
                let miles = km / KM_PER_MILE;
                if miles < 1.0 {
                    format!("{:.0} ft", miles * FEET_PER_MILE)
                } else {
                    format!("{miles:.2} mi")
                }
            }
        }
    }
    pub fn altitude(self, km: f64) -> String {
        match self {
            Self::Metric => fmt_alt(km),
            Self::Imperial => {
                let miles = km / KM_PER_MILE;
                if miles < 1.0 {
                    format!("{:.0} ft", miles * FEET_PER_MILE)
                } else {
                    format!("{miles:.0} mi")
                }
            }
        }
    }
    /// An area: square metres, hectares or square kilometres; acres or square miles.
    pub fn area(self, km2: f64) -> String {
        const KM2_PER_MI2: f64 = KM_PER_MILE * KM_PER_MILE;
        const ACRES_PER_MI2: f64 = 640.0;
        match self {
            Self::Metric if km2 < 0.01 => format!("{:.0} m²", km2 * 1e6),
            Self::Metric if km2 < 1.0 => format!("{:.2} ha", km2 * 100.0),
            Self::Metric => format!("{} km²", grouped(km2)),
            Self::Imperial => {
                let mi2 = km2 / KM2_PER_MI2;
                if mi2 < 1.0 {
                    format!("{:.2} acres", mi2 * ACRES_PER_MI2)
                } else {
                    format!("{} mi²", grouped(mi2))
                }
            }
        }
    }
    /// A ground height: whole metres or feet.
    pub fn elevation(self, metres: f64) -> String {
        match self {
            Self::Metric => format!("{metres:.0} m"),
            Self::Imperial => format!("{:.0} ft", metres / METERS_PER_FOOT),
        }
    }
    /// A length in metres to `decimals` places: metres, or feet (a ship's size).
    pub fn length(self, metres: f64, decimals: usize) -> String {
        match self {
            Self::Metric => format!("{metres:.decimals$} m"),
            Self::Imperial => format!("{:.decimals$} ft", metres / METERS_PER_FOOT),
        }
    }
    /// A speed in km/h: km/h, or mph.
    pub fn speed(self, kmh: f64) -> String {
        match self {
            Self::Metric => format!("{kmh:.0} km/h"),
            Self::Imperial => format!("{:.0} mph", kmh / KM_PER_MILE),
        }
    }
    /// A climb (positive) or descent in metres a second: m/s, or ft/min as pilots
    /// read it.
    pub fn vertical_rate(self, metres_per_second: f64) -> String {
        match self {
            Self::Metric => format!("{metres_per_second:+.1} m/s"),
            Self::Imperial => format!("{:+.0} ft/min", metres_per_second / MS_PER_FOOT_PER_MINUTE),
        }
    }
    /// A temperature in °C: °C, or °F.
    pub fn temperature(self, celsius: f64) -> String {
        match self {
            Self::Metric => format!("{celsius:.0} °C"),
            Self::Imperial => format!("{:.0} °F", celsius * 9.0 / 5.0 + 32.0),
        }
    }
    /// Choose feet for short scale bars and miles for larger ones.
    pub fn scale_unit_meters(self, target_m: f64) -> f64 {
        match self {
            Self::Metric => {
                if target_m >= METERS_PER_KM {
                    METERS_PER_KM
                } else {
                    1.0
                }
            }
            Self::Imperial => {
                if target_m >= KM_PER_MILE * METERS_PER_KM {
                    KM_PER_MILE * METERS_PER_KM
                } else {
                    KM_PER_MILE * METERS_PER_KM / FEET_PER_MILE
                }
            }
        }
    }
    pub fn scale_label(self, distance_m: f64) -> String {
        let divisor = self.scale_unit_meters(distance_m * (1.0 + f64::EPSILON));
        let unit = match self {
            Self::Metric => {
                if divisor == 1.0 {
                    "m"
                } else {
                    "km"
                }
            }
            Self::Imperial => {
                if divisor < 1.0 {
                    "ft"
                } else {
                    "mi"
                }
            }
        };
        format!("{} {unit}", (distance_m / divisor * 100.0).round() / 100.0)
    }
}
/// Select US-specific ruler naming independently of the UI language.
/// Two decimals below 100, whole numbers with thousands separated above.
fn grouped(value: f64) -> String {
    if value < 100.0 {
        return format!("{value:.2}");
    }
    let digits = format!("{value:.0}");
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn is_us_locale(locale: &str) -> bool {
    locale_region(locale).is_some_and(|region| region.as_str() == "US")
}
fn locale_region(locale: &str) -> Option<unic_langid::subtags::Region> {
    locale
        .split(['.', '@'])
        .next()
        .unwrap_or(locale)
        .replace('_', "-")
        .parse::<unic_langid::LanguageIdentifier>()
        .ok()
        .and_then(|id| id.region)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reported_values_follow_the_units() {
        let (m, i) = (DistanceUnits::Metric, DistanceUnits::Imperial);
        assert_eq!(m.length(10_668.0, 0), "10668 m");
        assert_eq!(i.length(10_668.0, 0), "35000 ft");
        assert_eq!(m.speed(450.0 * KMH_PER_KNOT), "833 km/h");
        assert_eq!(i.speed(450.0 * KMH_PER_KNOT), "518 mph");
        assert_eq!(
            i.vertical_rate(1_000.0 * MS_PER_FOOT_PER_MINUTE),
            "+1000 ft/min"
        );
        assert_eq!(
            m.vertical_rate(-1_000.0 * MS_PER_FOOT_PER_MINUTE),
            "-5.1 m/s"
        );
        assert_eq!(m.temperature(-40.0), "-40 °C");
        assert_eq!(i.temperature(-40.0), "-40 °F");
        assert_eq!(i.temperature(20.0), "68 °F");
    }
    #[test]
    fn areas_read_in_fitting_units() {
        let metric = DistanceUnits::Metric;
        assert_eq!(metric.area(0.0045), "4500 m²");
        assert_eq!(metric.area(0.25), "25.00 ha");
        assert_eq!(metric.area(12.345), "12.35 km²");
        assert_eq!(metric.area(12_308.8), "12,309 km²");
        let imperial = DistanceUnits::Imperial;
        assert_eq!(imperial.area(0.01), "2.47 acres");
        assert_eq!(imperial.area(12_308.8), "4,752 mi²");
    }
    #[test]
    fn region_not_ui_language_selects_default() {
        for locale in [
            "en-US",
            "es_US.UTF-8",
            "en_Latn_US",
            "en-GB",
            "cy-GB",
            "en-LR",
            "my-MM",
            "en-BZ",
            "en-BS",
            "en-VI",
            "en-GU",
            "es-PR",
            "en-KY",
            "en-GG",
            "en-JE",
            "en-IM",
        ] {
            assert_eq!(DistanceUnits::for_locale(locale), DistanceUnits::Imperial);
        }
        for locale in ["en-CA", "en-AU", "pt-BR", "fr-FR", "en", "", "C", "invalid"] {
            assert_eq!(DistanceUnits::for_locale(locale), DistanceUnits::Metric);
        }
    }
    #[test]
    fn display_conversion_preserves_geographic_values() {
        assert_eq!(DistanceUnits::Imperial.distance(KM_PER_MILE), "1.00 mi");
        assert_eq!(
            DistanceUnits::Imperial.distance(KM_PER_MILE / 2.0),
            "2640 ft"
        );
        assert_eq!(DistanceUnits::Metric.distance(1.5), "1.50 km");
        // A distance that would round to 1000 m shows in kilometres.
        assert_eq!(DistanceUnits::Metric.distance(0.9996), "1.00 km");
        assert_eq!(DistanceUnits::Metric.distance(0.9994), "999 m");
        assert_eq!(DistanceUnits::Metric.altitude(14000.0), "14000 km");
        assert_eq!(
            DistanceUnits::Imperial.scale_label(KM_PER_MILE * METERS_PER_KM),
            "1 mi"
        );
    }
}
