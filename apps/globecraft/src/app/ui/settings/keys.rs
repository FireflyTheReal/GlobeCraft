//! Settings -> Keys (roadmap adapter D): one field per service that needs a key, with
//! what it is for and where a free one is had. Keys are saved apart from the
//! settings, readable by the user only (`globecraft_data::keys`).
use crate::app::App;
use eframe::egui;
use globecraft_data::keys::{self, SERVICES};

impl App {
    /// The typed key of a service.
    pub(in crate::app) fn key_buffer(&mut self, id: &'static str) -> &mut String {
        if id == keys::TOMTOM {
            &mut self.tomtom_key
        } else {
            self.service_keys.entry(id).or_default()
        }
    }

    /// Takes a typed key into use and saves the keys in the background.
    pub(in crate::app) fn store_key(&mut self, id: &'static str) {
        let value = self.key_buffer(id).clone();
        let all = keys::set(id, &value);
        self.runtime.handle().spawn_blocking(move || {
            if let Err(error) = keys::save(&all) {
                eprintln!("Saving the service keys failed: {error}");
            }
        });
    }

    pub(super) fn settings_keys(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.weak(lang.text("settings-keys-intro"));
        for service in SERVICES {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(lang.text(service.name_key)).strong());
                ui.hyperlink_to(lang.text("settings-keys-get-one"), service.signup_url);
            });
            ui.weak(lang.text(service.help_key));
            if keys::from_environment(service.id) {
                ui.weak(lang.format(
                    "settings-keys-from-environment",
                    &[(
                        "variable",
                        format!("GLOBECRAFT_{}_KEY", service.id.to_ascii_uppercase()).into(),
                    )],
                ));
                continue;
            }
            let changed = ui
                .add(
                    egui::TextEdit::singleline(self.key_buffer(service.id))
                        .password(true)
                        .hint_text(lang.text("settings-keys-hint"))
                        .desired_width(320.0),
                )
                .changed();
            if changed {
                self.store_key(service.id);
            }
        }
    }
}
