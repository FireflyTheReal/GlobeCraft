//! Loads the bundled Natural Earth data once and streams the border tiles.
use super::geojson::{collect_country_geometry, collect_geojson_lines, feature_name, label_center};
use super::sources::{self, FeatureSource, first_features};
use super::{CountryRegion, LabelKind, Layer, Line, MapLabel, Patch, StatusLog, log_status};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

type Repaint = Arc<dyn Fn() + Send + Sync>;
struct ActiveJob(Arc<AtomicUsize>, Repaint);
impl Drop for ActiveJob {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
        (self.1)();
    }
}

pub struct BoundaryLoader {
    active: Arc<AtomicUsize>,
    osm_tiles: super::osm_tiles::OsmTileStreamer,
}
impl BoundaryLoader {
    /// Borders and names from OpenStreetMap vector tiles (see `osm_tiles`).
    pub fn request_osm_tiles(&mut self, view: super::OsmTileView) {
        self.osm_tiles.update(view);
    }

    /// Whether border tiles are loaded (until then the bundled borders are drawn).
    pub fn has_osm_tiles(&self) -> bool {
        self.osm_tiles.has_tiles()
    }

    /// Bytes the border tile cache uses on disk.
    pub fn border_cache_usage(&self) -> u64 {
        self.osm_tiles.cache_usage()
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading_base_layers() || self.is_loading_local()
    }
    /// The bundled countries and places are still being read.
    pub fn is_loading_base_layers(&self) -> bool {
        self.active.load(Ordering::Relaxed) > 0
    }
    /// Border tiles around the view are loading.
    pub fn is_loading_local(&self) -> bool {
        self.osm_tiles.is_loading()
    }
}

/// Reads the bundled Natural Earth countries, borders and places in the background and
/// starts the border tile streamer. Patches arrive through the returned channel.
pub fn load_boundaries_async(
    log: StatusLog,
    runtime: &tokio::runtime::Handle,
    repaint: impl Fn() + Send + Sync + 'static,
) -> (mpsc::Receiver<Patch>, BoundaryLoader) {
    let (tx, rx) = mpsc::channel();
    let repaint: Repaint = Arc::new(repaint);
    let active = Arc::new(AtomicUsize::new(2));

    let (t, l) = (tx.clone(), log.clone());
    let job = ActiveJob(active.clone(), repaint.clone());
    runtime.spawn(async move {
        let _job = job;
        if let Some(p) = load_countries(&l).await {
            let _ = t.send(p);
        }
    });

    let (t, l) = (tx.clone(), log.clone());
    let job = ActiveJob(active.clone(), repaint.clone());
    runtime.spawn(async move {
        let _job = job;
        if let Some(p) = load_places(&l, &sources::place_sources()).await {
            let _ = t.send(p);
        }
    });

    let osm_tiles = super::osm_tiles::OsmTileStreamer::new(runtime.clone(), tx, log, repaint);
    (rx, BoundaryLoader { active, osm_tiles })
}

#[derive(Default)]
struct CountryOutput {
    labels: Vec<MapLabel>,
    regions: Vec<CountryRegion>,
}
fn country_iso3(feature: &serde_json::Value) -> Option<String> {
    let props = feature.get("properties")?;
    for key in ["ADM0_A3", "ISO_A3", "SOV_A3", "adm0_a3", "iso_a3", "sov_a3"] {
        if let Some(code) = props.get(key).and_then(|v| v.as_str())
            && code.len() == 3
            && code != "-99"
            && code.chars().all(|c| c.is_ascii_alphabetic())
        {
            return Some(code.to_ascii_uppercase());
        }
    }
    None
}
/// Country names and outer rings (for the country lookup); the outlines themselves are
/// not drawn, so coasts stay unoutlined.
async fn load_country_polygons(log: &StatusLog) -> Option<Patch> {
    let started = std::time::Instant::now();
    let output = first_features(
        log,
        "Countries",
        &sources::country_sources(),
        |f: &serde_json::Value, out: &mut CountryOutput| {
            let Some(geometry) = f.get("geometry") else {
                return;
            };
            let (_, rings) = collect_country_geometry(geometry);
            if let Some(iso3) = country_iso3(f)
                && let Some(region) = CountryRegion::new(iso3, rings)
            {
                out.regions.push(region);
            }
            if let (Some(en), Some((lat, lon))) = (
                feature_name(f, &["NAME", "ADMIN", "name"]),
                label_center(geometry),
            ) {
                let pt = feature_name(f, &["NAME_PT", "name_pt", "NAME", "ADMIN"])
                    .unwrap_or_else(|| en.clone());
                out.labels.push(MapLabel {
                    lat,
                    lon,
                    en,
                    pt,
                    kind: LabelKind::Country,
                    rank: 0,
                    big: false,
                    from_tiles: false,
                });
            }
        },
    )
    .await?;
    log_status(
        log,
        format!(
            "✔ Countries: {} names in {} ms",
            output.labels.len(),
            started.elapsed().as_millis()
        ),
    );
    Some(Patch {
        layer: Layer::Countries,
        lines: Vec::new(),
        labels: output.labels,
        regions: output.regions,
        osm: None,
    })
}
async fn load_places(log: &StatusLog, sources: &[FeatureSource]) -> Option<Patch> {
    let started = std::time::Instant::now();
    let labels = first_features(
        log,
        "Cities",
        sources,
        |f: &serde_json::Value, labels: &mut Vec<MapLabel>| {
            let coords = f.pointer("/geometry/coordinates");
            let (Some(coords), Some(en)) =
                (coords, feature_name(f, &["NAME", "name", "NAMEASCII"]))
            else {
                return;
            };
            let Some((lat, lon)) = crate::json_fields::coordinates(coords) else {
                return;
            };
            let pt = feature_name(f, &["NAME_PT", "name_pt"]).unwrap_or_else(|| en.clone());
            let rank = f
                .get("properties")
                .and_then(|p| p.get("SCALERANK").or_else(|| p.get("scalerank")))
                .and_then(|v| v.as_f64())
                .unwrap_or(5.0)
                .clamp(0.0, 8.0) as u8;
            labels.push(MapLabel {
                lat,
                lon,
                en,
                pt,
                kind: LabelKind::City,
                rank,
                big: false,
                from_tiles: false,
            });
        },
    )
    .await?;
    log_status(
        log,
        format!(
            "✔ Cities: {} names in {} ms",
            labels.len(),
            started.elapsed().as_millis()
        ),
    );
    Some(Patch {
        layer: Layer::Cities,
        lines: Vec::new(),
        labels,
        regions: Vec::new(),
        osm: None,
    })
}

async fn load_countries(log: &StatusLog) -> Option<Patch> {
    let mut patch = load_country_polygons(log).await?;
    // Draw shared land borders only; the polygon rings above are lookup data.
    if let Some(lines) = first_features(
        log,
        "Country borders",
        &sources::border_sources(),
        |feature: &serde_json::Value, lines: &mut Vec<Line>| {
            if let Some(coords) = feature.pointer("/geometry/coordinates") {
                collect_geojson_lines(coords, lines);
            }
        },
    )
    .await
    {
        patch.lines = lines;
    }
    Some(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_iso3_skips_natural_earth_unknown_codes() {
        let feature = serde_json::json!({
            "properties": {"ADM0_A3": "FRA", "ISO_A3": "-99"}
        });
        assert_eq!(country_iso3(&feature).as_deref(), Some("FRA"));
        let feature = serde_json::json!({"properties": {"ISO_A3": "-99"}});
        assert_eq!(country_iso3(&feature), None);
    }

    #[tokio::test]
    async fn the_bundled_countries_and_places_load_without_network() {
        let log: StatusLog = Arc::default();
        let countries = load_countries(&log).await.unwrap();
        assert!(countries.lines.len() > 100 && countries.regions.len() > 150);
        assert!(
            countries
                .labels
                .iter()
                .any(|l| l.kind == LabelKind::Country)
        );
        let places = load_places(&log, &sources::place_sources()).await.unwrap();
        assert!(places.labels.len() > 100);
    }

    #[test]
    fn finishing_or_cancelling_a_job_wakes_the_ui_after_updating_activity() {
        let active = Arc::new(AtomicUsize::new(1));
        let count = active.clone();
        let repaint = Arc::new(AtomicUsize::new(0));
        let calls = repaint.clone();
        let job = ActiveJob(
            active.clone(),
            Arc::new(move || {
                assert_eq!(count.load(Ordering::Relaxed), 0);
                calls.fetch_add(1, Ordering::Relaxed);
            }),
        );
        drop(job);
        assert_eq!(active.load(Ordering::Relaxed), 0);
        assert_eq!(repaint.load(Ordering::Relaxed), 1);
    }
}
