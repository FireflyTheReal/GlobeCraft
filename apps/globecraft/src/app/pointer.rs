//! The pointer on the globe (roadmap 10.2: one of the feature states split out of `App`).
use super::{here, long_press};

/// Where the pointer is on the globe and what it asked about there.
#[derive(Default)]
pub(super) struct PointerState {
    /// The latitude and longitude under the pointer.
    pub(super) cursor: Option<(f64, f64)>,
    /// The point a context menu was opened at.
    pub(super) menu_pick: Option<(f64, f64)>,
    /// Press and hold on the globe opens the card (touch screens and trackpads).
    pub(super) long_press: long_press::LongPress,
    /// The "What's here?" card, while open.
    pub(super) here_card: Option<here::HereCard>,
    /// The card's state and municipality, with the border tile revision they were
    /// looked up at (the lookup reads lines, so it is not repeated every frame).
    pub(super) here_admin: Option<(here::HereCard, u64, globecraft_data::boundaries::AdminAreas)>,
}
