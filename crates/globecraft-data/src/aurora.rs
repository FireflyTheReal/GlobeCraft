//! The aurora forecast (roadmap 5, `HAZ-01`): NOAA SWPC's OVATION model, the chance of
//! seeing aurora overhead on a 1° grid for the next 30 to 90 minutes, refreshed while
//! the Aurora layer is on. Public domain (U.S. Government work).
use globecraft_core::math::now_unix;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

const URL: &str = "https://services.swpc.noaa.gov/json/ovation_aurora_latest.json";
/// SWPC renews the forecast about every five minutes.
const REFRESH: Duration = Duration::from_secs(10 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
/// The file is about 1 MB.
const MAX_BYTES: u64 = 4_000_000;
/// Grid nodes: every whole degree of longitude (0–359) and latitude (−90–90).
pub const LONGITUDES: usize = 360;
pub const LATITUDES: usize = 181;

/// One forecast: the chance of aurora (0–100 %) at every grid node.
#[derive(Clone, Debug, PartialEq)]
pub struct AuroraGrid {
    /// When the solar wind it is based on was measured, and the moment it is for
    /// (seconds since 1970).
    pub observed: f64,
    pub forecast: f64,
    values: Box<[u8]>,
}

impl AuroraGrid {
    /// The chance (%) at longitude index `lon` (0 = 0°, wrapping) and latitude index
    /// `lat` (0 = −90°).
    pub fn value(&self, lon: usize, lat: usize) -> u8 {
        self.values[(lon % LONGITUDES) * LATITUDES + lat.min(LATITUDES - 1)]
    }

    /// The highest chance anywhere.
    pub fn peak(&self) -> u8 {
        self.values.iter().copied().max().unwrap_or(0)
    }

    /// Reads SWPC's JSON: `"Data Format": "[Longitude, Latitude, Aurora]"` rows.
    pub fn parse(json: &serde_json::Value) -> Result<Self, crate::Error> {
        let time = |key: &str| {
            json.get(key)
                .and_then(serde_json::Value::as_str)
                .and_then(globecraft_core::time::parse_iso)
                .ok_or_else(|| format!("aurora forecast without {key}"))
        };
        let observed = time("Observation Time")?;
        let forecast = time("Forecast Time")?;
        let rows = json
            .get("coordinates")
            .and_then(serde_json::Value::as_array)
            .ok_or("aurora forecast without coordinates")?;
        let mut values = vec![0u8; LONGITUDES * LATITUDES].into_boxed_slice();
        let mut filled = 0usize;
        for row in rows {
            let Some([lon, lat, chance]) = row.as_array().and_then(|r| {
                Some([
                    r.first()?.as_f64()?,
                    r.get(1)?.as_f64()?,
                    r.get(2)?.as_f64()?,
                ])
            }) else {
                continue;
            };
            let lon = lon.round().rem_euclid(360.0) as usize;
            let lat = (lat.round() + 90.0).clamp(0.0, 180.0) as usize;
            values[lon * LATITUDES + lat] = chance.clamp(0.0, 100.0) as u8;
            filled += 1;
        }
        if filled < LONGITUDES * LATITUDES / 2 {
            return Err(format!("aurora forecast has only {filled} grid values").into());
        }
        Ok(Self {
            observed,
            forecast,
            values,
        })
    }
}

/// The forecast, kept up to date while the layer is on.
#[derive(Default)]
pub struct LiveAurora {
    grid: Option<Arc<AuroraGrid>>,
    attempted: Option<Instant>,
    error: Option<crate::Error>,
    rx: Option<mpsc::Receiver<Result<AuroraGrid, crate::Error>>>,
}

impl LiveAurora {
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        enabled: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(grid)) => {
                    self.grid = Some(Arc::new(grid));
                    self.error = None;
                    self.rx = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("NOAA SWPC disconnected".into());
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if enabled
            && self.rx.is_none()
            && self
                .attempted
                .is_none_or(|t| t.elapsed() >= if self.error.is_some() { RETRY } else { REFRESH })
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

    /// Replaces the forecast, as a finished download does (screenshots and tests use
    /// a forecast of their own).
    pub fn set_grid(&mut self, grid: AuroraGrid) {
        self.grid = Some(Arc::new(grid));
        self.error = None;
        self.attempted = Some(Instant::now());
    }

    pub fn grid(&self) -> Option<Arc<AuroraGrid>> {
        self.grid.clone()
    }

    pub fn status(&self, lang: globecraft_core::language::Lang) -> String {
        if let Some(error) = &self.error {
            return format!("NOAA SWPC: {error}");
        }
        let Some(grid) = &self.grid else {
            return lang.text("aurora-loading-status").into();
        };
        let minutes = ((grid.forecast - now_unix()) / 60.0).round() as i64;
        lang.format(
            "aurora-loaded-status",
            &[
                (
                    "time",
                    globecraft_core::time::format_utc(grid.forecast).into(),
                ),
                ("minutes", minutes.into()),
                ("peak", i64::from(grid.peak()).into()),
            ],
        )
    }
}

async fn download() -> Result<AuroraGrid, crate::Error> {
    let client = crate::http::client(crate::http::Policy::Aircraft)?;
    let bytes = crate::http::body(client.get(URL), MAX_BYTES).await?;
    let json = crate::http::parse_json(bytes).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || {
        AuroraGrid::parse(&json)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_reads_swpc_rows_and_rejects_short_files() {
        let mut rows = Vec::new();
        for lon in 0..360 {
            for lat in -90..=90 {
                let chance = if lon == 10 && lat == 65 { 42 } else { 0 };
                rows.push(serde_json::json!([lon, lat, chance]));
            }
        }
        let json = serde_json::json!({
            "Observation Time": "2026-11-04T09:40:00Z",
            "Forecast Time": "2026-11-04T10:25:00Z",
            "Data Format": "[Longitude, Latitude, Aurora]",
            "coordinates": rows,
        });
        let grid = AuroraGrid::parse(&json).unwrap();
        assert_eq!(grid.value(10, 155), 42);
        assert_eq!(grid.value(370, 155), 42);
        assert_eq!(grid.value(11, 155), 0);
        assert_eq!(grid.peak(), 42);
        assert_eq!(grid.forecast - grid.observed, 45.0 * 60.0);
        let short = serde_json::json!({
            "Observation Time": "2026-11-04T09:40:00Z",
            "Forecast Time": "2026-11-04T10:25:00Z",
            "coordinates": [[0, 0, 5]],
        });
        assert!(AuroraGrid::parse(&short).is_err());
    }

    /// SWPC answers with a whole grid. Run with the Mac checks.
    #[tokio::test]
    #[ignore = "network: downloads NOAA SWPC's aurora forecast"]
    async fn the_live_forecast_parses() {
        let grid = download().await.expect("aurora forecast");
        println!(
            "AURORA forecast {} peak {} %",
            globecraft_core::time::format_utc(grid.forecast),
            grid.peak()
        );
        assert!(grid.forecast >= grid.observed);
    }
}
