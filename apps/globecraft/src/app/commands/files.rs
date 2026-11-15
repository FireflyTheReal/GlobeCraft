//! Screenshot and settings-file commands (`file.*`) and view links (`view.*`).
use crate::app::App;
use globecraft_core::commands::FileCommand;
use globecraft_data::settings::screenshot::ScreenshotQuality;
use serde_json::{Value, json};

impl App {
    /// Runs a `file.*` or `view.*` command.
    pub(super) fn run_file(&mut self, command: FileCommand) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match command {
            FileCommand::Screenshot(p) => {
                if self.frame.image.is_none() {
                    return Err("The globe has not finished drawing yet.".into());
                }
                let quality = match p.quality {
                    Some(code) => ScreenshotQuality::from_code(&code)
                        .ok_or("quality must be normal, plus, pro or pro_max")?,
                    None => self.settings.screenshot_quality,
                };
                self.take_screenshot(quality)?;
                json!({"folder": self.settings.screenshot_dir, "quality": quality.code()})
            }
            FileCommand::CancelScreenshot => {
                self.cancel_screenshot()?;
                json!({"cancelling": true})
            }
            FileCommand::ScreenshotQuality(p) => {
                let quality = ScreenshotQuality::from_code(&p.quality)
                    .ok_or("quality must be normal, plus, pro or pro_max")?;
                self.settings.screenshot_quality = quality;
                json!({"quality": quality.code()})
            }
            FileCommand::ExportSettings => {
                self.pick_settings_export();
                json!({})
            }
            FileCommand::ImportSettings => {
                self.pick_settings_import();
                json!({})
            }
            FileCommand::CopyLink => self.copy_view_link(&ctx),
            FileCommand::OpenLink(p) => self.open_view_link(&p.link)?,
        })
    }
}
