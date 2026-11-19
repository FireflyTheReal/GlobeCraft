//! The timeline's track (roadmap C4, laid out as ArcGIS Map Viewer's time slider): a
//! span of time (`time.range`) with the app's time and the start of the window of
//! events as round handles, the interval between filled, the span's ends as dates,
//! dates under its ticks, and marks where dated data changes: Wayback releases with Wayback imagery,
//! night-light editions with the night lights, and the stretch of kept clouds with
//! the clouds. Dragging runs `time.set` and `time.window` as the bar's buttons do.
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Ui, vec2};
use globecraft_core::layers::MapLayer;
use globecraft_core::time::{DAY_SECS, HOUR_SECS, date_utc, format_utc, parse_iso};
use globecraft_data::tiles::Provider;
use serde_json::{Value, json};

/// The track's rows from its top (points): the labels above, the line, the dates
/// under the ticks.
const TRACK_HEIGHT: f32 = 50.0;
const LABEL_Y: f32 = 8.0;
const LINE_Y: f32 = 25.0;
const TICK_LABEL_Y: f32 = 42.0;
/// Room between two tick dates.
const TICK_LABEL_GAP: f32 = 10.0;
const HANDLE_RADIUS: f32 = 7.0;
const MARK_HEIGHT: f32 = 5.0;
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
    /// Draws the track, `width` wide: the time (and the window of events) above its
    /// middle, the span's start and end dates above its ends, the line with its
    /// handles and marks, and dates under its ticks. Dragging fills `run`. Returns
    /// where the track ends (kept in `TimelineState::track_end`).
    pub(in crate::app) fn timeline_track(
        &self,
        ui: &mut Ui,
        width: f32,
        unix: f64,
        live: bool,
        run: &mut Option<(&'static str, Value)>,
    ) -> Option<f64> {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        let now = globecraft_core::math::now_unix();
        let span = self.timeline.range_days * DAY_SECS;
        let end = track_end(unix, now, span, self.timeline.track_end);
        let start = end - span;
        let mut kept_end = (end != now).then_some(end);
        let (rect, response) = ui.allocate_exact_size(
            vec2(width.max(200.0), TRACK_HEIGHT),
            Sense::click_and_drag(),
        );
        let line = Rect::from_min_max(
            Pos2::new(rect.left() + HANDLE_RADIUS, rect.top() + LINE_Y),
            Pos2::new(rect.right() - HANDLE_RADIUS, rect.top() + LINE_Y),
        );
        let x_of =
            |time: f64| line.left() + ((time - start) / span).clamp(0.0, 1.0) as f32 * line.width();
        let time_at = |x: f32| {
            let at = start + f64::from((x - line.left()) / line.width().max(1.0)) * span;
            (at.clamp(start, end) / MINUTE_SECS).round() * MINUTE_SECS
        };
        let painter = ui.painter_at(rect.expand(2.0));
        let small = regular(10.5);
        // Above: the span's ends, and the time (with the window of events) between.
        let label_y = rect.top() + LABEL_Y;
        painter.text(
            Pos2::new(rect.left(), label_y),
            Align2::LEFT_CENTER,
            date_utc(start),
            small.clone(),
            t.text_dim,
        );
        painter.text(
            Pos2::new(rect.right(), label_y),
            Align2::RIGHT_CENTER,
            date_utc(end),
            small.clone(),
            t.text_dim,
        );
        let window = self.time.window_secs().filter(|secs| *secs > 0.0);
        let shown = match window {
            Some(secs) => format!("{} – {}", short_utc(unix - secs), format_utc(unix)),
            None => format_utc(unix),
        };
        let shown = if live {
            format!("{shown} · {}", lang.text("timeline-live"))
        } else {
            shown
        };
        painter.text(
            Pos2::new(rect.center().x, label_y),
            Align2::CENTER_CENTER,
            shown,
            semibold(12.5),
            t.text,
        );
        painter.line_segment(
            [line.left_top(), line.right_top()],
            egui::Stroke::new(3.0, t.text_dim.gamma_multiply(0.35)),
        );
        // Ticks across the line, dates under them where there is room.
        let tick = tick_secs(span);
        let mut at = (start / tick).ceil() * tick;
        let mut free_from = f32::MIN;
        while at <= end {
            let x = x_of(at);
            painter.line_segment(
                [
                    Pos2::new(x, line.top() - 4.0),
                    Pos2::new(x, line.top() + 4.0),
                ],
                egui::Stroke::new(1.0, t.text_dim.gamma_multiply(0.6)),
            );
            let label = tick_label(at, span);
            let label_w = super::widgets::text_w(ui, &label, small.clone());
            if x - label_w / 2.0 >= free_from
                && x - label_w / 2.0 >= rect.left()
                && x + label_w / 2.0 <= rect.right()
            {
                painter.text(
                    Pos2::new(x, rect.top() + TICK_LABEL_Y),
                    Align2::CENTER_CENTER,
                    label,
                    small.clone(),
                    t.text_dim,
                );
                free_from = x + label_w / 2.0 + TICK_LABEL_GAP;
            }
            at += tick;
        }
        // Where dated data changes, just under the line.
        let below = line.top() + 6.0;
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
                    Pos2::new(x_of(from), line.top() - 1.5),
                    Pos2::new(x_of(end), line.top() + 1.5),
                );
                painter.rect_filled(
                    band,
                    1.0,
                    egui::Color32::from_gray(200).gamma_multiply(0.45),
                );
            }
        }
        // The window of events between its two handles, and the time's.
        let time_x = x_of(unix);
        let window_x = window.map(|secs| x_of(unix - secs));
        if let Some(window_x) = window_x {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(window_x, line.top() - 2.5),
                    Pos2::new(time_x, line.top() + 2.5),
                ),
                2.0,
                t.accent,
            );
            handle(&painter, &t, Pos2::new(window_x, line.top()));
        }
        handle(&painter, &t, Pos2::new(time_x, line.top()));
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
                _ => ("time.set", json!({ "time": time })),
            });
            // Held on the track: it does not move under the pointer.
            kept_end = Some(end);
        }
        response.on_hover_text(lang.text("timeline-track-help"));
        kept_end
    }
}

/// A handle on the track, round as Map Viewer's thumbs.
fn handle(painter: &egui::Painter, t: &Tokens, at: Pos2) {
    painter.circle_filled(at, HANDLE_RADIUS, t.pane);
    painter.circle_stroke(at, HANDLE_RADIUS - 1.0, egui::Stroke::new(2.5, t.accent));
}

/// A tick's date as short as the span allows: the hour across a day or two, the month
/// and day across a week or a month, year and month across a year, the year beyond.
fn tick_label(time: f64, span: f64) -> String {
    let full = format_utc(time);
    match span / DAY_SECS {
        days if days <= 2.0 => full[11..16].to_owned(),
        days if days <= 62.0 => full[5..10].to_owned(),
        days if days <= 400.0 => full[..7].to_owned(),
        _ => full[..4].to_owned(),
    }
}

/// Date and time without the zone, for the window's start.
fn short_utc(time: f64) -> String {
    format_utc(time)[..16].to_owned()
}

/// One step back or forward on the timeline: a tick of the track.
pub(super) fn step_secs(span: f64) -> f64 {
    tick_secs(span)
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
