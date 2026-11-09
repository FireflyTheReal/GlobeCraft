//! "What's here?" (roadmap 6.12): everything GlobeCraft knows about a point, from data
//! already in memory: its coordinates in every format, the ground height, the country,
//! state and municipality, the nearest named place, the time and the sun. Shown as a
//! card (`ui.here`: the globe's context menu, or press and hold on the globe), and as
//! `info.here` for the assistant.
use super::App;
use eframe::egui;
use globecraft_core::coordinates::{CoordinateDisplay, CoordinateFormat};
use globecraft_core::language::Lang;
use globecraft_core::math::{haversine_km, ll_to_v3, subsolar};
use globecraft_data::boundaries::{AdminAreas, AdminLookup};
use serde_json::{Value, json};

/// Named places farther than this are not called "nearest".
const NEAREST_PLACE_MAX_KM: f64 = 300.0;
/// While the ground height loads, the card looks again this often.
const ELEVATION_POLL: std::time::Duration = std::time::Duration::from_millis(400);

/// The point the card shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HereCard {
    pub lat: f64,
    pub lon: f64,
}

/// What is known about a point.
pub(super) struct HereInfo {
    pub coordinates: Vec<(CoordinateFormat, String)>,
    pub elevation_m: Option<f32>,
    pub country: Option<String>,
    /// The state and municipality, from the border tiles in memory.
    pub admin: AdminAreas,
    /// The nearest named place, and its distance (km) and bearing (degrees).
    pub nearest: Option<(String, f64, f64)>,
    /// Hours and minutes of UTC, and of mean solar time at the point.
    pub utc: (u32, u32),
    pub solar: (u32, u32),
    /// The nautical time zone (UTC offset in hours).
    pub nautical_zone: i32,
    /// The sun's height above the horizon, in degrees.
    pub sun_altitude_deg: f64,
}

fn clock(seconds_of_day: f64) -> (u32, u32) {
    let minutes = (seconds_of_day / 60.0).rem_euclid(24.0 * 60.0) as u32;
    (minutes / 60, minutes % 60)
}

/// The initial bearing from `a` to `b`, degrees clockwise from north.
fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1) = (a.0.to_radians(), a.1.to_radians());
    let (la2, lo2) = (b.0.to_radians(), b.1.to_radians());
    let y = (lo2 - lo1).sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * (lo2 - lo1).cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// The compass point of a bearing (N, NE, …).
fn compass_point(bearing: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[((bearing + 22.5).rem_euclid(360.0) / 45.0) as usize % 8]
}

/// An administrative level's name for `info.here`, or null and why (in English, for
/// the model).
fn admin_json(lookup: &AdminLookup, lang: Lang, not_loaded: &str) -> (Value, Option<String>) {
    match lookup {
        AdminLookup::Found(label) => (json!(label.text(lang)), None),
        AdminLookup::NotLoaded => (Value::Null, Some(not_loaded.to_owned())),
        AdminLookup::NoName => (
            Value::Null,
            Some(
                "no name lies inside the same borders as the point in the loaded border tiles"
                    .into(),
            ),
        ),
    }
}

/// The card's line for an administrative level: the name, or a dash and why.
fn admin_row(ui: &mut egui::Ui, lang: Lang, lookup: &AdminLookup, not_loaded_help: &str) {
    match lookup {
        AdminLookup::Found(label) => {
            ui.label(label.text(lang));
        }
        AdminLookup::NotLoaded => {
            ui.label("—").on_hover_text(lang.text(not_loaded_help));
        }
        AdminLookup::NoName => {
            ui.label("—")
                .on_hover_text(lang.text("here-admin-no-name-help"));
        }
    }
}

/// The sun's height above the horizon at a point and time, degrees.
pub(super) fn sun_altitude_deg(lat: f64, lon: f64, unix: f64) -> f64 {
    let (sun_lat, sun_lon) = subsolar(unix);
    let cosine = ll_to_v3(lat, lon)
        .dot(ll_to_v3(sun_lat, sun_lon))
        .clamp(-1.0, 1.0);
    90.0 - cosine.acos().to_degrees()
}

/// The cards over the globe; one is open at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Card {
    Flight,
    Satellite,
    Here,
}

impl App {
    /// Closes the cards other than `keep`, which is opening: the globe shows one card.
    pub(super) fn close_other_cards(&mut self, keep: Card) {
        if keep != Card::Flight {
            self.flight_selection.clear();
            self.selected_snapshot = None;
        }
        if keep != Card::Satellite {
            self.satellite_selection.clear();
        }
        if keep != Card::Here {
            self.here_card = None;
        }
    }
}

impl App {
    pub(super) fn here_info(&self, lat: f64, lon: f64, admin: AdminAreas) -> HereInfo {
        let coordinates = CoordinateFormat::ALL
            .into_iter()
            .filter_map(|format| {
                let display = CoordinateDisplay {
                    format,
                    precision: format.default_precision(),
                };
                display.format(lat, lon).ok().map(|text| (format, text))
            })
            .collect();
        // The finest ground height; asking for it loads the elevation tile.
        let elevation_m = self.cache.elevation_m(
            lat,
            lon,
            globecraft_data::tiles::Provider::TerrainDem.max_zoom(),
        );
        let nearest = self
            .boundaries
            .labels
            .iter()
            .filter(|label| label.kind == globecraft_data::boundaries::LabelKind::City)
            .map(|label| (label, haversine_km((lat, lon), (label.lat, label.lon))))
            .filter(|(_, km)| *km <= NEAREST_PLACE_MAX_KM)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(label, km)| {
                (
                    label.text(self.lang).to_owned(),
                    km,
                    bearing_deg((label.lat, label.lon), (lat, lon)),
                )
            });
        // The app's time: the card answers for the moment the globe shows.
        let unix = self.scene_unix();
        let seconds_of_day = unix.rem_euclid(86_400.0);
        HereInfo {
            coordinates,
            elevation_m,
            country: self
                .boundaries
                .country_name_at(lat, lon, self.lang)
                .map(str::to_owned),
            admin,
            nearest,
            utc: clock(seconds_of_day),
            solar: clock(seconds_of_day + lon / 15.0 * 3_600.0),
            nautical_zone: (lon / 15.0).round() as i32,
            sun_altitude_deg: sun_altitude_deg(lat, lon, unix),
        }
    }

    /// `info.here`: what is known about a point, as JSON.
    pub(super) fn here_json(&self, lat: f64, lon: f64) -> Value {
        let admin = self.boundary_loader.admin_areas_at(lat, lon);
        let info = self.here_info(lat, lon, admin);
        let (state, state_unavailable) = admin_json(
            &info.admin.state,
            self.lang,
            "state borders around this point are not loaded: they load around the view",
        );
        let (municipality, municipality_unavailable) = admin_json(
            &info.admin.municipality,
            self.lang,
            "municipal borders load only while the view is zoomed in close to the point",
        );
        let coordinates: serde_json::Map<String, Value> = info
            .coordinates
            .iter()
            .map(|(format, text)| (format.code().to_owned(), json!(text)))
            .collect();
        let mut out = json!({
            "lat": lat,
            "lon": lon,
            "coordinates": coordinates,
            "elevation_m": info.elevation_m,
            "country": info.country,
            "state": state,
            // Named after the most important place inside the same municipal borders.
            "municipality": municipality,
            "nearest_place": info.nearest.as_ref().map(|(name, km, bearing)| json!({
                "name": name,
                "distance_km": (km * 10.0).round() / 10.0,
                "direction_from_place": compass_point(*bearing),
            })),
            "utc": format!("{:02}:{:02}", info.utc.0, info.utc.1),
            "mean_solar_time": format!("{:02}:{:02}", info.solar.0, info.solar.1),
            "nautical_time_zone": format!("UTC{:+}", info.nautical_zone),
            "sun_altitude_deg": (info.sun_altitude_deg * 10.0).round() / 10.0,
        });
        for (key, reason) in [
            ("state_unavailable", state_unavailable),
            ("municipality_unavailable", municipality_unavailable),
        ] {
            if let Some(reason) = reason {
                out[key] = json!(reason);
            }
        }
        out
    }

    /// The card's state and municipality, looked up again only when the point or the
    /// border tiles in memory change.
    fn here_admin(&mut self, card: HereCard) -> AdminAreas {
        let revision = self.boundary_loader.border_tiles_revision();
        match &self.here_admin {
            Some((at, seen, admin)) if *at == card && *seen == revision => admin.clone(),
            _ => {
                let admin = self.boundary_loader.admin_areas_at(card.lat, card.lon);
                self.here_admin = Some((card, revision, admin.clone()));
                admin
            }
        }
    }

    /// The card, while one is open.
    pub(super) fn here_window(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Some(card) = self.here_card else {
            return;
        };
        let lang = self.lang;
        let admin = self.here_admin(card);
        let info = self.here_info(card.lat, card.lon, admin);
        if info.elevation_m.is_none() {
            ctx.request_repaint_after(ELEVATION_POLL);
        }
        let mut action: Option<(&str, Value)> = None;
        let (open, _) = super::ui::surface::Surface::card(
            "here_card",
            lang.text("here-title"),
            rect,
        )
        .show(ctx, lang, |ui| {
            egui::Grid::new("here_card_grid")
                .num_columns(3)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    for (format, text) in &info.coordinates {
                        ui.weak(lang.text(format.name_id()));
                        ui.monospace(text);
                        if super::ui::icons::icon_button(ui, "copy", lang.text("here-copy-help"))
                            .clicked()
                        {
                            ui.copy_text(text.clone());
                        }
                        ui.end_row();
                    }
                });
            ui.separator();
            egui::Grid::new("here_card_facts")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.weak(lang.text("here-elevation-label"));
                    ui.label(info.elevation_m.map_or_else(
                        || lang.text("here-loading").to_owned(),
                        |metres| self.distance_units().elevation(f64::from(metres)),
                    ));
                    ui.end_row();
                    ui.weak(lang.text("here-country-label"));
                    ui.label(
                        info.country
                            .clone()
                            .unwrap_or_else(|| lang.text("here-no-country").to_owned()),
                    );
                    ui.end_row();
                    ui.weak(lang.text("here-state-label"));
                    admin_row(ui, lang, &info.admin.state, "here-state-not-loaded-help");
                    ui.end_row();
                    ui.weak(lang.text("here-municipality-label"))
                        .on_hover_text(lang.text("here-municipality-help"));
                    admin_row(
                        ui,
                        lang,
                        &info.admin.municipality,
                        "here-municipality-not-loaded-help",
                    );
                    ui.end_row();
                    ui.weak(lang.text("here-nearest-label"));
                    ui.label(info.nearest.as_ref().map_or_else(
                        || "—".to_owned(),
                        |(name, km, bearing)| {
                            lang.format(
                                "here-nearest-value",
                                &[
                                    ("name", name.as_str().into()),
                                    ("distance", self.distance_units().distance(*km).into()),
                                    ("direction", compass_point(*bearing).into()),
                                ],
                            )
                        },
                    ));
                    ui.end_row();
                    ui.weak(lang.text("here-time-label"));
                    ui.label(lang.format(
                        "here-time-value",
                        &[
                            (
                                "solar",
                                format!("{:02}:{:02}", info.solar.0, info.solar.1).into(),
                            ),
                            ("utc", format!("{:02}:{:02}", info.utc.0, info.utc.1).into()),
                            ("zone", format!("UTC{:+}", info.nautical_zone).into()),
                        ],
                    ))
                    .on_hover_text(lang.text("here-time-help"));
                    ui.end_row();
                    ui.weak(lang.text("here-sun-label"));
                    ui.label(lang.format(
                        if info.sun_altitude_deg >= 0.0 {
                            "here-sun-up"
                        } else {
                            "here-sun-down"
                        },
                        &[(
                            "degrees",
                            format!("{:.0}", info.sun_altitude_deg.abs()).into(),
                        )],
                    ));
                    ui.end_row();
                });
            super::ui::surface::footer(ui, |ui| {
                let at = json!({"lat": card.lat, "lon": card.lon});
                if super::ui::icons::button(ui, "places", lang.text("globe-placemark-action"))
                    .clicked()
                {
                    action = Some(("places.add", at.clone()));
                }
                if super::ui::icons::button(ui, "fly", lang.text("globe-fly-to-action")).clicked() {
                    action = Some(("camera.flyTo", at));
                }
            });
        });
        if let Some((id, params)) = action {
            self.run_reporting(id, params);
        }
        if !open {
            self.here_card = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_wrap_around_midnight() {
        assert_eq!(clock(0.0), (0, 0));
        assert_eq!(clock(-60.0), (23, 59));
        assert_eq!(clock(86_400.0 + 3_660.0), (1, 1));
    }

    #[test]
    fn bearings_name_the_compass_point() {
        assert_eq!(compass_point(bearing_deg((0.0, 0.0), (1.0, 0.0))), "N");
        assert_eq!(compass_point(bearing_deg((0.0, 0.0), (0.0, 1.0))), "E");
        assert_eq!(compass_point(bearing_deg((0.0, 0.0), (-1.0, -1.0))), "SW");
    }

    #[test]
    fn the_sun_is_overhead_at_the_subsolar_point_and_down_opposite() {
        let unix = 1_790_000_000.0;
        let (lat, lon) = subsolar(unix);
        assert!((sun_altitude_deg(lat, lon, unix) - 90.0).abs() < 1e-6);
        assert!((sun_altitude_deg(-lat, lon + 180.0, unix) + 90.0).abs() < 1e-6);
    }
}
