//! Screenshots: the view rendered off screen at one to eight times the displayed size
//! until every tile has loaded, written to the PNG strip by strip as it is made (see
//! `globecraft_render::still`). Rendering runs on its own thread; the UI only polls,
//! and can cancel it.
use super::App;
use globecraft_core::language::Lang;
use globecraft_core::math::now_unix;
use globecraft_data::files::PngStrips;
use globecraft_data::settings::screenshot::ScreenshotQuality;
use globecraft_render::still::{StillEnd, StillProgress};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// A strip whose tiles stop arriving for this long is saved as it is.
const STILL_STALL: Duration = Duration::from_secs(30);

enum StillMessage {
    Progress(StillProgress),
    Saved { path: PathBuf, missing: usize },
    Cancelled,
    Failed(String),
}

pub(super) struct StillShot {
    quality: ScreenshotQuality,
    receiver: mpsc::Receiver<StillMessage>,
    cancel: Arc<AtomicBool>,
}

fn quality_label(lang: Lang, quality: ScreenshotQuality) -> &'static str {
    lang.text(match quality {
        ScreenshotQuality::Normal => "screenshot-quality-normal",
        ScreenshotQuality::Plus => "screenshot-quality-plus",
        ScreenshotQuality::Pro => "screenshot-quality-pro",
        ScreenshotQuality::ProMax => "screenshot-quality-pro-max",
    })
}

impl App {
    /// Starts a screenshot at `quality` in the background.
    pub(super) fn take_screenshot(&mut self, quality: ScreenshotQuality) -> Result<(), String> {
        if self.still_shot.is_some() {
            return Err("A screenshot is still being made.".into());
        }
        let Some((width, height, _)) = self.image.as_ref() else {
            return Err("The globe has not finished drawing yet.".into());
        };
        let scale = quality.render_scale();
        let (width, height) = (width * scale, height * scale);
        let aspect = width as f64 / height.max(1) as f64;
        let basis = self.cam.basis_for_model(aspect, self.active_earth_model());
        let cfg = globecraft_render::RenderCfg {
            level_blend: quality.level_blend(),
            ..self.frame_config(width, height, 1.0)
        };
        let path = PathBuf::from(&self.settings.screenshot_dir).join(format!(
            "globecraft_{}_{}.png",
            now_unix() as u64,
            quality.code()
        ));
        let credits = self.renderer.displayed_credits.clone();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let cache = self.cache.clone();
        let ctx = self.ctx.clone();
        std::thread::Builder::new()
            .name("globecraft-still".into())
            .spawn(move || {
                let message = match PngStrips::create(&path, width as u32, height as u32, &credits)
                {
                    Err(error) => StillMessage::Failed(error.to_string()),
                    Ok(mut png) => {
                        let mut last = None;
                        let end = globecraft_render::still::render_still(
                            width,
                            height,
                            &basis,
                            &cfg,
                            &cache,
                            STILL_STALL,
                            &stop,
                            |progress| {
                                // Whole percents and tile counts: a message per change.
                                let shown = ((progress.done * 100.0) as u32, progress.missing);
                                if last.replace(shown) != Some(shown) {
                                    let _ = sender.send(StillMessage::Progress(progress));
                                    ctx.request_repaint();
                                }
                            },
                            |rows| png.write_rows(rows),
                        );
                        match end {
                            Ok(StillEnd::Finished { missing }) => match png.finish() {
                                Ok(path) => StillMessage::Saved { path, missing },
                                Err(error) => StillMessage::Failed(error.to_string()),
                            },
                            // Dropping the unfinished file deletes it.
                            Ok(StillEnd::Cancelled) => StillMessage::Cancelled,
                            Err(error) => StillMessage::Failed(error.to_string()),
                        }
                    }
                };
                let _ = sender.send(message);
                ctx.request_repaint();
            })
            .map_err(|error| format!("Could not start the screenshot: {error}"))?;
        self.still_shot = Some(StillShot {
            quality,
            receiver,
            cancel,
        });
        self.show_still_progress(
            quality,
            StillProgress {
                done: 0.0,
                missing: 0,
            },
        );
        Ok(())
    }

    /// Whether a screenshot is being made.
    pub(super) fn screenshot_running(&self) -> bool {
        self.still_shot.is_some()
    }

    /// Stops the screenshot being made; its partial file is deleted.
    pub(super) fn cancel_screenshot(&mut self) -> Result<(), String> {
        let shot = self
            .still_shot
            .as_ref()
            .ok_or_else(|| self.lang.text("screenshot-none-running-error").to_string())?;
        shot.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn show_still_progress(&mut self, quality: ScreenshotQuality, progress: StillProgress) {
        self.status = self.lang.format(
            "screenshot-rendering-status",
            &[
                ("quality", quality_label(self.lang, quality).into()),
                (
                    "percent",
                    ((progress.done * 100.0) as u32).to_string().into(),
                ),
                ("tiles", progress.missing.to_string().into()),
            ],
        );
    }

    /// Follows a background screenshot and reports how it ended.
    pub(super) fn poll_still_shot(&mut self) {
        let Some(shot) = &self.still_shot else {
            return;
        };
        let quality = shot.quality;
        let mut end = None;
        let mut latest = None;
        while let Ok(message) = shot.receiver.try_recv() {
            match message {
                StillMessage::Progress(progress) => latest = Some(progress),
                other => end = Some(other),
            }
        }
        if let Some(progress) = latest {
            self.show_still_progress(quality, progress);
        }
        let Some(end) = end else {
            return;
        };
        self.still_shot = None;
        match end {
            StillMessage::Saved { path, missing } => {
                if missing > 0 {
                    eprintln!("Screenshot saved with {missing} tiles that never arrived");
                }
                self.screenshot_saved(path);
            }
            StillMessage::Cancelled => {
                self.status = self.lang.text("screenshot-cancelled-status").into();
            }
            StillMessage::Failed(error) => {
                self.status = Lang::En.format("screenshot-save-error", &[("error", error.into())]);
            }
            StillMessage::Progress(_) => {}
        }
    }

    pub(super) fn screenshot_quality_label(&self, quality: ScreenshotQuality) -> &'static str {
        quality_label(self.lang, quality)
    }
}
