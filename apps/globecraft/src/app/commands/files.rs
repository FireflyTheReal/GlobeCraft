//! Screenshot commands (`file.*`) and view links (`view.*`).
use super::str_param;
use crate::app::App;
use globecraft_data::settings::screenshot::ScreenshotQuality;
use serde_json::{Value, json};

impl App {
    /// Runs a `file.*` or `view.*` command.
    pub(super) fn run_file(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match id {
            "file.screenshot" => {
                if self.frame.image.is_none() {
                    return Err("The globe has not finished drawing yet.".into());
                }
                let quality = match str_param(p, "quality") {
                    Some(code) => ScreenshotQuality::from_code(code)
                        .ok_or("quality must be normal, plus, pro or pro_max")?,
                    None => self.settings.screenshot_quality,
                };
                self.take_screenshot(quality)?;
                json!({"folder": self.settings.screenshot_dir, "quality": quality.code()})
            }
            "file.cancelScreenshot" => {
                self.cancel_screenshot()?;
                json!({"cancelling": true})
            }
            "file.screenshotQuality" => {
                let quality = str_param(p, "quality")
                    .and_then(ScreenshotQuality::from_code)
                    .ok_or("quality must be normal, plus, pro or pro_max")?;
                self.settings.screenshot_quality = quality;
                json!({"quality": quality.code()})
            }
            "view.copyLink" => self.copy_view_link(&ctx),
            "view.openLink" => {
                let link = str_param(p, "link").ok_or("link is required")?.to_owned();
                self.open_view_link(&link)?
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }
}
