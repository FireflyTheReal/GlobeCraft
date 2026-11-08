//! Names of the oceans, seas, exclusive economic zones and undersea features: the
//! Marine Regions gazetteer (Flanders Marine Institute, CC BY 4.0) for the surface,
//! the IHO-IOC GEBCO Gazetteer of Undersea Feature Names for the seafloor, published
//! into the `map-data` release by `.github/workflows/marine-names.yml`: one point per
//! place with its size, largest first.
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const MAX_BYTES: u64 = 40_000_000;
const RETRY: Duration = Duration::from_secs(10 * 60);

/// What a named place is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarineKind {
    Ocean,
    /// Seas, sea areas, gulfs, straits, channels, bays.
    Sea,
    /// An exclusive economic zone.
    Eez,
    /// A seamount, trench, ridge, plain, canyon…
    Undersea,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarineName {
    pub name: String,
    pub kind: MarineKind,
    pub lat: f64,
    pub lon: f64,
    /// About how far across it is.
    pub size_km: f64,
}

/// Every name, largest first.
#[derive(Debug, Default)]
pub struct MarineNames {
    pub names: Vec<MarineName>,
}

impl MarineNames {
    pub fn parse(json: &serde_json::Value) -> Result<Self, crate::Error> {
        let features = json
            .get("features")
            .and_then(serde_json::Value::as_array)
            .ok_or("marine names: no features")?;
        let mut names: Vec<MarineName> = features
            .iter()
            .filter_map(|feature| {
                let properties = feature.get("properties")?;
                let coordinates = feature.get("geometry")?.get("coordinates")?.as_array()?;
                let kind = match properties.get("kind")?.as_str()? {
                    "ocean" => MarineKind::Ocean,
                    "sea" => MarineKind::Sea,
                    "eez" => MarineKind::Eez,
                    "undersea" => MarineKind::Undersea,
                    _ => return None,
                };
                Some(MarineName {
                    name: properties.get("name")?.as_str()?.trim().to_owned(),
                    kind,
                    lon: coordinates.first()?.as_f64()?,
                    lat: coordinates.get(1)?.as_f64()?,
                    size_km: properties
                        .get("size_km")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0),
                })
            })
            .filter(|name| !name.name.is_empty() && name.lat.abs() <= 90.0)
            .collect();
        names.sort_by(|a, b| b.size_km.total_cmp(&a.size_km));
        Ok(Self { names })
    }

    pub fn from_gzip(bytes: &[u8]) -> Result<Self, crate::Error> {
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_BYTES)
            .read_to_end(&mut text)?;
        Self::parse(&serde_json::from_slice(&text)?)
    }
}

/// The names, loaded once when the layer is on.
#[derive(Default)]
pub struct LiveMarineNames {
    names: Option<Arc<MarineNames>>,
    attempted: Option<Instant>,
    error: Option<crate::Error>,
    rx: Option<mpsc::Receiver<Result<MarineNames, crate::Error>>>,
}

impl LiveMarineNames {
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        wanted: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(names)) => {
                    self.names = Some(Arc::new(names));
                    self.error = None;
                    self.rx = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if wanted
            && self.names.is_none()
            && self.rx.is_none()
            && self.attempted.is_none_or(|t| t.elapsed() >= RETRY)
        {
            let (tx, rx) = mpsc::channel();
            self.rx = Some(rx);
            self.attempted = Some(Instant::now());
            runtime.spawn(async move {
                let _ = tx.send(download().await);
                repaint();
            });
        }
    }

    pub fn names(&self) -> Option<Arc<MarineNames>> {
        self.names.clone()
    }

    /// Why nothing shows, when the download failed.
    pub fn error(&self) -> Option<String> {
        self.error.as_ref().map(ToString::to_string)
    }

    /// Replaces the names, as a finished download does (tests and screenshots).
    pub fn set(&mut self, names: MarineNames) {
        self.names = Some(Arc::new(names));
        self.error = None;
    }
}

async fn download() -> Result<MarineNames, crate::Error> {
    let url = crate::map_data::file("marine_names").await?;
    let client = crate::http::client(crate::http::Policy::Boundary)?;
    let bytes = crate::http::body(client.get(&url), MAX_BYTES).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || {
        MarineNames::from_gzip(&bytes)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_read_by_kind_and_sorted_largest_first() {
        let names = MarineNames::parse(&serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [142.2, 11.35]},
                 "properties": {"name": "Mariana Trench", "kind": "undersea", "size_km": 2500.0}},
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [-149.6, -0.9]},
                 "properties": {"name": "Pacific Ocean", "kind": "ocean", "size_km": 19000.0}},
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [2.7, 51.5]},
                 "properties": {"name": "  ", "kind": "eez", "size_km": 100.0}},
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
                 "properties": {"name": "Somewhere", "kind": "lake"}}
            ]
        }))
        .unwrap();
        let read: Vec<_> = names
            .names
            .iter()
            .map(|n| (n.name.as_str(), n.kind))
            .collect();
        assert_eq!(
            read,
            [
                ("Pacific Ocean", MarineKind::Ocean),
                ("Mariana Trench", MarineKind::Undersea)
            ]
        );
        assert_eq!((names.names[1].lat, names.names[1].lon), (11.35, 142.2));
    }
}
