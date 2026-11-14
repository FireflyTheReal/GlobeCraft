//! Read-only views of the application (`info.*`), for the interface and the assistant.
use super::position;
use crate::app::App;
use globecraft_core::{
    body::Body,
    language::Lang,
    layers::MapLayer,
    placemark::{PlaceItem, PlaceNode},
};
use serde_json::{Value, json};

impl App {
    /// Runs an `info.*` command, or `ui.here`.
    pub(super) fn run_info(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        Ok(match id {
            "info.view" => self.view_json(),
            "info.places" => {
                let mut places = Vec::new();
                self.places
                    .items
                    .walk(|depth, node| places.push(place_json(depth, node)));
                json!({"count": self.places.items.len(), "items": places})
            }
            "info.ruler" => self.ruler_json(),
            "info.here" => {
                let (lat, lon) = position(p)?;
                self.here_json(lat, lon)
            }
            "ui.here" => {
                let (lat, lon) = position(p)?;
                self.close_other_cards(crate::app::here::Card::Here);
                self.pointer.here_card = Some(crate::app::here::HereCard { lat, lon });
                json!({"lat": lat, "lon": lon})
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }

    pub(in crate::app) fn ruler_json(&self) -> Value {
        let model = self.active_earth_model();
        let total_km = self.measure.paths.distance_km(model);
        let paths: Vec<Vec<[f64; 2]>> = self
            .measure
            .paths
            .paths()
            .map(|path| path.iter().map(|(lat, lon)| [*lat, *lon]).collect())
            .collect();
        let bearing = self
            .measure
            .paths
            .last_leg()
            .and_then(|pair| model.initial_bearing(pair[0], pair[1]));
        json!({
            "enabled": self.measure.on,
            "closed_shapes": self.measure.paths.closed(),
            "area_km2": self.measure.paths.area_km2(model),
            "points": self.measure.paths.point_count(),
            "paths_lat_lon": paths,
            "total_km": total_km,
            "total_display": self.settings.ruler_unit.format_with_angle(
                total_km,
                self.measure.paths.angular_distance_degrees(model),
                self.distance_units(),
                Lang::En,
            ),
            "last_bearing_deg": bearing,
        })
    }

    /// What is on screen, for `info.view` and the assistant's context.
    pub(in crate::app) fn view_json(&self) -> Value {
        let body = match self.explorer.active {
            Body::Earth => "earth",
            Body::Moon => "moon",
            Body::Custom => "custom",
        };
        let layers: Vec<&str> = self
            .settings
            .visible_layers()
            .iter()
            .map(MapLayer::code)
            .collect();
        let results: Vec<Value> = self
            .search
            .results
            .iter()
            .take(5)
            .map(|r| json!({"name": r.name, "lat": r.lat, "lon": r.lon}))
            .collect();
        let center = self
            .settings
            .coordinate_display
            .format(self.cam.lat, self.cam.lon)
            .ok();
        json!({
            "camera": self.camera_json(),
            "camera_display": center,
            "world": body,
            "earth_model": self.settings.earth_model.code(),
            "basemap": self.settings.layers.basemap.code(),
            "wayback_release": (self.settings.layers.basemap == globecraft_data::tiles::Provider::Wayback)
                .then(|| self.wayback.releases.get(self.wayback.index).map(|r| r.date.clone())).flatten(),
            "visible_layers": layers,
            "user_layers": self.user_layers_json(),
            "units": match self.distance_units() {
                globecraft_core::units::DistanceUnits::Metric => "metric",
                globecraft_core::units::DistanceUnits::Imperial => "imperial",
            },
            "places": self.places.items.len(),
            "ruler_points": self.measure.paths.point_count(),
            "search_query": (!self.search.query.trim().is_empty()).then(|| self.search.query.trim().to_string()),
            "search_results": results,
            "status": self.status,
        })
    }
}

fn place_json(depth: usize, node: &PlaceNode) -> Value {
    match &node.item {
        PlaceItem::Folder {
            name, description, ..
        } => json!({"type": "folder", "name": name, "description": description,
            "visible": node.visible, "depth": depth}),
        PlaceItem::Marker(pm) => json!({"type": "placemark", "name": pm.name,
            "description": pm.desc, "lat": pm.lat, "lon": pm.lon,
            "visible": node.visible, "depth": depth}),
    }
}

/// A compact description of the view, sent with the user's message when it changed.
pub(in crate::app) fn context_text(app: &App) -> String {
    let v = app.view_json();
    let mut lines = vec![format!(
        "Camera: lat {:.4}, lon {:.4}, altitude {:.1} km, heading {:.0}°",
        app.cam.lat, app.cam.lon, app.cam.alt, app.cam.heading
    )];
    lines.push(format!(
        "World: {} · Basemap: {} · Units: {}",
        v["world"].as_str().unwrap_or_default(),
        v["basemap"].as_str().unwrap_or_default(),
        v["units"].as_str().unwrap_or_default()
    ));
    let layers: Vec<&str> = v["visible_layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    lines.push(format!("Visible layers: {}", layers.join(", ")));
    lines.push(format!(
        "Places: {} · Ruler points: {}",
        app.places.items.len(),
        app.measure.paths.point_count()
    ));
    if let Some(results) = v["search_results"].as_array().filter(|r| !r.is_empty()) {
        let names: Vec<String> = results
            .iter()
            .map(|r| {
                r["name"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(60)
                    .collect()
            })
            .collect();
        lines.push(format!("Search results: {}", names.join(" | ")));
    }
    lines.join("\n")
}
