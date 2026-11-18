//! The timeline's track (roadmap C4, after ArcGIS Map Viewer's time slider): a span of
//! time (`time.range`) with the app's time as a round handle and the start of the
//! window of events as a square one, the interval between shaded, the span's ends as
//! dates, and marks where dated data changes: Wayback releases with Wayback imagery,
//! night-light editions with the night lights, and the stretch of kept clouds with
//! the clouds. Dragging runs `time.set` and `time.window` as the bar's buttons do.
use super::theme::{Tokens, regular};
use crate::app::App;
use crate::app::time::RANGES;
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Ui, vec2};
use globecraft_core::layers::MapLayer;
use globecraft_core::time::{DAY_SECS, HOUR_SECS, date_utc, parse_iso};
use globecraft_data::tiles::Provider;
use serde_json::{Value, json};

const TRACK_HEIGHT: f32 = 30.0;
/// The track's line sits this far from its top; marks hang below it.
const LINE_Y: f32 = 11.0;
const HANDLE_RADIUS: f32 = 6.0;
const MARK_HEIGHT: f32 = 5.0;
/// Room at each end for the span's date.
const END_LABEL_WIDTH: f32 = 74.0;
/// A time moved off the track brings it along, the time this share from its end.
const REFOCUS_FROM_END: f64 = 0.2;
/// A drag lands on whole minutes.
const MINUTE_SECS: f64 = 60.0;

/// Which handle a drag holds.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    Time,
    WindowStart,
}

/// Where the track ends: where it was while the app's time is on it; else at the real
/// time when that brings the time on it, or moved so that it does.
pub(super) fn track_end(unix: f64, now: f64, span: f64, kept: Option<f64>) -> f64 {
    let end = kept.unwrap_or(now);
    if unix >= end - span && unix <= end {
        return end;
    }
    if unix >= now - span && unix <= now {
        return now;
    }
    unix + REFOCUS_FROM_END * span
}

impl App {
    /// Draws the track under the bar's buttons, `width` wide; dragging fills `run`.
    /// Returns where the track ends (kept in `TimelineState::track_end`).
    pub(in crate::app) fn timeline_track(
        &self,
        ui: &mut Ui,
        width: f32,
        unix: f64,
        run: &mut Option<(&'static str, Value)>,
    ) -> Option<f64> {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        let now = globecraft_core::math::now_unix();
        let span = self.timeline.range_days * DAY_SECS;
        let end = track_end(unix, now, span, self.timeline.track_end);
        let start = end - span;
        let mut kept_end = (end != now).then_some(end);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
            let range_w = 72.0;
            let track_w = (width - range_w - 8.0).max(160.0);
            let (rect, response) =
                ui.allocate_exact_size(vec2(track_w, TRACK_HEIGHT), Sense::click_and_drag());
            let line = Rect::from_min_max(
                Pos2::new(rect.left() + END_LABEL_WIDTH, rect.top() + LINE_Y),
                Pos2::new(rect.right() - END_LABEL_WIDTH, rect.top() + LINE_Y),
            );
            let x_of = |time: f64| {
                line.left() + ((time - start) / span).clamp(0.0, 1.0) as f32 * line.width()
            };
            let time_at = |x: f32| {
                let at = start + f64::from((x - line.left()) / line.width().max(1.0)) * span;
                (at.clamp(start, end) / MINUTE_SECS).round() * MINUTE_SECS
            };
            let painter = ui.painter_at(rect);
            let font = regular(10.5);
            painter.text(
                Pos2::new(rect.left(), line.top()),
                Align2::LEFT_CENTER,
                date_utc(start),
                font.clone(),
                t.text_dim,
            );
            painter.text(
                Pos2::new(rect.right(), line.top()),
                Align2::RIGHT_CENTER,
                date_utc(end),
                font,
                t.text_dim,
            );
            painter.line_segment(
                [line.left_top(), line.right_top()],
                egui::Stroke::new(2.0, t.text_dim.gamma_multiply(0.6)),
            );
            // Day (or hour, or year) ticks above the line.
            let tick = tick_secs(span);
            let mut at = (start / tick).ceil() * tick;
            while at <= end {
                let x = x_of(at);
                painter.line_segment(
                    [Pos2::new(x, line.top() - 4.0), Pos2::new(x, line.top())],
                    egui::Stroke::new(1.0, t.text_dim.gamma_multiply(0.5)),
                );
                at += tick;
            }
            // Where dated data changes, below the line.
            let below = line.top() + 4.0;
            let mark = |x: f32, colour: egui::Color32| {
                painter.line_segment(
                    [Pos2::new(x, below), Pos2::new(x, below + MARK_HEIGHT)],
                    egui::Stroke::new(1.5, colour),
                );
            };
            if self.settings.layers.basemap == Provider::Wayback {
                for release in &self.wayback.releases {
                    if let Some(time) = parse_iso(&release.date)
                        && (start..=end).contains(&time)
                    {
                        mark(x_of(time), t.accent);
                    }
                }
            }
            if self.layer(MapLayer::NightLights) {
                let source = Provider::NightLights.source();
                for edition in 0..source.editions.len() {
                    if let Some(time) = source
                        .edition_year(edition as u16)
                        .and_then(|year| parse_iso(&format!("{year}-01-01")))
                        && (start..=end).contains(&time)
                    {
                        mark(x_of(time), egui::Color32::from_rgb(240, 200, 90));
                    }
                }
            }
            if self.layer(MapLayer::Clouds) {
                let from = globecraft_data::live_maps::CLOUDS_KEPT_SINCE_UNIX.max(start);
                if from < end {
                    let band = Rect::from_min_max(
                        Pos2::new(x_of(from), below + MARK_HEIGHT + 2.0),
                        Pos2::new(x_of(end), below + MARK_HEIGHT + 4.0),
                    );
                    painter.rect_filled(
                        band,
                        1.0,
                        egui::Color32::from_gray(200).gamma_multiply(0.6),
                    );
                }
            }
            // The window of events and the time.
            let window = self.time.window_secs().filter(|secs| *secs > 0.0);
            let time_x = x_of(unix);
            let window_x = window.map(|secs| x_of(unix - secs));
            if let Some(window_x) = window_x {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(window_x, line.top() - 3.0),
                        Pos2::new(time_x, line.top() + 3.0),
                    ),
                    2.0,
                    t.accent.gamma_multiply(0.35),
                );
                painter.rect_filled(
                    Rect::from_center_size(
                        Pos2::new(window_x, line.top()),
                        vec2(HANDLE_RADIUS * 1.6, HANDLE_RADIUS * 1.6),
                    ),
                    2.0,
                    t.accent,
                );
            }
            painter.circle_filled(Pos2::new(time_x, line.top()), HANDLE_RADIUS, t.accent);
            painter.circle_stroke(
                Pos2::new(time_x, line.top()),
                HANDLE_RADIUS,
                egui::Stroke::new(1.5, t.text),
            );
            // A drag holds the handle nearer where it began; a click sets the time.
            let id = response.id.with("held");
            if response.drag_started()
                && let Some(at) = response.interact_pointer_pos()
            {
                let held = match window_x {
                    Some(window_x) if (at.x - window_x).abs() < (at.x - time_x).abs() => {
                        Held::WindowStart
                    }
                    _ => Held::Time,
                };
                ui.ctx().data_mut(|data| data.insert_temp(id, held));
            }
            let held = ui.ctx().data(|data| data.get_temp::<Held>(id));
            if let Some(at) = response.interact_pointer_pos()
                && (response.dragged() || response.clicked())
            {
                let time = time_at(at.x);
                *run = Some(match (held, response.dragged()) {
                    (Some(Held::WindowStart), true) => (
                        "time.window",
                        json!({"hours": ((unix - time) / HOUR_SECS).max(1.0 / 60.0)}),
                    ),
                    _ => ("time.set", json!({"time": time})),
                });
                // Held on the track: it does not move under the pointer.
                kept_end = Some(end);
            }
            response.on_hover_text(lang.text("timeline-track-help"));
            // The span.
            let key = RANGES
                .iter()
                .find(|(days, _)| *days == self.timeline.range_days)
                .map_or("timeline-range-week", |(_, key)| key);
            let button = egui::Button::new(lang.text(key)).min_size(vec2(range_w, 0.0));
            egui::containers::menu::MenuButton::from_button(button)
                .ui(ui, |ui| {
                    for (days, key) in RANGES {
                        if ui
                            .radio(days == self.timeline.range_days, lang.text(key))
                            .clicked()
                        {
                            *run = Some(("time.range", json!({ "days": days })));
                            ui.close();
                        }
                    }
                })
                .0
                .on_hover_text(lang.text("cmd-time-range-help"));
        });
        kept_end
    }
}

/// The step between the track's ticks for a span: hours across a day, days across a
/// week or a month, months across a year, years across ten.
fn tick_secs(span: f64) -> f64 {
    match span / DAY_SECS {
        days if days <= 2.0 => 3.0 * HOUR_SECS,
        days if days <= 62.0 => DAY_SECS,
        days if days <= 400.0 => 30.0 * DAY_SECS,
        _ => 365.0 * DAY_SECS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_track_follows_a_time_that_leaves_it() {
        let (now, span) = (1_000_000.0, 7.0 * DAY_SECS);
        // On the track: it ends at the real time, or where it was kept.
        assert_eq!(track_end(now - DAY_SECS, now, span, None), now);
        assert_eq!(
            track_end(now - 9.0 * DAY_SECS, now, span, Some(now - 5.0 * DAY_SECS)),
            now - 5.0 * DAY_SECS
        );
        // Off it: at the real time when that holds it, else moved so the time is a
        // fifth of the span from its end.
        let moved = track_end(now - 30.0 * DAY_SECS, now, span, None);
        assert!((moved - (now - 30.0 * DAY_SECS + 0.2 * span)).abs() < 1.0);
        assert_eq!(
            track_end(now - 2.0 * DAY_SECS, now, span, Some(now - 20.0 * DAY_SECS)),
            now
        );
        let ahead = track_end(now + 3.0 * DAY_SECS, now, span, None);
        assert!(ahead > now + 3.0 * DAY_SECS);
    }

    #[test]
    fn ticks_grow_with_the_span() {
        assert_eq!(tick_secs(DAY_SECS), 3.0 * HOUR_SECS);
        assert_eq!(tick_secs(7.0 * DAY_SECS), DAY_SECS);
        assert!(tick_secs(3650.0 * DAY_SECS) > tick_secs(365.0 * DAY_SECS));
    }
}
