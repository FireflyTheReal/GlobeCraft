//! Symbols for places of interest, peaks, parks and lakes, as map apps draw them
//! (roadmap 3.8): a round badge coloured by group (food, shopping, culture, health,
//! transport, nature…) with a small white symbol of the kind, chosen from
//! OpenMapTiles' `subclass` and `class`. Drawn as vector shapes so they stay sharp
//! at every scale.
use eframe::egui::{Color32, Pos2, Shape, Stroke, epaint::CircleShape};
mod classify;
mod symbols;
pub(super) use classify::{is_open_water, point_badge};
use symbols::{Pen, Symbol};

/// The badge's radius, map points (times the map text scale).
pub(super) const BADGE_RADIUS_POINTS: f32 = 6.5;
/// The symbol's half size as a share of the badge radius.
const SYMBOL_SHARE: f32 = 0.58;
/// The symbol's line width, map points.
const SYMBOL_STROKE_POINTS: f32 = 1.15;
/// The badge's white rim, map points.
const RIM_POINTS: f32 = 1.0;

/// What a place is, for its badge colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::live_layers) enum Group {
    Food,
    Shopping,
    Lodging,
    Culture,
    Health,
    Education,
    Transport,
    Nature,
    Terrain,
    Water,
    Worship,
    Services,
}

impl Group {
    fn color(self) -> Color32 {
        match self {
            Self::Food => Color32::from_rgb(232, 131, 58),
            Self::Shopping => Color32::from_rgb(142, 107, 217),
            Self::Lodging => Color32::from_rgb(194, 84, 155),
            Self::Culture => Color32::from_rgb(38, 154, 160),
            Self::Health => Color32::from_rgb(214, 69, 69),
            Self::Education => Color32::from_rgb(160, 112, 64),
            Self::Transport => Color32::from_rgb(60, 120, 200),
            Self::Nature => Color32::from_rgb(62, 155, 79),
            Self::Terrain => Color32::from_rgb(150, 105, 70),
            Self::Water => Color32::from_rgb(38, 132, 196),
            Self::Worship => Color32::from_rgb(98, 108, 122),
            Self::Services => Color32::from_rgb(112, 124, 138),
        }
    }
}

/// A badge with its symbol at `at`, `scale` the map text scale.
pub(super) fn badge(
    out: &mut Vec<Shape>,
    at: Pos2,
    (group, symbol): (Group, Symbol),
    scale: f32,
    opacity: f32,
) {
    let r = BADGE_RADIUS_POINTS * scale;
    let white = Color32::WHITE.gamma_multiply(opacity);
    out.push(Shape::Circle(CircleShape {
        center: at,
        radius: r,
        fill: group.color().gamma_multiply(opacity),
        stroke: Stroke::new(RIM_POINTS * scale, white),
    }));
    let pen = Pen {
        at,
        size: r * SYMBOL_SHARE,
        stroke: Stroke::new(SYMBOL_STROKE_POINTS * scale, white),
        fill: white,
    };
    pen.symbol(out, symbol);
}
