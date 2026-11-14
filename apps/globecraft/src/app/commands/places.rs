//! Places commands (`places.*`).
use super::{position, str_param};
use crate::app::App;
use globecraft_core::placemark::{PlaceItem, PlaceNode, Placemark};
use serde_json::{Value, json};

/// Altitude a placemark is shown from, as in the Places tree.
const PLACE_FLY_ALTITUDE_KM: f64 = 3.0;

impl App {
    /// Runs a `places.*` command.
    pub(super) fn run_places(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        Ok(match id {
            "places.addAtCenter" => {
                if let Some(name) = str_param(p, "name") {
                    self.places.new_name = name.to_string();
                }
                self.add_center_placemark();
                if let Some(desc) = str_param(p, "description")
                    && let Some(node) = self
                        .places
                        .selection
                        .and_then(|id| self.places.items.find_mut(id))
                {
                    *node.description_mut() = desc.to_string();
                }
                self.reveal_places();
                json!({"added": {"lat": self.cam.lat, "lon": self.cam.lon}})
            }
            "places.add" => {
                let (lat, lon) = position(p)?;
                let parent = match str_param(p, "folder").filter(|f| !f.is_empty()) {
                    Some(folder) => Some(
                        self.places
                            .items
                            .find_by_name(folder)
                            .filter(|n| matches!(n.item, PlaceItem::Folder { .. }))
                            .map(PlaceNode::id)
                            .ok_or_else(|| format!("no folder named '{folder}'"))?,
                    ),
                    None => None,
                };
                let name = str_param(p, "name")
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        self.lang.format(
                            "placemarks-generated-name",
                            &[("number", (self.places.items.len() + 1).to_string().into())],
                        )
                    });
                let node = PlaceNode::marker(Placemark {
                    name: name.clone(),
                    desc: str_param(p, "description").unwrap_or_default().to_string(),
                    lat,
                    lon,
                });
                self.places.selection = Some(node.id());
                self.places.items.insert(parent, node);
                json!({"added": name})
            }
            "places.addFolder" => {
                if let Some(name) = str_param(p, "name") {
                    self.places.new_name = name.to_string();
                }
                self.add_places_folder();
                self.reveal_places();
                json!({})
            }
            "places.flyTo" => {
                let name = str_param(p, "name").ok_or("name is required")?;
                let node = self
                    .places
                    .items
                    .find_by_name(name)
                    .ok_or_else(|| format!("no place named '{name}'"))?;
                let PlaceItem::Marker(pm) = &node.item else {
                    return Err(format!("'{name}' is a folder"));
                };
                let (lat, lon) = (pm.lat, pm.lon);
                self.fly_to(lat, lon, PLACE_FLY_ALTITUDE_KM);
                json!({"flying_to": {"lat": lat, "lon": lon}})
            }
            "places.remove" => {
                let name = str_param(p, "name").ok_or("name is required")?;
                let id = self
                    .places
                    .items
                    .find_by_name(name)
                    .map(PlaceNode::id)
                    .ok_or_else(|| format!("no place named '{name}'"))?;
                self.places.items.remove(id);
                if self.places.selection == Some(id) {
                    self.places.selection = None;
                }
                json!({"removed": name})
            }
            "places.open" => {
                self.pick_kml_open();
                json!({})
            }
            "places.save" => {
                let path = self.resolve_kml_path(&self.places.kml_path.clone());
                self.save_kml_to(path.clone());
                json!({"saving": path})
            }
            "places.saveAs" => {
                self.pick_kml_save_as();
                json!({})
            }
            "places.clear" => {
                self.clear_places();
                json!({})
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }
}
