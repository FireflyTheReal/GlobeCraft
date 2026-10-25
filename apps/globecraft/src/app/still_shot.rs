//! High-quality screenshots (Plus, Pro, Pro Max): the view rendered off screen at one,
//! two or four times the displayed size until every tile is sharp, then saved like a
//! normal screenshot. Rendering runs on its own thread; the UI only polls.
use super::App;
use globecraft_core::language::Lang;
use globecraft_core::math::now_unix;
use globecraft_data::settings::screenshot::ScreenshotQuality;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// A still that has not finished loading by then is saved as it is.
const STILL_DEADLINE: Duration = Duration::from_secs(120);

enum StillMessage {
    Missing(usize),
    Done {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
        missing: usize,
    },
}

pub(super) struct StillShot {
    quality: ScreenshotQuality,
    receiver: mpsc::Receiver<StillMessage>,
    path: PathBuf,
    credits: String,
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
    /// Takes a screenshot at `quality`: Normal saves the frame on screen, the others
    /// start a background render.
    pub(super) fn take_screenshot(&mut self, quality: ScreenshotQuality) -> Result<(), String> {
        let Some(scale) = quality.render_scale() else {
            self.save_screenshot();
            return Ok(());
        };
        if self.still_shot.is_some() {
            return Err("A screenshot is still being made.".into());
        }
        let Some((width, height, _)) = self.image.as_ref() else {
            return Err("The globe has not finished drawing yet.".into());
        };
        let (width, height) = (width * scale, height * scale);
        let aspect = width as f64 / height.max(1) as f64;
        let basis = self.cam.basis_for_model(aspect, self.active_earth_model());
        let cfg = self.frame_config(width, height, 1.0);
        let (sender, receiver) = mpsc::channel();
        let cache = self.cache.clone();
        let ctx = self.ctx.clone();
        std::thread::Builder::new()
            .name("globecraft-still".into())
            .spawn(move || {
                let last = std::cell::Cell::new(usize::MAX);
                let still = globecraft_render::still::render_still(
                    width,
                    height,
                    &basis,
                    &cfg,
                    &cache,
                    STILL_DEADLINE,
                    |missing| {
                        if last.replace(missing) != missing {
                            let _ = sender.send(StillMessage::Missing(missing));
                            ctx.request_repaint();
                        }
                    },
                );
                let _ = sender.send(StillMessage::Done {
                    width,
                    height,
                    pixels: still.pixels,
                    missing: still.missing,
                });
                ctx.request_repaint();
            })
            .map_err(|error| format!("Could not start the screenshot: {error}"))?;
        self.still_shot = Some(StillShot {
            quality,
            receiver,
            path: PathBuf::from(&self.settings.screenshot_dir).join(format!(
                "globecraft_{}_{}.png",
                now_unix() as u64,
                quality.code()
            )),
            credits: self.renderer.displayed_credits.clone(),
        });
        self.status = self.lang.format(
            "screenshot-rendering-status",
            &[
                ("quality", quality_label(self.lang, quality).into()),
                ("tiles", "…".into()),
            ],
        );
        Ok(())
    }

    /// Follows a background screenshot and saves it when done.
    pub(super) fn poll_still_shot(&mut self) {
        let Some(shot) = &self.still_shot else {
            return;
        };
        let mut done = None;
        while let Ok(message) = shot.receiver.try_recv() {
            match message {
                StillMessage::Missing(missing) => {
                    self.status = self.lang.format(
                        "screenshot-rendering-status",
                        &[
                            ("quality", quality_label(self.lang, shot.quality).into()),
                            ("tiles", missing.to_string().into()),
                        ],
                    );
                }
                StillMessage::Done {
                    width,
                    height,
                    pixels,
                    missing,
                } => done = Some((width, height, pixels, missing)),
            }
        }
        let Some((width, height, pixels, missing)) = done else {
            return;
        };
        let Some(shot) = self.still_shot.take() else {
            return;
        };
        if missing > 0 {
            eprintln!("Screenshot saved with {missing} tiles still missing after the time limit");
        }
        if let Err(error) = self.file_jobs.screenshot_with_credits(
            shot.path,
            width as u32,
            height as u32,
            &Arc::new(pixels),
            shot.credits,
        ) {
            self.status = Lang::En.format(
                "screenshot-save-error",
                &[("error", error.to_string().into())],
            );
        }
    }

    pub(super) fn screenshot_quality_label(&self, quality: ScreenshotQuality) -> &'static str {
        quality_label(self.lang, quality)
    }
}
