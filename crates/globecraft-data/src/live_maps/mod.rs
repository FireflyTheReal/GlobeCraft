//! Cloud/radar fetching, refresh policy and loaded imagery snapshots.
use globecraft_core::language::Lang;
use image::{GrayImage, RgbaImage};
use std::sync::Arc;
mod download;
/// The radar's colours: each band from its lowest reflectivity (dBZ) up, as drawn
/// on the map and in the legend.
pub const RADAR_BANDS: [(f32, [u8; 3]); 5] = [
    (0.0, [42, 205, 60]),
    (18.0, [155, 220, 35]),
    (28.0, [247, 230, 20]),
    (38.0, [255, 145, 0]),
    (47.0, [240, 35, 30]),
];
use download::{download_clouds, download_radar, download_water};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
const CLOUD_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);
const RADAR_INTERVAL: Duration = Duration::from_secs(10 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// The clouds' opacity over the whole Earth (equirectangular, north up, from 180° W),
/// finest level first, each half the size of the one before.
pub struct CloudMap {
    pub levels: Vec<GrayImage>,
}

struct RadarFrame {
    image: RgbaImage,
    time: i64,
}

/// One image fetched again and again: the last one loaded, the download under way,
/// when it was last asked for and why it last failed.
struct Feed<T> {
    value: Option<T>,
    receiver: Option<mpsc::Receiver<Result<T, crate::Error>>>,
    attempt: Option<Instant>,
    error: Option<String>,
}

impl<T> Default for Feed<T> {
    fn default() -> Self {
        Self {
            value: None,
            receiver: None,
            attempt: None,
            error: None,
        }
    }
}

impl<T: Send + 'static> Feed<T> {
    /// Takes in a finished download; whether a new value came.
    fn receive(&mut self, interrupted: &str) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(crate::Error::msg(interrupted)),
        };
        self.receiver = None;
        match result {
            Ok(value) => {
                self.value = Some(value);
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    /// Starts `download` when it is `wanted` and due: `interval` after the last load,
    /// sooner after a failure.
    fn refresh<F>(
        &mut self,
        wanted: bool,
        interval: Duration,
        runtime: &crate::runtime::ServiceRuntime,
        repaint: &Arc<impl Fn() + Send + Sync + 'static>,
        download: impl FnOnce() -> F,
    ) where
        F: std::future::Future<Output = Result<T, crate::Error>> + Send + 'static,
    {
        let loaded = self.value.is_some() && self.error.is_none();
        if !(crate::live::ENABLED
            && wanted
            && self.receiver.is_none()
            && refresh_due(self.attempt, loaded, interval))
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.attempt = Some(Instant::now());
        let (repaint, download) = (repaint.clone(), download());
        runtime.spawn(async move {
            let _ = sender.send(download.await);
            repaint();
        });
    }

    /// The status line: `failed` with the error (and `cached` while an older image
    /// stays), `loaded` or `loading`.
    fn status(&self, lang: Lang, failed: &str, loaded: String, loading: &str) -> String {
        if let Some(error) = &self.error {
            let cached = if self.value.is_some() {
                lang.text("live-cached-image")
            } else {
                ""
            };
            format!("{}{cached}: {error}", lang.text(failed))
        } else if self.value.is_some() {
            loaded
        } else {
            lang.text(loading).to_owned()
        }
    }
}

#[derive(Default)]
pub struct LiveMaps {
    cloud: Feed<Arc<CloudMap>>,
    radar: Feed<RadarFrame>,
    water: Feed<Arc<GrayImage>>,
    generation: u64,
}
impl LiveMaps {
    /// Takes in finished downloads and starts the ones due: the clouds and the radar
    /// while their layers show, the water's shine while `water` (the sun's glint).
    pub fn update(
        &mut self,
        runtime: &crate::runtime::ServiceRuntime,
        clouds: bool,
        radar: bool,
        water: bool,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        let repaint = Arc::new(repaint);
        let arrived = [
            self.cloud.receive("Cloud download interrupted"),
            self.radar.receive("Radar download interrupted"),
            self.water.receive("Water map download interrupted"),
        ];
        if arrived.contains(&true) {
            self.generation += 1;
            repaint();
        }
        self.cloud
            .refresh(clouds, CLOUD_INTERVAL, runtime, &repaint, || async {
                download_clouds().await.map(Arc::new)
            });
        self.radar
            .refresh(radar, RADAR_INTERVAL, runtime, &repaint, download_radar);
        // The water's shine is the clouds' companion: as often, from the same source.
        self.water
            .refresh(water, CLOUD_INTERVAL, runtime, &repaint, || async {
                download_water().await.map(Arc::new)
            });
    }

    /// The cloud map; a new download is a new `Arc`.
    pub fn cloud(&self) -> Option<&Arc<CloudMap>> {
        self.cloud.value.as_ref()
    }
    pub fn radar(&self) -> Option<&RgbaImage> {
        self.radar.value.as_ref().map(|frame| &frame.image)
    }
    /// How much the water shines (0 to 255; land and cloud dark), equirectangular from
    /// 180° W, north up.
    pub fn water(&self) -> Option<&Arc<GrayImage>> {
        self.water.value.as_ref()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn status(&self, clouds: bool, radar: bool, lang: Lang) -> String {
        let mut lines = Vec::new();
        if clouds {
            lines.push(self.cloud.status(
                lang,
                "live-clouds-failed",
                lang.text("live-clouds-ok").to_owned(),
                "live-clouds-loading",
            ));
        }
        if radar {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let age = self
                .radar
                .value
                .as_ref()
                .map_or(0, |frame| (now - frame.time).max(0) / 60);
            lines.push(self.radar.status(
                lang,
                "live-radar-failed",
                lang.format("live-radar-age", &[("minutes", age.to_string().into())]),
                "live-radar-loading",
            ));
        }
        lines.join("\n")
    }
}
fn refresh_due(last: Option<Instant>, loaded: bool, interval: Duration) -> bool {
    last.is_none_or(|t| t.elapsed() >= if loaded { interval } else { RETRY_INTERVAL })
}
