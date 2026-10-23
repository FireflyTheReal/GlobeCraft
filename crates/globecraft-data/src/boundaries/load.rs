use super::brazil::load_brazil;
use super::geojson::{collect_country_geometry, collect_geojson_lines, feature_name, label_center};
use super::lod::DetailLevel;
use super::municipal::{MunicipalStreamer, MunicipalView};
use super::sources::{self, FeatureSource, first_features};
use super::states::{self, StateStage};
use super::{
    CountryRegion, LabelKind, Layer, Line, MapLabel, Patch, StatusLog, is_big_country, log_status,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    mpsc,
};
/// A state stage whose sources all failed is retried after this delay.
const STATE_RETRY_SECS: u64 = 60;

type Repaint = Arc<dyn Fn() + Send + Sync>;
struct ActiveJob(Arc<AtomicUsize>, Repaint);
impl Drop for ActiveJob {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
        (self.1)();
    }
}
/// Clears the "detail job running" flag when the job finishes, however it ends.
struct RunningFlag(Arc<AtomicBool>);
impl Drop for RunningFlag {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Default)]
struct BrazilRequests {
    labels: bool,
    geometry: bool,
}
pub struct BoundaryLoader {
    runtime: tokio::runtime::Handle,
    tx: mpsc::Sender<Patch>,
    log: StatusLog,
    active: Arc<AtomicUsize>,
    municipal: MunicipalStreamer,
    places: super::places::PlaceStreamer,
    requests: BrazilRequests,
    /// Highest round-detail level already applied to the world layers.
    world_detail: Arc<AtomicU8>,
    /// The world level the camera currently wants; the running job loads
    /// this one, in either direction.
    world_target: Arc<AtomicU8>,
    world_running: Arc<AtomicBool>,
    /// State/province data already applied, the stage the camera wants, and
    /// whether a state job is running (same coalescing as the world layers).
    state_applied: Arc<AtomicU8>,
    state_target: Arc<AtomicU8>,
    state_running: Arc<AtomicBool>,
    state_failed_at: Arc<std::sync::Mutex<Option<std::time::Instant>>>,

    repaint: Repaint,
}
impl BoundaryLoader {
    /// Follow the camera's world-detail level in both directions: rising
    /// toward the surface upgrades, falling back replaces the world layers
    /// with the coarser level once the camera has cleared its hysteresis
    /// margin. Repeated requests coalesce into the stored target; the
    /// running job always loads the latest level.
    pub fn request_world_detail(&mut self, altitude_km: f64) {
        let applied = self.world_detail.load(Ordering::Acquire);
        let wanted =
            DetailLevel::with_downgrade_hysteresis(altitude_km, DetailLevel::from_code(applied))
                as u8;
        if wanted == applied {
            return;
        }
        self.world_target.store(wanted, Ordering::Release);
        if self.world_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let (log, tx) = (self.log.clone(), self.tx.clone());
        let active = self.active.clone();
        let repaint = self.repaint.clone();
        let target = self.world_target.clone();
        let detail = self.world_detail.clone();
        let running = self.world_running.clone();
        active.fetch_add(1, Ordering::Relaxed);
        self.runtime.spawn(async move {
            let _job = ActiveJob(active, repaint);
            let _running = RunningFlag(running);
            loop {
                let level = DetailLevel::from_code(target.load(Ordering::Acquire));
                if level as u8 == detail.load(Ordering::Acquire) {
                    break;
                }
                if let Some(p) = load_countries(&log, level).await {
                    let _ = tx.send(p);
                }
                if let Some(p) = load_places(&log, &sources::place_sources(level)).await {
                    let _ = tx.send(p);
                }
                detail.store(level as u8, Ordering::Release);
            }
        });
    }

    /// Load state/province data ahead of where it becomes visible. The stage only
    /// rises: lines above their altitude are not drawn, so keeping them costs no
    /// frame time, and a later descent finds them ready.
    pub fn request_states(&mut self, altitude_km: f64, state_visible_max_km: f64) {
        let wanted = StateStage::wanted(altitude_km, state_visible_max_km);
        let applied = StateStage::from_code(self.state_applied.load(Ordering::Acquire));
        if wanted <= applied {
            return;
        }
        // A failed stage waits before retrying instead of refetching every frame.
        if self
            .state_failed_at
            .lock()
            .ok()
            .and_then(|failed| *failed)
            .is_some_and(|at| at.elapsed().as_secs() < STATE_RETRY_SECS)
        {
            return;
        }
        self.state_target.fetch_max(wanted as u8, Ordering::AcqRel);
        if self.state_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let (log, tx) = (self.log.clone(), self.tx.clone());
        let active = self.active.clone();
        let repaint = self.repaint.clone();
        let target = self.state_target.clone();
        let applied = self.state_applied.clone();
        let running = self.state_running.clone();
        let failed_at = self.state_failed_at.clone();
        active.fetch_add(1, Ordering::Relaxed);
        self.runtime.spawn(async move {
            let _job = ActiveJob(active, repaint);
            let _running = RunningFlag(running);
            loop {
                let stage = StateStage::from_code(target.load(Ordering::Acquire));
                if stage as u8 <= applied.load(Ordering::Acquire) {
                    break;
                }
                match states::load_stage(&log, stage).await {
                    Some(patch) => {
                        let _ = tx.send(patch);
                        applied.fetch_max(stage as u8, Ordering::AcqRel);
                    }
                    // Leave the stage unapplied; the next request retries it later.
                    None => {
                        if let Ok(mut failed) = failed_at.lock() {
                            *failed = Some(std::time::Instant::now());
                        }
                        break;
                    }
                }
            }
        });
    }

    fn take_request(&mut self, cities: bool, labels: bool, altitude_km: f64) -> Option<bool> {
        let geometry = cities;
        let labels = labels && altitude_km < super::OTHER_CITY_MAX_ALTITUDE_KM;
        if geometry {
            if self.requests.geometry {
                return None;
            }
            self.requests.geometry = true;
            self.requests.labels = true;
        } else if labels {
            if self.requests.labels {
                return None;
            }
            self.requests.labels = true;
        } else {
            return None;
        }
        Some(geometry)
    }
    /// Municipal outlines around the view, streamed by area (see `municipal`).
    pub fn request_municipal(&mut self, view: MunicipalView<'_>, boundaries: &super::BoundaryData) {
        let borders = super::CountryBorderLines {
            lines: &boundaries.countries,
            pairs: &boundaries.country_pairs,
            land: &boundaries.country_regions,
        };
        self.municipal.update(view, borders, |lat, lon| {
            boundaries.country_at(lat, lon).map(str::to_owned)
        });
    }

    /// Town, village and island names around the view (see `places`).
    pub fn request_places(&mut self, view: super::PlaceView) {
        self.places.update(view);
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading_base_layers() || self.is_loading_local()
    }
    pub fn is_loading_base_layers(&self) -> bool {
        self.active.load(Ordering::Relaxed) > 0
    }
    pub fn is_loading_local(&self) -> bool {
        self.municipal.is_loading()
    }
    pub fn request_visible_layers(&mut self, cities: bool, labels: bool, altitude_km: f64) {
        let Some(geometry) = self.take_request(cities, labels, altitude_km) else {
            return;
        };
        let (log, tx) = (self.log.clone(), self.tx.clone());
        let active = self.active.clone();
        active.fetch_add(1, Ordering::Relaxed);
        let job = ActiveJob(active, self.repaint.clone());
        self.runtime.spawn(async move {
            let _job = job;
            load_brazil(&log, &tx, geometry).await;
        });
    }
}
// World layers start at the bundled low detail, then rise with the camera.
// Background async tasks deliver each completed layer through the application channel.
pub fn load_boundaries_async(
    log: StatusLog,
    runtime: &tokio::runtime::Handle,
    repaint: impl Fn() + Send + Sync + 'static,
) -> (mpsc::Receiver<Patch>, BoundaryLoader) {
    let (tx, rx) = mpsc::channel();
    let repaint: Repaint = Arc::new(repaint);
    let active = Arc::new(AtomicUsize::new(2));

    // The 1:110m set is compiled into the binary, so a cold start needs no network.
    let (t, l) = (tx.clone(), log.clone());
    let job = ActiveJob(active.clone(), repaint.clone());
    runtime.spawn(async move {
        let _job = job;
        if let Some(p) = load_countries(&l, DetailLevel::Low).await {
            let _ = t.send(p);
        }
    });

    let (t, l) = (tx.clone(), log.clone());
    let job = ActiveJob(active.clone(), repaint.clone());
    runtime.spawn(async move {
        let _job = job;
        if let Some(p) = load_places(&l, &sources::place_sources(DetailLevel::Low)).await {
            let _ = t.send(p);
        }
    });

    let municipal =
        MunicipalStreamer::new(runtime.clone(), tx.clone(), log.clone(), repaint.clone());
    let places = super::places::PlaceStreamer::new(
        runtime.clone(),
        tx.clone(),
        log.clone(),
        repaint.clone(),
    );
    (
        rx,
        BoundaryLoader {
            runtime: runtime.clone(),
            tx,
            log,
            active,
            municipal,
            places,
            requests: BrazilRequests::default(),
            world_detail: Arc::new(AtomicU8::new(DetailLevel::Low as u8)),
            world_target: Arc::new(AtomicU8::new(DetailLevel::Low as u8)),
            world_running: Arc::new(AtomicBool::new(false)),
            state_applied: Arc::new(AtomicU8::new(StateStage::None as u8)),
            state_target: Arc::new(AtomicU8::new(StateStage::None as u8)),
            state_running: Arc::new(AtomicBool::new(false)),
            state_failed_at: Arc::default(),
            repaint,
        },
    )
}

#[derive(Clone, Copy)]
struct PolygonLayer {
    what: &'static str,
    layer: Layer,
    kind: LabelKind,
    en_keys: &'static [&'static str],
    pt_keys: &'static [&'static str],
    rank: u8,
}
#[derive(Default)]
struct PolygonOutput {
    lines: Vec<Line>,
    lines_big: Vec<Line>,
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
async fn load_polygon_layer(
    log: &StatusLog,
    sources: &[FeatureSource],
    options: PolygonLayer,
) -> Option<Patch> {
    let PolygonLayer {
        what,
        layer,
        kind,
        en_keys,
        pt_keys,
        rank,
    } = options;
    let started = std::time::Instant::now();
    let output = first_features(
        log,
        what,
        sources,
        move |f: &serde_json::Value, out: &mut PolygonOutput| {
            let Some(geometry) = f.get("geometry") else {
                return;
            };
            let big = layer == Layer::States && is_big_country(f);
            if layer == Layer::Countries {
                // Only the outer rings are kept here (for country lookup). The
                // country outline itself comes from the land-border linework so
                // coastlines stay unoutlined.
                let (_, rings) = collect_country_geometry(geometry);
                if let Some(iso3) = country_iso3(f)
                    && let Some(region) = CountryRegion::new(iso3, rings)
                {
                    out.regions.push(region);
                }
            } else if let Some(coords) = geometry.get("coordinates") {
                collect_geojson_lines(
                    coords,
                    if big {
                        &mut out.lines_big
                    } else {
                        &mut out.lines
                    },
                );
            }
            if let (Some(en), Some((lat, lon))) = (feature_name(f, en_keys), label_center(geometry))
            {
                let pt = feature_name(f, pt_keys).unwrap_or_else(|| en.clone());
                out.labels.push(MapLabel {
                    lat,
                    lon,
                    en,
                    pt,
                    kind,
                    rank,
                    big,
                });
            }
        },
    )
    .await?;
    log_status(
        log,
        if layer == Layer::Countries {
            format!(
                "✔ {what}: {} names in {} ms",
                output.labels.len(),
                started.elapsed().as_millis()
            )
        } else {
            format!(
                "✔ {what}: {} lines, {} names in {} ms",
                output.lines.len() + output.lines_big.len(),
                output.labels.len(),
                started.elapsed().as_millis()
            )
        },
    );
    Some(Patch {
        layer,
        pairs: Vec::new(),
        disputed: Vec::new(),
        lines: output.lines,
        lines_big: output.lines_big,
        labels: output.labels,
        regions: output.regions,
        replace: false,
        municipal: None,
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
        pairs: Vec::new(),
        disputed: Vec::new(),
        lines: vec![],
        lines_big: vec![],
        labels,
        regions: vec![],
        replace: true,
        municipal: None,
    })
}

fn disputed_type(feature: &serde_json::Value) -> bool {
    matches!(
        map_unit_type(feature),
        Some("Disputed" | "Indeterminate" | "Lease" | "Overlay")
    )
}
fn map_unit_type(feature: &serde_json::Value) -> Option<&str> {
    feature
        .get("properties")
        .and_then(|p| p.get("TYPE").or_else(|| p.get("type")))
        .and_then(|v| v.as_str())
}
/// Whether a Natural Earth border line or map unit is disputed: a disputed line, a line
/// of control, an indeterminate frontier or an unrecognised one, or the outline of a
/// disputed or indeterminate territory. Leases, overlays and undemarcated ("indefinite")
/// lines are not.
fn is_disputed(feature: &serde_json::Value) -> bool {
    let class = feature
        .pointer("/properties/FEATURECLA")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    class.starts_with("Disputed")
        || class.starts_with("Line of control")
        || class.starts_with("Indeterminant")
        || class.starts_with("Unrecognized")
        || matches!(map_unit_type(feature), Some("Disputed" | "Indeterminate"))
}

async fn load_disputed(log: &StatusLog, sources: &[FeatureSource]) -> Option<Patch> {
    let started = std::time::Instant::now();
    let (lines, disputed) = first_features(
        log,
        "Disputed boundaries",
        sources,
        |f: &serde_json::Value, (out, disputed): &mut (Vec<Line>, Vec<bool>)| {
            if disputed_type(f)
                && let Some(coords) = f.pointer("/geometry/coordinates")
            {
                collect_geojson_lines(coords, out);
                disputed.resize(out.len(), is_disputed(f));
            }
        },
    )
    .await?;
    log_status(
        log,
        format!(
            "✔ Disputed boundaries: {} lines in {} ms",
            lines.len(),
            started.elapsed().as_millis()
        ),
    );
    Some(Patch {
        layer: Layer::Countries,
        pairs: Vec::new(),
        disputed,
        lines,
        lines_big: vec![],
        labels: vec![],
        regions: vec![],
        replace: false,
        municipal: None,
    })
}

async fn load_countries(log: &StatusLog, level: DetailLevel) -> Option<Patch> {
    let mut patch = load_polygon_layer(
        log,
        &sources::country_sources(level),
        PolygonLayer {
            what: "Countries",
            layer: Layer::Countries,
            kind: LabelKind::Country,
            en_keys: &["NAME", "ADMIN", "name"],
            pt_keys: &["NAME_PT", "name_pt", "NAME", "ADMIN"],
            rank: 0,
        },
    )
    .await?;
    patch.replace = true;
    // Draw shared land borders only; the polygon rings above are lookup data.
    if let Some((lines, pairs, disputed)) = load_border_lines(log, level).await {
        patch.lines = lines;
        patch.pairs = pairs;
        patch.disputed = disputed;
    }
    // A detail upgrade replaces the previous outlines, so disputed linework must
    // travel in the same patch or it would be discarded with them.
    let disputed = sources::map_unit_sources(level);
    if !disputed.is_empty()
        && let Some(more) = load_disputed(log, &disputed).await
    {
        // Flags stay aligned with the lines: the border lines' first, then these.
        patch.pairs.resize(patch.lines.len(), None);
        patch.disputed.resize(patch.lines.len(), false);
        patch.lines.extend(more.lines);
        patch.disputed.extend(more.disputed);
    }
    Some(patch)
}

/// The two countries a Natural Earth border line separates.
fn border_pair(feature: &serde_json::Value) -> Option<super::CountryPair> {
    let props = feature.get("properties")?;
    let code = |key: &str| -> Option<[u8; 3]> {
        let code = props.get(key)?.as_str()?;
        (code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic()))
            .then(|| code.to_ascii_uppercase().as_bytes().try_into().ok())?
    };
    Some([code("ADM0_A3_L")?, code("ADM0_A3_R")?])
}

/// Land border lines with the pair of countries each separates, and whether each is
/// disputed.
type BorderLines = (Vec<Line>, Vec<Option<super::CountryPair>>, Vec<bool>);
async fn load_border_lines(log: &StatusLog, level: DetailLevel) -> Option<BorderLines> {
    let started = std::time::Instant::now();
    let (lines, pairs, disputed) = first_features(
        log,
        "Country borders",
        &sources::border_sources(level),
        |feature: &serde_json::Value, (lines, pairs, disputed): &mut BorderLines| {
            if let Some(coords) = feature
                .get("geometry")
                .and_then(|geometry| geometry.get("coordinates"))
            {
                collect_geojson_lines(coords, lines);
                pairs.resize(lines.len(), border_pair(feature));
                disputed.resize(lines.len(), is_disputed(feature));
            }
        },
    )
    .await?;
    log_status(
        log,
        format!(
            "✔ Country borders: {} lines in {} ms",
            lines.len(),
            started.elapsed().as_millis()
        ),
    );
    Some((lines, pairs, disputed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_low_detail_is_the_cold_start_path() {
        let countries = sources::country_sources(DetailLevel::Low);
        assert!(matches!(countries.as_slice(), [FeatureSource::Bundled(_)]));
        assert!(!countries.iter().any(|s| matches!(s, FeatureSource::Url(_))));
    }

    #[test]
    fn border_lines_name_the_two_countries() {
        let feature = serde_json::json!({"properties": {"ADM0_A3_L": "DEU", "ADM0_A3_R": "CZE"}});
        assert_eq!(border_pair(&feature), Some([*b"DEU", *b"CZE"]));
        let feature = serde_json::json!({"properties": {"ADM0_A3_L": "-99", "ADM0_A3_R": "CZE"}});
        assert_eq!(border_pair(&feature), None);
    }

    #[test]
    fn disputed_lines_and_territories_are_marked() {
        let line = |class: &str| serde_json::json!({"properties": {"FEATURECLA": class}});
        for class in [
            "Disputed (please verify)",
            "Line of control (please verify)",
            "Indeterminant frontier",
            "Unrecognized",
        ] {
            assert!(is_disputed(&line(class)), "{class}");
        }
        for class in [
            "International boundary (verify)",
            "Indefinite (please verify)",
            "Lease limit",
        ] {
            assert!(!is_disputed(&line(class)), "{class}");
        }
        let unit = |kind: &str| serde_json::json!({"properties": {"TYPE": kind}});
        assert!(is_disputed(&unit("Disputed")) && is_disputed(&unit("Indeterminate")));
        assert!(!is_disputed(&unit("Lease")) && !is_disputed(&unit("Overlay")));
    }

    #[test]
    fn country_iso3_skips_natural_earth_unknown_codes() {
        let feature = serde_json::json!({
            "properties": {"ADM0_A3": "FRA", "ISO_A3": "-99"}
        });
        assert_eq!(country_iso3(&feature).as_deref(), Some("FRA"));
        let feature = serde_json::json!({"properties": {"ISO_A3": "-99"}});
        assert_eq!(country_iso3(&feature), None);
    }

    #[test]
    fn disputed_map_units_are_limited_to_political_overlays() {
        for kind in ["Disputed", "Indeterminate", "Lease", "Overlay"] {
            assert!(disputed_type(
                &serde_json::json!({"properties": {"TYPE": kind}})
            ));
        }
        assert!(!disputed_type(
            &serde_json::json!({"properties": {"TYPE": "Sovereign country"}})
        ));
    }

    #[tokio::test]
    async fn municipal_layers_start_only_when_needed_and_upgrade_once() {
        let (tx, _rx) = mpsc::channel();
        let log: StatusLog = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut loader = BoundaryLoader {
            runtime: tokio::runtime::Handle::current(),
            municipal: MunicipalStreamer::new(
                tokio::runtime::Handle::current(),
                tx.clone(),
                log.clone(),
                Arc::new(|| {}),
            ),
            places: crate::boundaries::places::PlaceStreamer::new(
                tokio::runtime::Handle::current(),
                tx.clone(),
                log.clone(),
                Arc::new(|| {}),
            ),
            tx,
            log,
            active: Arc::new(AtomicUsize::new(0)),
            requests: BrazilRequests::default(),
            world_detail: Arc::new(AtomicU8::new(DetailLevel::Low as u8)),
            world_target: Arc::new(AtomicU8::new(DetailLevel::Low as u8)),
            world_running: Arc::new(AtomicBool::new(false)),
            state_applied: Arc::new(AtomicU8::new(0)),
            state_target: Arc::new(AtomicU8::new(0)),
            state_running: Arc::new(AtomicBool::new(false)),
            state_failed_at: Arc::default(),
            repaint: Arc::new(|| {}),
        };
        assert_eq!(loader.take_request(false, true, 14000.0), None);
        assert_eq!(loader.take_request(false, true, 300.0), None);
        assert_eq!(loader.take_request(false, true, 299.0), Some(false));
        assert_eq!(loader.take_request(false, true, 299.0), None);
        assert_eq!(loader.take_request(true, true, 14000.0), Some(true));
        assert_eq!(loader.take_request(false, true, 100.0), None);
        assert!(!loader.is_loading());
        assert!(!loader.is_loading_local());
        assert!(!loader.is_loading_base_layers());
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
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
