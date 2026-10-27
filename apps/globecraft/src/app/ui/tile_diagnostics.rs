//! The streaming diagnostics overlay (roadmap 1.11, command `ui.tileDiagnostics`):
//! the imagery tiles the view wants, outlined and coloured by state, and a panel with
//! the download queues per source and the download rates. For tuning on a real
//! machine, where the replay benchmark cannot see.
use crate::app::App;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use globecraft_core::camera::Basis;
use globecraft_data::download_stats::{DownloadKind, totals};
use globecraft_data::tiles::Lookup;
use std::time::{Duration, Instant};

const READY_COLOR: Color32 = Color32::from_rgb(80, 210, 120);
const LOADING_COLOR: Color32 = Color32::from_rgb(240, 190, 60);
const MISSING_COLOR: Color32 = Color32::from_rgb(160, 160, 170);
const FAILED_COLOR: Color32 = Color32::from_rgb(235, 80, 80);
const OUTLINE_WIDTH_POINTS: f32 = 1.2;
const LABEL_FONT_POINTS: f32 = 10.0;
const PANEL_FONT_POINTS: f32 = 11.5;
const PANEL_MARGIN_POINTS: f32 = 10.0;
const PANEL_PADDING_POINTS: f32 = 8.0;
const PANEL_OPACITY: f32 = 0.82;
/// Tile labels only while few tiles show, so they stay readable.
const MAX_LABELLED_TILES: usize = 80;
/// Download rates are averaged over this window.
const RATE_WINDOW: Duration = Duration::from_secs(2);

/// Download totals at the start of the current rate window, and the last rates.
#[derive(Default)]
pub(in crate::app) struct DownloadMeter {
    window: Option<(Instant, [u64; 3])>,
    /// Bytes per second: imagery for the view, imagery ahead, border tiles.
    rates: [f64; 3],
}

impl DownloadMeter {
    fn sample(&mut self) -> [f64; 3] {
        let now = Instant::now();
        let bytes = [
            DownloadKind::Imagery,
            DownloadKind::ImageryAhead,
            DownloadKind::BorderTiles,
        ]
        .map(|kind| totals(kind).bytes);
        match self.window {
            Some((start, before)) if now - start >= RATE_WINDOW => {
                let seconds = (now - start).as_secs_f64();
                for i in 0..3 {
                    self.rates[i] = bytes[i].saturating_sub(before[i]) as f64 / seconds;
                }
                self.window = Some((now, bytes));
            }
            Some(_) => {}
            None => self.window = Some((now, bytes)),
        }
        self.rates
    }
}

fn rate_text(bytes_per_second: f64) -> String {
    if bytes_per_second >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_second / 1_000_000.0)
    } else {
        format!("{:.0} KB/s", bytes_per_second / 1_000.0)
    }
}

impl App {
    /// Draws the overlay over the globe in `rect` for the displayed `basis`.
    pub(in crate::app) fn draw_tile_diagnostics(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
    ) {
        if !self.tile_diagnostics {
            return;
        }
        // The levels of the full-resolution frame, as the renderer picks them.
        let (w, h) = globecraft_render::view::raster_size(
            rect.size(),
            painter.ctx().pixels_per_point(),
            true,
            false,
        );
        let provider = self.effective_provider();
        let cfg = globecraft_render::RenderCfg {
            sky: None,
            provider,
            terrain: None,
            rel: if provider == globecraft_data::tiles::Provider::Wayback {
                self.wayback_idx as u16
            } else {
                0
            },
            grid: false,
            grid_step: 10.0,
            atmo: false,
            sun: None,
            lod_scale: 1.0,
            level_blend: true,
            fixed_level: None,
            night_lights: false,
            stars: false,
        };
        let tiles = globecraft_render::diagnostics::view_tiles(w, h, basis, &cfg, rect);
        let mut counts = [0usize; 4];
        let label_font = FontId::monospace(LABEL_FONT_POINTS);
        for tile in &tiles {
            let (state, color) = match self.cache.lookup(tile.key) {
                Lookup::Ready(_) => (0, READY_COLOR),
                Lookup::Pending => (1, LOADING_COLOR),
                _ if self.cache.failed(tile.key) => (3, FAILED_COLOR),
                _ => (2, MISSING_COLOR),
            };
            counts[state] += 1;
            for run in &tile.outline {
                painter.add(egui::Shape::line(
                    run.clone(),
                    Stroke::new(OUTLINE_WIDTH_POINTS, color),
                ));
            }
            if tiles.len() <= MAX_LABELLED_TILES
                && let Some(centre) = tile.centre
            {
                painter.text(
                    centre,
                    Align2::CENTER_CENTER,
                    format!("{}/{}/{}", tile.z, tile.x, tile.y),
                    label_font.clone(),
                    color,
                );
            }
        }

        // The panel: tiles by state, queues per source, rates.
        let lang = self.lang;
        let mut lines = vec![lang.format(
            "diagnostics-tiles-line",
            &[
                ("total", (tiles.len() as i64).into()),
                ("ready", (counts[0] as i64).into()),
                ("loading", (counts[1] as i64).into()),
                ("missing", (counts[2] as i64).into()),
                ("failed", (counts[3] as i64).into()),
            ],
        )];
        lines.push(lang.format(
            "diagnostics-downloads-line",
            &[("count", (self.cache.inflight() as i64).into())],
        ));
        for source in self.cache.diagnostics() {
            if source.queued == 0 && source.downloading == 0 && !source.paused {
                continue;
            }
            let id = if source.paused {
                "diagnostics-source-paused-line"
            } else {
                "diagnostics-source-line"
            };
            lines.push(lang.format(
                id,
                &[
                    ("source", source.provider.code().into()),
                    ("queued", (source.queued as i64).into()),
                    ("downloading", (source.downloading as i64).into()),
                ],
            ));
        }
        let [view, ahead, borders] = self.download_meter.sample();
        lines.push(lang.format(
            "diagnostics-rates-line",
            &[
                ("view", rate_text(view).into()),
                ("ahead", rate_text(ahead).into()),
                ("borders", rate_text(borders).into()),
            ],
        ));
        let font = FontId::monospace(PANEL_FONT_POINTS);
        let galleys: Vec<_> = lines
            .into_iter()
            .map(|line| painter.layout_no_wrap(line, font.clone(), Color32::WHITE))
            .collect();
        let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let height: f32 = galleys.iter().map(|g| g.size().y).sum();
        let origin = rect.left_bottom()
            + Vec2::new(
                PANEL_MARGIN_POINTS,
                -PANEL_MARGIN_POINTS - height - 2.0 * PANEL_PADDING_POINTS,
            );
        let panel = Rect::from_min_size(
            origin,
            Vec2::new(width, height) + Vec2::splat(2.0 * PANEL_PADDING_POINTS),
        );
        painter.rect_filled(panel, 6.0, Color32::BLACK.gamma_multiply(PANEL_OPACITY));
        let mut y = panel.top() + PANEL_PADDING_POINTS;
        for galley in galleys {
            let height = galley.size().y;
            painter.galley(
                Pos2::new(panel.left() + PANEL_PADDING_POINTS, y),
                galley,
                Color32::WHITE,
            );
            y += height;
        }
        // Rates and states change without the view moving.
        painter
            .ctx()
            .request_repaint_after(Duration::from_millis(500));
    }
}
