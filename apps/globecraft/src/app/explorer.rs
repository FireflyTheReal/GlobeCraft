//! Optional explorer utilities. Does not modify the CPU globe renderer.
use globecraft_core::layers::MapLayer;
mod files;
use super::App;
use eframe::egui::{self, TextureHandle};
use globecraft_core::body::Body;
use globecraft_data::{
    files::{FileJobs, ImageSource, ImageTarget},
    links::photo_search,
};
use std::{process::Command, sync::Arc};

#[derive(Default)]
pub(super) struct Explorer {
    pub open: bool,
    name: String,
    image_path: String,
    sky_path: String,
    image_bytes: Option<Arc<Vec<u8>>>,
    sky_bytes: Option<Arc<Vec<u8>>>,
    pending: bool,
    activate_on_completion: bool,
    moon_loading: bool,
    pub viewer: Option<TextureHandle>,
    pub active: Body,
    moon: Option<TextureHandle>,
    pub sky: Option<TextureHandle>,
    pub(super) message: String,
    tour_text: String,
    generated_tour_text: String,
}

impl Explorer {
    pub fn choose(&mut self, body: Body, jobs: &FileJobs) {
        self.active = body;
        self.activate_on_completion = false;
        if body == Body::Moon && self.moon.is_none() && !self.moon_loading {
            match jobs.load_image(
                ImageTarget::Moon,
                ImageSource::Embedded(include_bytes!("../../assets/lroc_color_2k.jpg")),
            ) {
                Ok(()) => self.moon_loading = true,
                Err(error) => self.message = format!("Moon image request failed: {error}"),
            }
        }
    }
    pub(super) fn has_custom_document(&self) -> bool {
        self.viewer.is_some()
    }
    pub fn active_texture(&self) -> Option<&TextureHandle> {
        match self.active {
            Body::Moon => self.moon.as_ref(),
            Body::Custom => self.viewer.as_ref(),
            Body::Earth => None,
        }
    }
}
#[cfg(target_os = "windows")]
const SPEECH_TEXT_VAR: &str = "GLOBECRAFT_SPEECH_TEXT";

// `powershell -Command` appends trailing arguments to the script source instead
// of binding `$args`, so narration on the command line would be parsed as code.
// The text travels in an environment variable and is only ever read as data.
#[cfg(target_os = "windows")]
fn windows_speech_command(text: &str) -> Command {
    let mut command = Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Add-Type -AssemblyName System.Speech; \
                 $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                 $s.Speak($env:{SPEECH_TEXT_VAR})"
            ),
        ])
        .env(SPEECH_TEXT_VAR, text);
    command
}

fn speak(text: &str, _lang: globecraft_core::language::Lang) -> Result<(), String> {
    // System TTS, without uploading the narration to any cloud service.
    #[cfg(target_os = "macos")]
    let result = Command::new("say").arg(text).spawn();
    #[cfg(target_os = "windows")]
    let result = windows_speech_command(text).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let result = Command::new("espeak")
        .args([
            "-v",
            if _lang == globecraft_core::language::Lang::PtBr {
                "pt-br"
            } else {
                "en"
            },
            text,
        ])
        .spawn();
    result
        .map(|_| ())
        .map_err(|e| format!("TTS unavailable (install espeak on Linux): {e}"))
}

impl App {
    pub(super) fn explorer_window(&mut self, ctx: &egui::Context) {
        if !self.explorer.open {
            return;
        }
        let lg = self.lang;
        let mut open = self.explorer.open;
        egui::Window::new(lg.text("explorer-title"))
            .open(&mut open)
            .default_width(390.0)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| {
                let label = self.explorer_location(ui);
                ui.separator();
                self.explorer_tour(ui, &label);
                ui.separator();
                self.explorer_layers(ui);
                ui.separator();
                self.explorer_bodies(ui);
                if !self.explorer.message.is_empty() {
                    ui.separator();
                    ui.label(&self.explorer.message);
                }
            });
        self.explorer.open = open;
    }

    /// The selected place (the first search result, else the view centre) and its links.
    /// Returns the place's label for the tour.
    fn explorer_location(&mut self, ui: &mut egui::Ui) -> String {
        let lg = self.lang;
        ui.heading(lg.text("explorer-selected-location"));
        let selected = self.results.first();
        let label = selected
            .map(|r| r.name.clone())
            .unwrap_or_else(|| self.query.clone());
        ui.label(if label.trim().is_empty() {
            lg.text("explorer-search-place")
        } else {
            label.as_str()
        });
        let (id, (lat, lon)) = match selected {
            Some(r) => ("explorer-latlon", (r.lat, r.lon)),
            None => ("explorer-center", (self.cam.lat, self.cam.lon)),
        };
        ui.monospace(lg.format(
            id,
            &[
                ("latitude", format!("{lat:.6}").into()),
                ("longitude", format!("{lon:.6}").into()),
            ],
        ));
        ui.horizontal_wrapped(|ui| {
            // Links open in the system browser through egui, on every platform.
            if ui.button(lg.text("explorer-place-photos")).clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(photo_search(&label)));
                self.explorer.message = lg.text("explorer-photos-opened").into();
            }
            if ui.button(lg.text("explorer-view-osm")).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                    "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}"
                )));
                self.explorer.message = lg.text("explorer-map-opened").into();
            }
        });
        ui.small(lg.text("explorer-photo-copyright"));
        label
    }

    fn explorer_tour(&mut self, ui: &mut egui::Ui, label: &str) {
        let lg = self.lang;
        ui.heading(lg.text("explorer-narrated-tour"));
        let generated_or_empty = self.explorer.tour_text.is_empty()
            || self.explorer.tour_text == self.explorer.generated_tour_text;
        if !label.trim().is_empty() && generated_or_empty {
            // Preserve user-written narration; only refresh our autogenerated message.
            let generated = lg.format("explorer-tour-default", &[("place", label.into())]);
            self.explorer.tour_text = generated.clone();
            self.explorer.generated_tour_text = generated;
        }
        ui.add(egui::TextEdit::multiline(&mut self.explorer.tour_text).desired_rows(3));
        if ui.button(lg.text("explorer-read-aloud")).clicked() {
            self.explorer.message = match speak(&self.explorer.tour_text, lg) {
                Ok(()) => lg.text("explorer-speech-sent").into(),
                Err(error) => error,
            };
        }
    }

    fn explorer_layers(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        ui.heading(lg.text("explorer-globe-layers"));
        ui.small(lg.text("explorer-layer-info"));
        self.layer_checkbox(ui, MapLayer::Radar, lg.text("layers-weather-toggle"));
        self.layer_checkbox(ui, MapLayer::Clouds, lg.text("layers-clouds-toggle"));
        let (clouds, radar) = (self.layer(MapLayer::Clouds), self.layer(MapLayer::Radar));
        if clouds || radar {
            ui.small(self.live_maps.status(clouds, radar, lg));
        }
        if clouds {
            ui.label(lg.text("explorer-cloud-credit"));
            ui.hyperlink_to(
                lg.text("explorer-cloud-source"),
                "https://github.com/matteason/live-cloud-maps",
            );
            ui.small(lg.text("explorer-cloud-license"));
        }
        if radar {
            ui.label(lg.text("explorer-radar-credit"));
            ui.hyperlink_to(
                lg.text("explorer-radar-source"),
                "https://www.rainviewer.com/",
            );
            ui.small(lg.text("explorer-radar-explanation"));
        }
        self.layer_checkbox(ui, MapLayer::Flights, lg.text("layers-flights-toggle"));
        if self.layer(MapLayer::Flights) {
            super::ui::aircraft::draw_status(ui, &self.live_aircraft.snapshot(), lg);
            ui.hyperlink_to("OpenSky Network", "https://opensky-network.org/");
            ui.hyperlink_to("ADSB.lol (ODbL)", "https://www.adsb.lol/");
        }
    }

    fn explorer_bodies(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        ui.heading(lg.text("explorer-bodies"));
        ui.horizontal(|ui| {
            for (body, label) in [
                (Body::Earth, "explorer-earth"),
                (Body::Moon, "explorer-moon"),
                (Body::Custom, "explorer-custom-planet"),
            ] {
                if ui.button(lg.text(label)).clicked() {
                    self.flat_earth.open = false;
                    self.explorer.choose(body, &self.file_jobs);
                }
            }
        });
        ui.small(lg.text("explorer-moon-fixed"));
        if self.explorer.pending || self.explorer.moon_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(lg.text("explorer-file-working"));
            });
        }
        ui.add_enabled_ui(!self.explorer.pending, |ui| {
            self.explorer_planet_files(ui);
            ui.separator();
            self.explorer_sky_files(ui);
        });
    }

    fn explorer_planet_files(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        ui.horizontal(|ui| {
            ui.label(lg.text("explorer-name"));
            ui.text_edit_singleline(&mut self.explorer.name);
        });
        ui.horizontal(|ui| {
            ui.label(lg.text("explorer-image"));
            let hint = if self.explorer.image_bytes.is_some() {
                lg.text("explorer-embedded-image")
            } else {
                ""
            };
            let edit = egui::TextEdit::singleline(&mut self.explorer.image_path)
                .hint_text(hint)
                .desired_width(200.0);
            if ui.add(edit).changed() {
                self.explorer.image_bytes = None;
            }
            if ui.button(lg.text("explorer-browse")).clicked()
                && let Some(path) = image_dialog().pick_file()
            {
                self.explorer.image_path = path.to_string_lossy().into();
                self.explorer.image_bytes = None;
            }
        });
        if ui.button(lg.text("explorer-display-planet")).clicked() {
            self.explorer
                .request_image(ImageTarget::Planet, &self.file_jobs);
        }
        if let Some(texture) = &self.explorer.viewer {
            ui.image((texture.id(), egui::vec2(260.0, 150.0)));
        }
        ui.horizontal(|ui| {
            if ui.button(lg.text("explorer-save-planet")).clicked()
                && let Some(path) = planet_dialog()
                    .set_file_name("my-planet.globeplanet")
                    .save_file()
            {
                self.explorer.request_save(path, &self.file_jobs);
            }
            if ui.button(lg.text("explorer-load-planet")).clicked()
                && let Some(path) = planet_dialog().pick_file()
            {
                self.explorer.request_open(path, &self.file_jobs);
            }
        });
    }

    fn explorer_sky_files(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        ui.heading(lg.text("explorer-sky-galaxy"));
        ui.horizontal(|ui| {
            let hint = lg.text(if self.explorer.sky_bytes.is_some() {
                "explorer-embedded-image"
            } else {
                "explorer-image-path"
            });
            let edit = egui::TextEdit::singleline(&mut self.explorer.sky_path)
                .hint_text(hint)
                .desired_width(210.0);
            if ui.add(edit).changed() {
                self.explorer.sky_bytes = None;
            }
            if ui.button(lg.text("explorer-browse")).clicked()
                && let Some(path) = image_dialog().pick_file()
            {
                self.explorer.sky_path = path.to_string_lossy().into();
                self.explorer.sky_bytes = None;
            }
        });
        if ui.button(lg.text("explorer-display-galaxy")).clicked() {
            self.explorer
                .request_image(ImageTarget::Sky, &self.file_jobs);
        }
        if let Some(texture) = &self.explorer.sky {
            ui.image((texture.id(), egui::vec2(260.0, 120.0)));
        }
        ui.small(lg.text("explorer-sky-preview"));
    }
}

fn image_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"])
}

/// Planet documents: `.globeplanet`, and `.earthplanet` from before the rename.
pub(super) const PLANET_EXTENSIONS: [&str; 2] = ["globeplanet", "earthplanet"];

pub(super) fn planet_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("GlobeCraft planet", &PLANET_EXTENSIONS)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn windows_speech_text_is_passed_as_data_not_script() {
        let text = "; Write-Output INJECTED\nWe are exploring \"Paris\" & $(calc)";
        let command = windows_speech_command(text);
        let args: Vec<_> = command.get_args().collect();
        assert!(
            args.iter()
                .all(|arg| !arg.to_string_lossy().contains("INJECTED"))
        );
        let script = args.last().unwrap().to_string_lossy();
        assert!(script.ends_with(&format!("$s.Speak($env:{SPEECH_TEXT_VAR})")));
        let envs: Vec<_> = command.get_envs().collect();
        assert_eq!(
            envs,
            [(
                std::ffi::OsStr::new(SPEECH_TEXT_VAR),
                Some(std::ffi::OsStr::new(text))
            )]
        );
    }
}
