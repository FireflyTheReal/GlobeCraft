//! Territorial claims in Antarctica, as frozen by the Antarctic Treaty (1959): each
//! a sector between two meridians, from 60° S (the treaty's limit) to the South
//! Pole, the British, Argentine and Chilean sectors overlapping. Marie Byrd Land is
//! claimed by no one. Norway's Queen Maud Land has no defined southern limit; its
//! sides are shared with the British and Australian sectors, which reach the pole.

/// The treaty's northern limit, where the claim lines start.
pub const NORTH_LIMIT_DEG: f64 = -60.0;

/// One claim (or the unclaimed sector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claim {
    /// The translation key of its name and claimant.
    pub name_key: &'static str,
    /// Its sides, west then east, degrees (east may be past 180°, then negative).
    pub west_deg: f64,
    pub east_deg: f64,
    /// Where its name goes, so the overlapping claims' names do not cover each other.
    pub label_lat_deg: f64,
}

impl Claim {
    /// The meridian halfway between its sides.
    pub fn middle_lon_deg(&self) -> f64 {
        let east = if self.east_deg < self.west_deg {
            self.east_deg + 360.0
        } else {
            self.east_deg
        };
        let middle = (self.west_deg + east) / 2.0;
        if middle > 180.0 {
            middle - 360.0
        } else {
            middle
        }
    }
}

/// 44°38′ E, 136°11′ E and 142°02′ E, as the claims define them.
const QUEEN_MAUD_EAST_DEG: f64 = 44.0 + 38.0 / 60.0;
const ADELIE_WEST_DEG: f64 = 136.0 + 11.0 / 60.0;
const ADELIE_EAST_DEG: f64 = 142.0 + 2.0 / 60.0;

pub const CLAIMS: [Claim; 9] = [
    Claim {
        name_key: "antarctic-claim-united-kingdom",
        west_deg: -80.0,
        east_deg: -20.0,
        label_lat_deg: -71.0,
    },
    Claim {
        name_key: "antarctic-claim-argentina",
        west_deg: -74.0,
        east_deg: -25.0,
        label_lat_deg: -77.0,
    },
    Claim {
        name_key: "antarctic-claim-chile",
        west_deg: -90.0,
        east_deg: -53.0,
        label_lat_deg: -83.0,
    },
    Claim {
        name_key: "antarctic-claim-norway",
        west_deg: -20.0,
        east_deg: QUEEN_MAUD_EAST_DEG,
        label_lat_deg: -76.0,
    },
    Claim {
        name_key: "antarctic-claim-australia",
        west_deg: QUEEN_MAUD_EAST_DEG,
        east_deg: ADELIE_WEST_DEG,
        label_lat_deg: -76.0,
    },
    Claim {
        name_key: "antarctic-claim-france",
        west_deg: ADELIE_WEST_DEG,
        east_deg: ADELIE_EAST_DEG,
        label_lat_deg: -71.0,
    },
    Claim {
        name_key: "antarctic-claim-australia",
        west_deg: ADELIE_EAST_DEG,
        east_deg: 160.0,
        label_lat_deg: -78.0,
    },
    Claim {
        name_key: "antarctic-claim-new-zealand",
        west_deg: 160.0,
        east_deg: -150.0,
        label_lat_deg: -79.0,
    },
    Claim {
        name_key: "antarctic-claim-unclaimed",
        west_deg: -150.0,
        east_deg: -90.0,
        label_lat_deg: -79.0,
    },
];

/// Every meridian a claim ends at, once each.
pub fn boundaries() -> Vec<f64> {
    let mut sides: Vec<f64> = CLAIMS
        .iter()
        .flat_map(|claim| [claim.west_deg, claim.east_deg])
        .collect();
    sides.sort_by(f64::total_cmp);
    sides.dedup();
    sides
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_claims_have_their_published_sides() {
        let sides = boundaries();
        assert_eq!(sides.len(), 11, "{sides:?}");
        for side in [-150.0, -90.0, -80.0, -74.0, -53.0, -25.0, -20.0, 160.0] {
            assert!(sides.contains(&side), "{side}");
        }
        // New Zealand's Ross Dependency crosses 180°: its middle is at 175° W.
        let ross = CLAIMS
            .iter()
            .find(|claim| claim.name_key.ends_with("new-zealand"))
            .unwrap();
        assert!((ross.middle_lon_deg() + 175.0).abs() < 1e-9);
        // Adélie Land lies between the two Australian sectors.
        let france = CLAIMS
            .iter()
            .find(|c| c.name_key.ends_with("france"))
            .unwrap();
        assert!((france.middle_lon_deg() - 139.1).abs() < 0.01);
    }
}
