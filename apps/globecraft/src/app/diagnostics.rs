//! "Copy Diagnostics" (Help menu): what a bug report needs about this installation, as
//! plain text. The camera position, the addresses of the user's data, file paths other
//! than the log's, and anything about keys are left out.
use super::App;

/// The run log's last lines included.
const LOG_TAIL_LINES: usize = 40;
/// At most this much of the run log's end is read.
const LOG_TAIL_BYTES: u64 = 64 * 1024;

impl App {
    pub(super) fn diagnostics_text(&self) -> String {
        let viewport = self.ctx.input(|input| input.viewport_rect());
        let pixels_per_point = self.ctx.pixels_per_point();
        let settings = &self.settings;
        let log_path = globecraft_data::run_log::path();
        let mut lines = vec![
            format!(
                "GlobeCraft {} on {} {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            format!(
                "Window: {:.0}×{:.0} points at {pixels_per_point} pixels per point",
                viewport.width(),
                viewport.height()
            ),
            format!(
                "Interface scale {}, map text scale {}, GPU globe allowed: {}, uncapped frame rate: {}",
                settings.ui_scale, settings.map_scale, settings.gpu_globe, settings.uncap_fps
            ),
            format!(
                "Language {}, world {}, basemap {}",
                self.lang.code(),
                match self.explorer.active {
                    globecraft_core::body::Body::Earth => "Earth",
                    globecraft_core::body::Body::Moon => "Moon",
                    globecraft_core::body::Body::Custom => "custom planet",
                },
                settings.layers.basemap.code()
            ),
            format!(
                "Visible layers: {}",
                settings.visible_layers().codes().join(", ")
            ),
            format!("Added data layers: {}", settings.user_layers.len()),
            format!(
                "Tile cache: {} MiB decoded, {} downloads in flight",
                self.cache.resident_bytes() / (1024 * 1024),
                self.cache.inflight()
            ),
            format!("Run log: {}", log_path.display()),
            String::new(),
        ];
        match log_tail(&log_path) {
            Ok(tail) => lines.extend(tail),
            Err(error) => lines.push(format!("(run log unreadable: {error})")),
        }
        lines.join("\n")
    }
}

/// The last lines of the run log.
fn log_tail(path: &std::path::Path) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL_BYTES)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.len().saturating_sub(LOG_TAIL_LINES);
    Ok(lines[first..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_tail_keeps_the_last_lines() {
        let path =
            std::env::temp_dir().join(format!("globecraft-diagnostics-{}.log", std::process::id()));
        let text: String = (0..100).map(|n| format!("line {n}\n")).collect();
        std::fs::write(&path, text).unwrap();
        let tail = log_tail(&path).unwrap();
        assert_eq!(tail.len(), LOG_TAIL_LINES);
        assert_eq!(tail.last().map(String::as_str), Some("line 99"));
        std::fs::remove_file(&path).unwrap();
        assert!(log_tail(&path).is_err());
    }
}
