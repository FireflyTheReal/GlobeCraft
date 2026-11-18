//! The aurora oval (roadmap 5, `HAZ-01`): NOAA's OVATION forecast. The renderer draws
//! it as light in the air (`globecraft_render::aurora`); here are its legend key and
//! when it shows.
use eframe::egui::Color32;

/// The forecast shows while the app's time is this close to its moment.
pub(in crate::app) const SHOWN_WITHIN_SECS: f64 = 3.0 * 3600.0;

/// The glow's colour at a chance (%), at night seen from above (the legend's key).
pub(in crate::app) fn legend_colour(chance: u8) -> Color32 {
    let [r, g, b] = globecraft_render::aurora::straight_down(f32::from(chance));
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
