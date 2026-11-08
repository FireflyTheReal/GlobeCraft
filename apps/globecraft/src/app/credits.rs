//! The credits at the bottom right of the globe (roadmap 6.4): one for each source
//! drawn now (the base map, the layers that are on and have data, the user's own
//! layers), in that order. When they do not fit on one line they wrap, a whole
//! credit to a line part, never cut inside one.
use super::App;
use super::globe::OSM_TILES_CREDIT;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect};
use globecraft_core::{body::Body, camera::Basis, language::Lang, layers::MapLayer};
use globecraft_data::tiles::Provider;

const SEPARATOR: &str = " · ";
const FONT_POINTS: f32 = 11.0;
const MARGIN_POINTS: f32 = 8.0;
const BOTTOM_POINTS: f32 = 6.0;
/// The credits take at most this share of the globe's width, leaving the scale bar
/// at the bottom left free.
const WIDTH_SHARE: f32 = 0.7;
const COLOUR: Color32 = Color32::from_rgba_premultiplied(200, 200, 200, 200);
const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 170);

/// Packs the credits into lines no wider than `max`, joined by " · " within a line.
/// A credit wider than `max` on its own gets a line to itself.
fn pack(credits: &[String], width: impl Fn(&str) -> f32, max: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for credit in credits {
        match lines.last_mut() {
            Some(line) if width(&format!("{line}{SEPARATOR}{credit}")) <= max => {
                line.push_str(SEPARATOR);
                line.push_str(credit);
            }
            _ => lines.push(credit.clone()),
        }
    }
    lines
}

/// The height of one line of credits.
fn row_height(painter: &egui::Painter, font: &FontId) -> f32 {
    painter
        .layout_no_wrap("Ag".to_owned(), font.clone(), COLOUR)
        .size()
        .y
}

impl App {
    /// Every source drawn now, each credited once, the base map first.
    pub(super) fn map_credits(&self, lg: Lang, basis: &Basis, rect: Rect) -> Vec<String> {
        let mut credits: Vec<String> = Vec::new();
        let mut base = self.settings.layers.basemap.attribution(lg).to_string();
        if self.settings.layers.basemap == Provider::Wayback
            && let Some(release) = self.wayback.releases.get(self.wayback.index)
        {
            base = lg.format(
                "wayback-snapshot-attribution",
                &[
                    ("attribution", base.into()),
                    ("date", release.date.as_str().into()),
                ],
            );
        }
        credits.push(base);
        let earth = self.explorer.active == Body::Earth;
        if self.layer(MapLayer::NightLights) {
            credits.push(self.night_lights_credit(lg));
        }
        // The polar caps show NASA's imagery past the Mercator limit.
        if earth
            && globecraft_render::polar::under(self.effective_provider())
            && globecraft_render::polar::shows_polar_cap(
                basis,
                rect.width() as usize,
                rect.height() as usize,
            )
        {
            credits.push(lg.text("polar-imagery-attribution").to_owned());
        }
        let transport_drawn = (self.map_data_shown()
            && self.cam.alt <= globecraft_data::transport::MAX_ALTITUDE_KM)
            || (self.layer(MapLayer::Roads)
                && self.cam.alt <= globecraft_data::roads::MAX_ALTITUDE_KM);
        if (self.uses_border_tiles() && self.boundary_loader.has_osm_tiles()) || transport_drawn {
            // The tiles are cut from OpenFreeMap's planet build (OpenMapTiles schema).
            credits.push(OSM_TILES_CREDIT.to_owned());
        }
        if self.layer(MapLayer::TimeZones) && self.live_time_zones.zones().is_some() {
            credits.push(lg.text("timezones-credit").to_owned());
        }
        if self.layer(MapLayer::Aurora) && self.live_aurora.grid().is_some() {
            credits.push(lg.text("aurora-credit").to_owned());
        }
        // The live objects, while there are some to show.
        if earth
            && self.layer(MapLayer::Flights)
            && !self.live_aircraft.snapshot().aircraft.is_empty()
        {
            credits.push(lg.text("aircraft-credit").to_owned());
        }
        if earth
            && self.layer(MapLayer::Vessels)
            && let Some(key) = self.live_vessels.credit_key()
        {
            credits.push(lg.text(key).to_owned());
        }
        if earth && self.layer(MapLayer::Satellites) && !self.live_satellites.snapshot().is_empty()
        {
            credits.push(lg.text("satellites-credit").to_owned());
        }
        if earth && self.layer(MapLayer::Traffic) && !self.tomtom_key.trim().is_empty() {
            credits.push(lg.text("traffic-credit").to_owned());
        }
        // Credits of the layers the user added, as their licences ask.
        if earth {
            credits.extend(
                self.settings
                    .user_layers
                    .iter()
                    .filter(|layer| layer.visible && !layer.attribution.is_empty())
                    .map(|layer| layer.attribution.clone()),
            );
        }
        if self.renderer.displayed_sky {
            credits.push(lg.text("sky-short-credit").to_owned());
        }
        credits.dedup();
        credits
    }

    /// The credits, right-aligned at the bottom right, wrapped upward.
    pub(super) fn paint_credits(&self, painter: &egui::Painter, rect: Rect, credits: &[String]) {
        let font = FontId::proportional(FONT_POINTS * self.settings.map_scale);
        let width = |text: &str| {
            painter
                .layout_no_wrap(text.to_owned(), font.clone(), COLOUR)
                .size()
                .x
        };
        let lines = pack(credits, width, rect.width() * WIDTH_SHARE);
        let line_height = row_height(painter, &font);
        let right = rect.right() - MARGIN_POINTS;
        let mut bottom = rect.bottom() - BOTTOM_POINTS;
        let shadow = egui::vec2(1.0, 1.0) * self.settings.map_scale;
        for line in lines.iter().rev() {
            // A dark copy below keeps them readable over ice and cloud.
            painter.text(
                Pos2::new(right, bottom) + shadow,
                Align2::RIGHT_BOTTOM,
                line,
                font.clone(),
                SHADOW,
            );
            painter.text(
                Pos2::new(right, bottom),
                Align2::RIGHT_BOTTOM,
                line,
                font.clone(),
                COLOUR,
            );
            bottom -= line_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn credits_share_a_line_while_they_fit() {
        let width = |text: &str| text.chars().count() as f32;
        let lines = pack(&credits(&["Esri", "NASA", "CelesTrak"]), width, 40.0);
        assert_eq!(lines, ["Esri · NASA · CelesTrak"]);
    }

    #[test]
    fn credits_wrap_whole_and_a_long_one_keeps_its_own_line() {
        let width = |text: &str| text.chars().count() as f32;
        let lines = pack(
            &credits(&["Esri", "NASA", "a credit far too long", "OpenSky"]),
            width,
            12.0,
        );
        assert_eq!(lines, ["Esri · NASA", "a credit far too long", "OpenSky"]);
    }
}
