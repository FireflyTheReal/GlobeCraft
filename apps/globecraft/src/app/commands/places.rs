//! Places commands (`places.*`).
use super::position;
use crate::app::App;
use globecraft_core::commands::PlacesCommand;
use globecraft_core::placemark::{PlaceItem, PlaceNode, Placemark};
use serde_json::{Value, json};

/// Altitude a placemark is shown from, as in the Places tree.
const PLACE_FLY_ALTITUDE_KM: f64 = 3.0;

impl App {
    /// Runs a `places.*` command.
    pub(super) fn run_places(&mut self, command: PlacesCommand) -> Result<Value, String> {
        Ok(match command {
            PlacesCommand::AddAtCenter(p) => {
                if let Some(name) = p.name {
                    self.places.new_name = name;
                }
                self.add_center_placemark();
                if let Some(desc) = p.description
                    && let Some(node) = self
                        .places
                        .selection
                        .and_then(|id| self.places.items.find_mut(id))
                {
                    *node.description_mut() = desc;
                }
                self.reveal_places();
                json!({"added": {"lat": self.cam.lat, "lon": self.cam.lon}})
            }
            PlacesCommand::Add(p) => {
                let (lat, lon) = position(p.lat, p.lon)?;
                let parent = match p.folder.as_deref().filter(|f| !f.is_empty()) {
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
                let name = p.name.filter(|n| !n.is_empty()).unwrap_or_else(|| {
                    self.lang.format(
                        "placemarks-generated-name",
                        &[("number", (self.places.items.len() + 1).to_string().into())],
                    )
                });
                let node = PlaceNode::marker(Placemark {
                    name: name.clone(),
                    desc: p.description.unwrap_or_default(),
                    lat,
                    lon,
                    time: None,
                });
                self.places.selection = Some(node.id());
                self.places.items.insert(parent, node);
                json!({"added": name})
            }
            PlacesCommand::AddFolder(p) => {
                if let Some(name) = p.name {
                    self.places.new_name = name;
                }
                self.add_places_folder();
                self.reveal_places();
                json!({})
            }
            PlacesCommand::FlyTo(p) => {
                let name = &p.name;
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
            PlacesCommand::Remove(p) => {
                let name = p.name;
                let id = self
                    .places
                    .items
                    .find_by_name(&name)
                    .map(PlaceNode::id)
                    .ok_or_else(|| format!("no place named '{name}'"))?;
                self.places.items.remove(id);
                if self.places.selection == Some(id) {
                    self.places.selection = None;
                }
                json!({"removed": name})
            }
            PlacesCommand::Open => {
                self.pick_kml_open();
                json!({})
            }
            PlacesCommand::Save => {
                let path = self.resolve_kml_path(&self.places.kml_path.clone());
                self.save_kml_to(path.clone());
                json!({"saving": path})
            }
            PlacesCommand::SaveAs => {
                self.pick_kml_save_as();
                json!({})
            }
            PlacesCommand::Export(p) => {
                self.export_places(p.path.as_deref(), p.format.as_deref())?
            }
            PlacesCommand::Clear => {
                self.clear_places();
                json!({})
            }
        })
    }
}
