use super::{
    Aircraft, AircraftQuery, AircraftSnapshot, decode,
    snapshot::{self, Samples},
};
use globecraft_core::math::{haversine_km, now_unix};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

const GLOBAL_REFRESH: Duration = Duration::from_secs(17 * 60);
/// ADSB.lol's own map asks every second; every 10 s keeps planes close to where
/// they are without leaning on the service.
const LOCAL_REFRESH: Duration = Duration::from_secs(10);
const FAILURE_RETRY: Duration = Duration::from_secs(3 * 60);
const MOVE_RETRY_MIN: Duration = Duration::from_secs(10);
const MOVE_REFRESH_KM: f64 = 140.0;
const LOCAL_MAX_CAMERA_ALT_KM: f64 = 1800.0;
/// The ground radius of one ADSB.lol circle (250 nm).
const CIRCLE_KM: f64 = 463.0;
/// Circles around the camera's own sit on a fixed grid (so they stay put while the
/// camera moves): rows this far apart, circles in a row `CELL_KM` apart. Rows at
/// different latitudes do not line up, so every point is within 438 km of a centre
/// however they fall (half the diagonal of a 640 × 600 km cell).
const ROW_KM: f64 = 600.0;
const CELL_KM: f64 = 640.0;
/// At most this many circles besides the camera's, and this many asked at once:
/// about half a request a second (every 15 s each), within ADSB.lol's limits.
const MAX_CELLS: usize = 8;
const MAX_CELL_JOBS: usize = 2;
/// The circles around the camera's are asked less often than its own.
const CELL_REFRESH: Duration = Duration::from_secs(15);
const KM_PER_DEGREE: f64 = 111.195;
/// The view is sampled in this many rings of this many points to choose circles.
const COVER_RINGS: u32 = 10;
const COVER_BEARINGS: u32 = 36;
/// A circle the view left keeps its planes this long after its last answer.
const CELL_KEEP: Duration = Duration::from_secs(90);
/// The circles of a view are worked out again once the camera has been still this
/// long: while it moves or zooms they would change every frame.
const CELL_SETTLE: Duration = Duration::from_millis(800);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_and_failure_deadlines_preserve_provider_limits() {
        let now = Instant::now();
        let mut provider = Provider::default();
        assert_eq!(provider.deadline(now, GLOBAL_REFRESH), now);
        provider.attempt = Some(now);
        assert_eq!(provider.deadline(now, GLOBAL_REFRESH), now + GLOBAL_REFRESH);
        assert_eq!(provider.deadline(now, LOCAL_REFRESH), now + LOCAL_REFRESH);
        provider.samples.status.error = Some(Arc::new("fixture error".into()));
        assert_eq!(provider.deadline(now, GLOBAL_REFRESH), now + FAILURE_RETRY);
        assert_eq!(
            provider.deadline(now, LOCAL_REFRESH),
            now + LOCAL_REFRESH * 3
        );
        assert!(haversine_km((0.0, 179.8), (0.0, -179.8)) < MOVE_REFRESH_KM);
    }

    #[test]
    fn grid_circles_cover_a_wide_view_and_stay_few() {
        // Close up, the camera's own circle is enough.
        assert!(cells_for((50.0, 8.0), 300.0).is_empty());
        for center in [(50.0, 8.0), (0.0, 179.0), (-35.0, -60.0), (70.0, 20.0)] {
            // Eight circles cover a view about 600 km around (from about 770 km up);
            // beyond, the farthest planes come from OpenSky.
            let radius = 600.0;
            let cells = cells_for(center, radius);
            assert!(
                !cells.is_empty() && cells.len() <= MAX_CELLS,
                "{center:?}: {}",
                cells.len()
            );
            // Every point of the view is in the camera's circle or a grid circle.
            for ring in 1..=9 {
                for step in 0..36 {
                    let point = globecraft_core::math::destination(
                        center.0,
                        center.1,
                        f64::from(step) * 10.0,
                        radius * f64::from(ring) / 9.0,
                    );
                    let covered = haversine_km(center, point) <= CIRCLE_KM
                        || cells
                            .iter()
                            .any(|key| haversine_km(cell_center(*key), point) <= CIRCLE_KM);
                    assert!(covered, "{center:?}: {point:?} is not covered");
                }
            }
        }
        // From high up, still at most MAX_CELLS circles, the middle of the view first.
        let far = cells_for((50.0, 8.0), 3000.0);
        assert_eq!(far.len(), MAX_CELLS);
        let first = haversine_km((50.0, 8.0), cell_center(far[0]));
        assert!(first < 2.0 * CIRCLE_KM, "{first} km");
    }
}

pub(super) struct Endpoints {
    pub global: String,
    pub local: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            global: "https://opensky-network.org/api/states/all".into(),
            local: "https://api.adsb.lol/v2".into(),
        }
    }
}

#[derive(Default)]
struct Provider {
    samples: Samples,
    attempt: Option<Instant>,
    job: Option<JoinHandle<Result<Arc<[Aircraft]>, crate::Error>>>,
}
impl Provider {
    fn deadline(&self, now: Instant, interval: Duration) -> Instant {
        self.attempt.map_or(now, |last| {
            last + if self.samples.status.error.is_none() {
                interval
            } else {
                // A failed frequent source tries again sooner than a slow one.
                FAILURE_RETRY.min(interval * 3)
            }
        })
    }
    fn finish(
        &mut self,
        result: Result<Result<Arc<[Aircraft]>, crate::Error>, tokio::task::JoinError>,
    ) {
        self.job = None;
        match result.map_err(crate::Error::from).and_then(|result| result) {
            Ok(aircraft) => {
                self.samples.aircraft = aircraft;
                self.samples.status.loaded = Some(Instant::now());
                self.samples.status.error = None;
            }
            Err(error) => self.samples.status.error = Some(Arc::new(error)),
        }
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.abort();
        }
    }
}

/// A grid circle: (row, column).
type CellKey = (i32, i32);

/// A row's number of circles around the globe and their spacing in degrees: a whole
/// number, so the row closes up at ±180°.
fn row_columns(lat: f64) -> (i32, f64) {
    let around = 2.0 * std::f64::consts::PI * 6371.0 * lat.to_radians().cos().max(0.01);
    let columns = (around / CELL_KM).ceil().max(1.0) as i32;
    (columns, 360.0 / f64::from(columns))
}

fn cell_center((row, column): CellKey) -> (f64, f64) {
    let lat = (f64::from(row) * ROW_KM / KM_PER_DEGREE).clamp(-89.0, 89.0);
    let (_, step) = row_columns(lat);
    (
        lat,
        globecraft_core::math::wrap_lon(f64::from(column) * step),
    )
}

/// The grid circles to ask for a view `radius_km` around `center`: at most
/// [`MAX_CELLS`], covering the view from its middle outwards (each new circle is the
/// one covering most of what is still uncovered, starting at the uncovered point
/// nearest the middle); none while the camera's own circle covers the view.
fn cells_for(center: (f64, f64), radius_km: f64) -> Vec<CellKey> {
    if radius_km <= CIRCLE_KM * 0.8 {
        return Vec::new();
    }
    // Candidates: grid circles reaching into the view.
    let reach = radius_km + CIRCLE_KM;
    let rows = (reach / ROW_KM).ceil() as i32 + 1;
    let row0 = (center.0 * KM_PER_DEGREE / ROW_KM).round() as i32;
    let mut candidates: Vec<(CellKey, (f64, f64))> = Vec::new();
    for row in row0 - rows..=row0 + rows {
        let lat = f64::from(row) * ROW_KM / KM_PER_DEGREE;
        if lat.abs() > 89.0 {
            continue;
        }
        let (columns, step) = row_columns(lat);
        let column0 = (center.1 / step).round() as i32;
        let span = ((reach / CELL_KM) / lat.to_radians().cos().max(0.05)).ceil() as i32 + 1;
        for column in column0 - span..=column0 + span {
            // Columns past ±180° are the same circles as those on the other side.
            let key = (row, column.rem_euclid(columns));
            let at = cell_center(key);
            if haversine_km(center, at) < reach && !candidates.iter().any(|(k, _)| *k == key) {
                candidates.push((key, at));
            }
        }
    }
    // Points of the view, middle first, that the camera's own circle misses.
    let mut uncovered: Vec<(f64, f64)> = (1..=COVER_RINGS)
        .flat_map(|ring| {
            let distance = radius_km * f64::from(ring) / f64::from(COVER_RINGS);
            (0..COVER_BEARINGS).map(move |step| {
                let bearing = f64::from(step) * 360.0 / f64::from(COVER_BEARINGS);
                globecraft_core::math::destination(center.0, center.1, bearing, distance)
            })
        })
        .filter(|point| haversine_km(center, *point) > CIRCLE_KM)
        .collect();
    let mut chosen = Vec::new();
    while chosen.len() < MAX_CELLS {
        let Some(first) = uncovered.first().copied() else {
            break;
        };
        let covers = |at: (f64, f64), point: (f64, f64)| haversine_km(at, point) <= CIRCLE_KM;
        let best = candidates
            .iter()
            .filter(|(_, at)| covers(*at, first))
            .max_by_key(|(_, at)| {
                uncovered
                    .iter()
                    .filter(|point| covers(*at, **point))
                    .count()
            });
        let Some(&(key, at)) = best else {
            // No grid circle reaches this point (it cannot happen on this grid).
            uncovered.remove(0);
            continue;
        };
        chosen.push(key);
        candidates.retain(|(k, _)| *k != key);
        uncovered.retain(|point| !covers(at, *point));
    }
    chosen
}

/// A grid circle's last answer: when, and its aircraft (`None` if it failed with
/// nothing to keep).
struct CellSample {
    attempt: Instant,
    aircraft: Option<Arc<[Aircraft]>>,
}

/// A finished grid circle download.
type CellDone = (CellKey, Result<Arc<[Aircraft]>, crate::Error>);

/// The grid circles' downloads and their latest aircraft.
#[derive(Default)]
struct Cells {
    wanted: Vec<CellKey>,
    samples: std::collections::HashMap<CellKey, CellSample>,
    running: std::collections::HashMap<CellKey, JoinHandle<()>>,
}
impl Cells {
    /// Sets the circles of this view. Circles it no longer covers keep their planes
    /// until [`CELL_KEEP`] after their last answer (a pan back finds them), and
    /// downloads already running finish; returns whether any planes were dropped.
    fn want(&mut self, wanted: Vec<CellKey>, now: Instant) -> bool {
        self.wanted = wanted;
        let before = self.samples.len();
        let wanted = &self.wanted;
        self.samples.retain(|key, sample| {
            wanted.contains(key) || now.saturating_duration_since(sample.attempt) < CELL_KEEP
        });
        self.samples.len() != before
    }
    /// Starts the downloads due; returns when the next one is due.
    fn start(
        &mut self,
        now: Instant,
        url: &str,
        done: &mpsc::UnboundedSender<CellDone>,
    ) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        for key in self.wanted.clone() {
            if self.running.contains_key(&key) {
                continue;
            }
            let due = self.samples.get(&key).map_or(now, |sample| {
                sample.attempt
                    + if sample.aircraft.is_some() {
                        CELL_REFRESH
                    } else {
                        CELL_REFRESH * 3
                    }
            });
            if due > now {
                next = Some(next.map_or(due, |old| old.min(due)));
                continue;
            }
            if self.running.len() >= MAX_CELL_JOBS {
                continue;
            }
            let (lat, lon) = cell_center(key);
            let (url, done) = (url.to_owned(), done.clone());
            self.running.insert(
                key,
                tokio::spawn(async move {
                    let _ = done.send((key, decode::download_local(url, lat, lon).await));
                }),
            );
        }
        next
    }
    fn finish(&mut self, key: CellKey, result: Result<Arc<[Aircraft]>, crate::Error>) -> bool {
        if self.running.remove(&key).is_none() {
            return false;
        }
        let aircraft = match result {
            Ok(aircraft) => Some(aircraft),
            // A failed circle keeps showing its last aircraft until they expire.
            Err(_) => self
                .samples
                .remove(&key)
                .and_then(|old| old.aircraft)
                .filter(|old| !old.is_empty()),
        };
        self.samples.insert(
            key,
            CellSample {
                attempt: Instant::now(),
                aircraft,
            },
        );
        true
    }
    fn aircraft(&self) -> impl Iterator<Item = &Aircraft> {
        self.samples
            .values()
            .filter_map(|sample| sample.aircraft.as_deref())
            .flatten()
    }
}
impl Drop for Cells {
    fn drop(&mut self) {
        for job in self.running.values() {
            job.abort();
        }
    }
}

async fn receive(
    provider: &mut Provider,
) -> Result<Result<Arc<[Aircraft]>, crate::Error>, tokio::task::JoinError> {
    match &mut provider.job {
        Some(job) => job.await,
        None => std::future::pending().await,
    }
}

pub(super) async fn run(
    mut queries: watch::Receiver<AircraftQuery>,
    publish: watch::Sender<Arc<AircraftSnapshot>>,
    repaint: impl Fn() + Send + Sync + 'static,
    endpoints: Endpoints,
) {
    let mut global = Provider::default();
    let mut local = Provider::default();
    let mut cells = Cells::default();
    let mut query_changed = Instant::now();
    let (cell_done, mut cell_results) = mpsc::unbounded_channel();
    let mut local_center = None;
    let mut query = *queries.borrow_and_update();
    let mut dirty = false;
    let mut expires = None;
    loop {
        let now = Instant::now();
        let mut deadline = if query.enabled { expires } else { None };
        if query.enabled {
            if global.job.is_none() {
                let due = global.deadline(now, GLOBAL_REFRESH);
                if now >= due {
                    global.attempt = Some(now);
                    global.samples.status.attempted = true;
                    global.job = Some(tokio::spawn(decode::download_global(
                        endpoints.global.clone(),
                    )));
                    dirty = true;
                } else {
                    deadline = Some(deadline.map_or(due, |old: Instant| old.min(due)));
                }
            }
            if query.altitude_km <= LOCAL_MAX_CAMERA_ALT_KM && local.job.is_none() {
                let moved = local_center.is_some_and(|center| {
                    haversine_km(center, (query.latitude, query.longitude)) > MOVE_REFRESH_KM
                });
                let movement = local.attempt.map_or(now, |last| last + MOVE_RETRY_MIN);
                let due = if moved {
                    local.deadline(now, LOCAL_REFRESH).min(movement)
                } else {
                    local.deadline(now, LOCAL_REFRESH)
                };
                if now >= due {
                    local.attempt = Some(now);
                    local.samples.status.attempted = true;
                    local_center = Some((query.latitude, query.longitude));
                    local.job = Some(tokio::spawn(decode::download_local(
                        endpoints.local.clone(),
                        query.latitude,
                        query.longitude,
                    )));
                    dirty = true;
                } else {
                    deadline = Some(deadline.map_or(due, |old| old.min(due)));
                }
            }
            let settled = query_changed + CELL_SETTLE;
            let wanted = if query.altitude_km > LOCAL_MAX_CAMERA_ALT_KM {
                Vec::new()
            } else if now >= settled {
                cells_for((query.latitude, query.longitude), query.radius_km)
            } else {
                deadline = Some(deadline.map_or(settled, |old| old.min(settled)));
                cells.wanted.clone()
            };
            dirty |= cells.want(wanted, now);
            if let Some(due) = cells.start(now, &endpoints.local, &cell_done) {
                deadline = Some(deadline.map_or(due, |old| old.min(due)));
            }
        } else {
            dirty |= cells.want(Vec::new(), now);
        }
        if dirty || (query.enabled && expires.is_some_and(|time| now >= time)) {
            let global_input = global.samples.clone();
            let mut local_input = local.samples.clone();
            if !cells.samples.is_empty() {
                // The grid circles count as the close-up source too.
                let mut aircraft = local_input.aircraft.to_vec();
                aircraft.extend(cells.aircraft().cloned());
                local_input.aircraft = aircraft.into();
                let newest = cells.samples.values().map(|sample| sample.attempt).max();
                local_input.status.loaded = local_input.status.loaded.max(newest);
            }
            match crate::cpu::run(crate::cpu::Service::Features, move || {
                Ok(snapshot::prepare(
                    global_input,
                    local_input,
                    Instant::now(),
                    now_unix(),
                ))
            })
            .await
            {
                Ok(prepared) => {
                    expires = prepared.expires;
                    publish.send_replace(prepared.snapshot);
                }
                Err(error) => {
                    let old = publish.borrow().clone();
                    publish.send_replace(Arc::new(AircraftSnapshot {
                        aircraft: old.aircraft.clone(),
                        global: global.samples.status.clone(),
                        local: local.samples.status.clone(),
                        error: Some(Arc::new(error)),
                        valid_until: old.valid_until,
                    }));
                    expires = Some(Instant::now() + FAILURE_RETRY);
                }
            }
            repaint();
            dirty = false;
            continue;
        }
        let wait = async {
            match deadline {
                Some(time) => tokio::time::sleep_until(time.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            changed = queries.changed() => {
                if changed.is_err() { return; }
                let next = *queries.borrow_and_update();
                dirty |= next.enabled && !query.enabled;
                query = next;
                query_changed = Instant::now();
            }
            result = receive(&mut global) => { global.finish(result); dirty = true; }
            result = receive(&mut local) => { local.finish(result); dirty = true; }
            Some((key, result)) = cell_results.recv() => { dirty |= cells.finish(key, result); }
            () = wait => {}
        }
    }
}
