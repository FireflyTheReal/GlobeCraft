//! View links (roadmap 2.6): "Copy View Link" puts the camera, the basemap, the
//! layers shown and the user's web data on the clipboard as a `globecraft://` link;
//! opening one (search box, Command Search, `view.openLink`, or the link given on
//! the command line, as Linux hands it over from a browser) brings that view back.
use super::App;
use globecraft_core::layers::MapLayer;
use globecraft_core::view_link::ViewLink;
use globecraft_data::user_data::UserSource;
use serde_json::{Value, json};

/// Web data from the Add Data library travels by name: `preset:gebco`.
const PRESET_PREFIX: &str = "preset:";

impl App {
    /// The view now, as a link.
    pub(super) fn view_link(&self) -> ViewLink {
        let data = self
            .settings
            .user_layers
            .iter()
            .filter(|layer| layer.visible)
            .filter_map(|layer| match (&layer.preset, &layer.source) {
                // A library source by its name, so it opens with its own settings.
                (Some(preset), _) => Some(format!("{PRESET_PREFIX}{preset}")),
                (None, UserSource::Tiles { template, .. }) => Some(template.clone()),
                // Files on this computer mean nothing elsewhere.
                (None, UserSource::Features { location, .. }) if location.starts_with("http") => {
                    Some(location.clone())
                }
                _ => None,
            })
            .collect();
        ViewLink {
            lat: self.cam.lat,
            lon: self.cam.lon,
            altitude_km: Some(self.cam.alt),
            heading_deg: Some(self.cam.heading),
            tilt_deg: Some(self.cam.tilt),
            basemap: Some(self.settings.layers.basemap.code().to_owned()),
            layers: Some(
                MapLayer::ALL
                    .into_iter()
                    .filter(|layer| layer.soloable() && self.layer(*layer))
                    .map(|layer| layer.code().to_owned())
                    .collect(),
            ),
            data,
        }
    }

    /// `view.copyLink`: the link to the clipboard (and in the answer).
    pub(super) fn copy_view_link(&mut self, ctx: &eframe::egui::Context) -> Value {
        let link = self.view_link().to_link();
        ctx.copy_text(link.clone());
        self.status = self.lang.text("view-link-copied-status").to_owned();
        json!({"link": link})
    }

    /// `view.openLink`: the camera, basemap and layers of a link, and its web data
    /// added (unless already there).
    pub(super) fn open_view_link(&mut self, text: &str) -> Result<Value, String> {
        let link = ViewLink::parse(text)?;
        let mut camera = json!({"lat": link.lat, "lon": link.lon});
        for (key, value) in [
            ("altitude_km", link.altitude_km),
            ("heading_deg", link.heading_deg),
            ("tilt_deg", link.tilt_deg),
        ] {
            if let Some(value) = value {
                camera[key] = json!(value);
            }
        }
        self.run("camera.set", camera)?;
        let mut skipped: Vec<String> = Vec::new();
        if let Some(basemap) = &link.basemap
            && self
                .run("layer.basemap", json!({"basemap": basemap}))
                .is_err()
        {
            skipped.push(basemap.clone());
        }
        if let Some(codes) = &link.layers {
            let shown: Vec<MapLayer> = codes
                .iter()
                .filter_map(|code| {
                    let layer = MapLayer::from_code(code).filter(|layer| layer.soloable());
                    if layer.is_none() {
                        skipped.push(code.clone());
                    }
                    layer
                })
                .collect();
            for layer in MapLayer::ALL.into_iter().filter(|layer| layer.soloable()) {
                self.settings
                    .set_layer_visible(layer, shown.contains(&layer));
            }
            self.solo = None;
        }
        let mut added = 0;
        for url in &link.data {
            let preset = url.strip_prefix(PRESET_PREFIX);
            let present = self.settings.user_layers.iter().any(|layer| {
                preset.is_some_and(|preset| layer.preset.as_deref() == Some(preset))
                    || match &layer.source {
                        UserSource::Tiles { template, .. } => template == url,
                        UserSource::Features { location, .. } => location == url,
                    }
            });
            if present {
                continue;
            }
            let params = match preset {
                Some(preset) => json!({"preset": preset}),
                None => json!({"url": url}),
            };
            match self.run("data.add", params) {
                Ok(_) => added += 1,
                Err(_) => skipped.push(url.clone()),
            }
        }
        self.status = self.lang.text("view-link-opened-status").to_owned();
        Ok(json!({"opened": true, "data_added": added, "not_understood": skipped}))
    }
}

/// A view link given on the command line (`globecraft globecraft://view?…`), as a
/// desktop hands over a link clicked in a browser.
pub(super) fn link_from_args() -> Option<String> {
    std::env::args()
        .skip(1)
        .find(|arg| arg.starts_with(globecraft_core::view_link::PREFIX))
}
