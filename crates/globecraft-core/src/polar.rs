//! Polar stereographic projections on WGS 84, as NASA GIBS serves its polar tiles:
//! EPSG:3413 for the Arctic (true scale at 70° N, 45° W pointing down) and EPSG:3031
//! for the Antarctic (true scale at 71° S, 0° pointing up). Snyder, *Map
//! Projections: A Working Manual*, §21 (polar stereographic with a standard
//! parallel); the tests compare with PROJ.

/// WGS 84 semi-major axis, metres.
const SEMI_MAJOR_AXIS_M: f64 = 6_378_137.0;
/// WGS 84 first eccentricity.
const ECCENTRICITY: f64 = 0.081_819_190_842_621_49;

/// A pole and its polar stereographic projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pole {
    /// EPSG:3413: `+lat_0=90 +lat_ts=70 +lon_0=-45`.
    North,
    /// EPSG:3031: `+lat_0=-90 +lat_ts=-71 +lon_0=0`.
    South,
}

impl Pole {
    /// The pole on the side of `lat_deg`.
    pub fn of(lat_deg: f64) -> Self {
        if lat_deg >= 0.0 {
            Self::North
        } else {
            Self::South
        }
    }

    /// The latitude of true scale, degrees away from the equator (positive).
    fn standard_parallel_deg(self) -> f64 {
        match self {
            Self::North => 70.0,
            Self::South => 71.0,
        }
    }

    /// The meridian running straight down (north) or up (south) from the pole.
    fn central_meridian_deg(self) -> f64 {
        match self {
            Self::North => -45.0,
            Self::South => 0.0,
        }
    }

    /// Easting and northing (metres) of a point on this pole's side.
    pub fn project(self, lat_deg: f64, lon_deg: f64) -> (f64, f64) {
        // Snyder's north-polar formulas; the south pole is the mirror image
        // (latitude and longitude negated in, both axes negated out).
        let (lat, lon, central) = match self {
            Self::North => (lat_deg, lon_deg, self.central_meridian_deg()),
            Self::South => (-lat_deg, -lon_deg, -self.central_meridian_deg()),
        };
        let t = |phi_deg: f64| {
            let phi = phi_deg.to_radians();
            let e_sin = ECCENTRICITY * phi.sin();
            (std::f64::consts::FRAC_PI_4 - phi / 2.0).tan()
                / ((1.0 - e_sin) / (1.0 + e_sin)).powf(ECCENTRICITY / 2.0)
        };
        let standard = self.standard_parallel_deg().to_radians();
        let m_c = standard.cos() / (1.0 - (ECCENTRICITY * standard.sin()).powi(2)).sqrt();
        let rho = SEMI_MAJOR_AXIS_M * m_c * t(lat) / t(self.standard_parallel_deg());
        let angle = (lon - central).to_radians();
        let (x, y) = (rho * angle.sin(), -rho * angle.cos());
        match self {
            Self::North => (x, y),
            Self::South => (-x, -y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points and their projections by PROJ (pyproj 3, `always_xy`), to 1 cm.
    #[test]
    fn projections_match_proj() {
        let cases = [
            (Pole::North, 90.0, 0.0, (0.0, 0.0)),
            (Pole::North, 80.0, -45.0, (0.0, -1085920.2974)),
            (Pole::North, 85.0, 45.0, (541966.7006, 0.0)),
            (Pole::North, 70.0, 10.0, (1792245.4067, -1254943.7441)),
            (Pole::North, 88.5, -170.0, (-133112.1150, 93206.1064)),
            (Pole::North, 60.0, 100.0, (1906086.4255, 2722173.5292)),
            (Pole::South, -90.0, 0.0, (0.0, 0.0)),
            (Pole::South, -80.0, 0.0, (0.0, 1089179.4556)),
            (Pole::South, -85.0, 90.0, (543593.2981, 0.0)),
            (Pole::South, -65.0, -60.0, (-2388932.7669, 1379250.9760)),
            (Pole::South, -71.0, 170.0, (361667.4974, -2051118.3026)),
            (Pole::South, -88.0, -135.0, (-153672.7307, -153672.7307)),
        ];
        for (pole, lat, lon, (x, y)) in cases {
            let (px, py) = pole.project(lat, lon);
            assert!(
                (px - x).abs() < 0.01 && (py - y).abs() < 0.01,
                "{pole:?} {lat},{lon}: ({px:.4}, {py:.4}) instead of ({x}, {y})"
            );
        }
        assert_eq!(Pole::of(85.1), Pole::North);
        assert_eq!(Pole::of(-85.1), Pole::South);
    }
}
