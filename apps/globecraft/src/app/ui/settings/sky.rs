use crate::app::App;
use eframe::egui;
impl App {
    pub(in crate::app) fn sky_ready(&self) -> bool {
        self.sky.image.is_some() && self.explorer.active == globecraft_core::body::Body::Earth
    }
    pub(super) fn settings_sky(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.checkbox(
            &mut self.settings.sky.cinematic,
            lang.text("layers-stars-toggle"),
        );
        ui.weak(lang.text("sky-help"));
        if self.sky.is_loading() {
            ui.label(lang.text("sky-loading"));
        }
        if let Some(error) = &self.sky.error {
            ui.label(format!(
                "Celestial background failed: {error}. Using procedural stars."
            ));
            if ui.button(lang.text("sky-retry")).clicked() {
                self.sky.retry();
            }
        }
        ui.hyperlink_to(
            lang.text("sky-short-credit"),
            globecraft_data::sky::SOURCE_URL,
        );
        ui.weak(globecraft_data::sky::CREDIT);
        ui.collapsing(lang.text("sky-software-credits"), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("sofa-license")
                .max_height(160.0)
                .show(ui, |ui| {
                    ui.label(include_str!(
                        "../../../../../../assets/sky/sofars-LICENSE.txt"
                    ));
                });
        });
        ui.separator();
    }
}
