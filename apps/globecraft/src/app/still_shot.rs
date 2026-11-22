//! Screenshots. Normal is the globe exactly as shown (Julien, 2026-11-21: "the sun is
//! not in the normal photo"): the sun, shadows, clouds, buildings, borders, names and
//! live layers, taken from the screen once its tiles have arrived, without the
//! controls over it (navigator, timeline), as Google Earth's Save Image and Cesium's
//! canvas capture do. Plus, Pro and Pro Max render the view off screen at two to eight
//! times the displayed size until every tile has loaded, written to the PNG strip by
//! strip (see `globecraft_render::still`), with the sun, clouds, glint and aurora as
//! on screen and each pixel at its own imagery level, faded between levels as on
//! screen: one level for the whole picture blurred the near ground and sharpened the
//! far one at angles. Rendering and writing run on their own thread; the UI only
//! polls, and can cancel.
use super::App;
use eframe::egui;
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
/// A Normal screenshot waits at most this long for the view's tiles.
const CAPTURE_WAIT: Duration = Duration::from_secs(10);

/// A Normal screenshot waiting for its frame: the screen is captured once the view's
/// tiles have arrived (or after `CAPTURE_WAIT`).
pub(super) struct ScreenCapture {
    path: PathBuf,
    started: std::time::Instant,
    asked: bool,
}

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
        if self.screenshot_running() {
            return Err("A screenshot is still being made.".into());
        }
        let Some(frame) = self.frame.image.as_ref() else {
            return Err("The globe has not finished drawing yet.".into());
        };
        let path = PathBuf::from(&self.settings.screenshot_dir).join(format!(
            "globecraft_{}_{}.png",
            now_unix() as u64,
            quality.code()
        ));
        if quality == ScreenshotQuality::Normal {
            self.frame.capture = Some(ScreenCapture {
                path,
                started: std::time::Instant::now(),
                asked: false,
            });
            self.show_still_progress(
                quality,
                StillProgress {
                    done: 0.0,
                    missing: self.tiles.cache.inflight(),
                },
            );
            self.ctx.request_repaint();
            return Ok(());
        }
        let scale = quality.render_scale();
        let (width, height) = (frame.width * scale, frame.height * scale);
        let aspect = width as f64 / height.max(1) as f64;
        let basis = self.view_basis(aspect);
        let cfg = globecraft_render::RenderCfg {
            // Behind each strip, as the sky layer does on screen (`render_still`).
            sky_behind: true,
            ..self.frame_config(width, height, 1.0)
        };
        let credits = self.frame.renderer.displayed_credits.clone();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let cache = self.tiles.cache.clone();
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
        self.frame.still_shot = Some(StillShot {
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
        self.frame.still_shot.is_some() || self.frame.capture.is_some()
    }

    /// Whether the globe is being captured for a Normal screenshot: its controls are
    /// left out meanwhile.
    pub(super) fn capturing_globe(&self) -> bool {
        self.frame.capture.is_some()
    }

    /// Follows a Normal screenshot: asks for the screen once the view's tiles are in,
    /// then writes the globe's part of it on a thread of its own.
    pub(super) fn poll_screen_capture(&mut self, ctx: &egui::Context) {
        let Some(capture) = &mut self.frame.capture else {
            return;
        };
        let image = ctx.input(|input| {
            input.raw.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let path = capture.path.clone();
            self.frame.capture = None;
            let Some(rect) = super::ui::surface::globe_rect(ctx) else {
                self.status = Lang::En.format(
                    "screenshot-save-error",
                    &[("error", "the globe is not shown".into())],
                );
                return;
            };
            let credits = self.frame.renderer.displayed_credits.clone();
            self.write_capture(&image, rect, ctx.pixels_per_point(), path, credits);
            return;
        }
        if capture.asked {
            ctx.request_repaint();
            return;
        }
        let settled = self.tiles.cache.inflight() == 0
            && self.frame.renderer.is_idle()
            && !self.frame.renderer.waits_for_tiles();
        if settled || capture.started.elapsed() >= CAPTURE_WAIT {
            capture.asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    /// Writes the part of a screen `image` under `rect` (points) to `path` on a thread.
    fn write_capture(
        &mut self,
        image: &egui::ColorImage,
        rect: egui::Rect,
        pixels_per_point: f32,
        path: PathBuf,
        credits: String,
    ) {
        let [image_w, image_h] = image.size;
        let to = |value: f32, most: usize| {
            ((value * pixels_per_point).round().max(0.0) as usize).min(most)
        };
        let (left, right) = (to(rect.left(), image_w), to(rect.right(), image_w));
        let (top, bottom) = (to(rect.top(), image_h), to(rect.bottom(), image_h));
        let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
        let mut rgba = Vec::with_capacity(width * height * 4);
        for row in top..bottom {
            for pixel in &image.pixels[row * image_w + left..row * image_w + right] {
                rgba.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b(), 255]);
            }
        }
        let (sender, receiver) = mpsc::channel();
        let ctx = self.ctx.clone();
        let written = std::thread::Builder::new()
            .name("globecraft-capture".into())
            .spawn(move || {
                let message = match PngStrips::create(&path, width as u32, height as u32, &credits)
                    .and_then(|mut png| {
                        png.write_rows(&rgba)?;
                        png.finish()
                    }) {
                    Ok(path) => StillMessage::Saved { path, missing: 0 },
                    Err(error) => StillMessage::Failed(error.to_string()),
                };
                let _ = sender.send(message);
                ctx.request_repaint();
            });
        match written {
            Ok(_) => {
                self.frame.still_shot = Some(StillShot {
                    quality: ScreenshotQuality::Normal,
                    receiver,
                    cancel: Arc::new(AtomicBool::new(false)),
                });
            }
            Err(error) => {
                self.status = Lang::En.format(
                    "screenshot-save-error",
                    &[("error", error.to_string().into())],
                );
            }
        }
    }

    /// Stops the screenshot being made; its partial file is deleted.
    pub(super) fn cancel_screenshot(&mut self) -> Result<(), String> {
        if self.frame.capture.take().is_some() {
            self.status = self.lang.text("screenshot-cancelled-status").into();
            return Ok(());
        }
        let shot = self
            .frame
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
        let Some(shot) = &self.frame.still_shot else {
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
        self.frame.still_shot = None;
        match end {
            StillMessage::Saved { path, missing } => {
                if missing > 0 {
                    globecraft_data::log_line!(
                        "Screenshot saved with {missing} tiles that never arrived"
                    );
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
