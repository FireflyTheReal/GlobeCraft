use super::theme::Tokens;
use super::widgets::section_heading;
use crate::app::App;
use eframe::egui;
const PLACES_DETAILS_HEIGHT_POINTS: f32 = 135.0;
const PLACES_ADD_HEIGHT_POINTS: f32 = 84.0;
const MIN_TREE_HEIGHT_POINTS: f32 = 64.0;

/// Search results listed at the top of Places at most; the rest scroll.
const SEARCH_RESULTS_MAX_HEIGHT_POINTS: f32 = 160.0;

impl App {
    /// Places: the last search's results, then the user's placemarks and folders. Opening
    /// and saving KML files is in the File menu and on Explore › Places; a KML file dropped
    /// anywhere on the window opens.
    pub(in crate::app) fn places_pane(&mut self, ui: &mut egui::Ui) {
        self.search_results(ui);
        let details_height = if self.places.selection.is_some() {
            PLACES_DETAILS_HEIGHT_POINTS
        } else {
            0.0
        };
        let tree_height = (ui.available_height() - PLACES_ADD_HEIGHT_POINTS - details_height)
            .max(MIN_TREE_HEIGHT_POINTS);
        let goto = egui::ScrollArea::vertical()
            .id_salt("places-viewer")
            .auto_shrink([false, false])
            .show(ui, |ui| self.places_tree(ui, tree_height))
            .inner;
        self.places.reveal_requested = false;
        if let Some((lat, lon, alt)) = goto {
            self.fly_to(lat, lon, alt);
        }
    }

    /// The last search's results, as in Google Earth's Search panel: a click flies there.
    fn search_results(&mut self, ui: &mut egui::Ui) {
        if self.search.results.is_empty() {
            return;
        }
        let t = Tokens::get(ui.ctx());
        let lg = self.lang;
        let mut goto = None;
        let mut clear = false;
        ui.horizontal(|ui| {
            section_heading(ui, lg.text("search-results-label"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::widgets::icon_button(ui, "close", lg.text("cmd-search-clear")).clicked() {
                    clear = true;
                }
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("search-results")
            .max_height(SEARCH_RESULTS_MAX_HEIGHT_POINTS)
            .show(ui, |ui| {
                for r in &self.search.results {
                    let short: String = r.name.chars().take(64).collect();
                    let resp = super::widgets::row_button(
                        ui,
                        true,
                        egui::Button::new(egui::RichText::new(short).size(12.0).color(t.text))
                            .frame(false)
                            .wrap(),
                    );
                    if resp.on_hover_text(&r.name).clicked() {
                        goto = Some((r.lat, r.lon, r.alt));
                    }
                }
            });
        ui.separator();
        if let Some((lat, lon, alt)) = goto {
            let _ = self.run(
                "camera.flyTo",
                serde_json::json!({"lat": lat, "lon": lon, "altitude_km": alt}),
            );
        }
        if clear {
            self.run_reporting("search.clear", serde_json::Value::Null);
        }
    }
}
