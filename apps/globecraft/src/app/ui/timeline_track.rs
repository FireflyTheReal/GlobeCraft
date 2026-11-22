//! The timeline's track (roadmap C4; since 2026-11-22 after Bing Maps' sun slider):
//! a span of time (`time.range`, a day by default: the place's own day), its line
//! coloured by day and night at the place in view, with the app's time as a round
//! handle holding the sun as it stands there, the window of events' start as a
//! smaller one and the interval between filled, dates or times under its ticks, the
//! real time as a thin mark, and marks where dated data changes: Wayback releases
//! with Wayback imagery, night-light editions with the night lights, and the stretch
//! of kept clouds with the clouds. Dragging runs `time.set` and `time.window` as the
//! bar's buttons do. With a loop (`time.loop`), a row under the dates holds its two
//! handles.
use super::theme::{Tokens, regular};
use crate::app::App;
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Ui, vec2};
use globecraft_core::layers::MapLayer;
use globecraft_core::time::{
    DAY_SECS, HOUR_SECS, civil_from_days, date_utc, days_from_civil, format_utc, parse_iso,
};
use globecraft_data::tiles::Provider;
use serde_json::{Value, json};

/// The track's rows from its top (points): the marks, the line, the dates under the
/// ticks.
const TRACK_HEIGHT: f32 = 40.0;
const LINE_Y: f32 = 15.0;
const TICK_LABEL_Y: f32 = 32.0;
/// Half the line's thickness, and half the real time's mark across it.
const LINE_HALF: f32 = 2.5;
const NOW_MARK_HALF: f32 = 7.0;
/// Room between two tick dates.
const TICK_LABEL_GAP: f32 = 10.0;
/// The time's handle (it holds the sun) and the window of events' handle.
const HANDLE_RADIUS: f32 = 11.0;
const WINDOW_HANDLE_RADIUS: f32 = 6.0;
const MARK_HEIGHT: f32 = 5.0;
/// The line shows day and night across spans up to this many days, in pieces this
/// many points long.
const DAYLIGHT_SPAN_DAYS: f64 = 7.0;
const DAYLIGHT_PIECE_POINTS: f32 = 3.0;
/// The line's colours: night, the sun on the horizon, day; night past this cosine
/// below the horizon (12°, nautical twilight's end), day above this one (10°).
const NIGHT_COLOUR: [u8; 3] = [52, 66, 112];
const HORIZON_COLOUR: [u8; 3] = [232, 130, 104];
const DAY_COLOUR: [u8; 3] = [246, 204, 110];
const TWILIGHT_COSINE: f64 = 0.208;
const LOW_SUN_COSINE: f64 = 0.174;
/// The handle shows the moon once the sun is this far down (its disc gone, 0.8°).
const SUN_SET_COSINE: f64 = -0.014;
/// The loop's row under the dates, while there is a loop: its height, the middle of
/// its line from the track's top, and its handles' size.
const LOOP_ROW_HEIGHT: f32 = 16.0;
const LOOP_Y: f32 = TRACK_HEIGHT + 4.0;
const LOOP_HANDLE: egui::Vec2 = vec2(7.0, 13.0);
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
    /// Draws the track, `width` wide, its times in a zone `zone` seconds ahead of UTC:
    /// the line coloured by the sun's height at the place in view (night, twilight,
    /// day), ticks with their times or dates under it, the real time as a thin mark,
    /// the marks of dated data, and the time's handle, which shows the sun as it
    /// stands there (as Bing Maps' sun slider). A day's span is the place's own day,
    /// midnight to midnight. Dragging or clicking fills `run`; the time under the
    /// pointer shows beside it. Returns where the track ends while kept away from the
    /// real time (`TimelineState::track_end`).
    pub(in crate::app) fn timeline_track(
        &self,
        ui: &mut Ui,
        width: f32,
        (unix, zone): (f64, f64),
        run: &mut Option<(&'static str, Value)>,
    ) -> Option<f64> {
        let t = Tokens::get(ui.ctx());
        let lang = self.lang;
        let now = globecraft_core::math::now_unix();
        let span = self.timeline.range_days * DAY_SECS;
        let (start, end, mut kept_end) = if span <= DAY_SECS {
            let start = local_midnight(unix, zone);
            (start, start + span, None)
        } else {
            let end = track_end(unix, now, span, self.timeline.track_end);
            (end - span, end, (end != now).then_some(end))
        };
        let looped = self.time.loop_range();
        let height = TRACK_HEIGHT
            + if looped.is_some() {
                LOOP_ROW_HEIGHT
            } else {
                0.0
            };
        let (rect, response) =
            ui.allocate_exact_size(vec2(width.max(160.0), height), Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Slider,
                true,
                lang.text("timeline-track-help"),
            )
        });
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
        let painter = ui.painter_at(rect.expand(HANDLE_RADIUS));
        let small = regular(10.5);
        let place = globecraft_core::math::ll_to_v3(self.cam.lat, self.cam.lon);
        // The line: day and night at the place in view, while the span shows them.
        if span <= DAYLIGHT_SPAN_DAYS * DAY_SECS {
            let pieces = (line.width() / DAYLIGHT_PIECE_POINTS).ceil().max(1.0) as usize;
            for piece in 0..pieces {
                let (a, b) = (
                    piece as f32 / pieces as f32,
                    (piece + 1) as f32 / pieces as f32,
                );
                let middle = start + f64::from((a + b) / 2.0) * span;
                let colour = daylight_colour(sun_cosine(place, middle));
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(line.left() + a * line.width(), line.top() - LINE_HALF),
                        Pos2::new(line.left() + b * line.width() + 0.5, line.top() + LINE_HALF),
                    ),
                    0.0,
                    colour,
                );
            }
        } else {
            painter.line_segment(
                [line.left_top(), line.right_top()],
                egui::Stroke::new(2.0 * LINE_HALF, t.text_dim.gamma_multiply(0.35)),
            );
        }
        // Ticks under the line, their times or dates under them where there is room.
        let mut free_from = f32::MIN;
        for at in tick_times(start + zone, end + zone, span) {
            let at = at - zone;
            let x = x_of(at);
            painter.line_segment(
                [
                    Pos2::new(x, line.top() + LINE_HALF + 1.0),
                    Pos2::new(x, line.top() + LINE_HALF + 4.0),
                ],
                egui::Stroke::new(1.0, t.text_dim.gamma_multiply(0.7)),
            );
            let label = tick_label(at + zone, span);
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
        }
        // Where dated data changes, just over the line.
        let above = line.top() - LINE_HALF - 1.0;
        let mark = |x: f32, colour: egui::Color32| {
            painter.line_segment(
                [Pos2::new(x, above - MARK_HEIGHT), Pos2::new(x, above)],
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
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(x_of(from), above - 2.0),
                        Pos2::new(x_of(end), above - 0.5),
                    ),
                    1.0,
                    egui::Color32::from_gray(200).gamma_multiply(0.45),
                );
            }
        }
        // The real time: a thin mark across the line.
        if (start..=end).contains(&now) {
            let x = x_of(now);
            painter.line_segment(
                [
                    Pos2::new(x, line.top() - NOW_MARK_HALF),
                    Pos2::new(x, line.top() + NOW_MARK_HALF),
                ],
                egui::Stroke::new(1.5, t.text),
            );
        }
        // The window of events between its handle and the time's.
        let time_x = x_of(unix);
        let window = self.time.window_secs().filter(|secs| *secs > 0.0);
        let window_x = window.map(|secs| x_of(unix - secs));
        if let Some(window_x) = window_x {
            // Over the line, see-through: day and night still show under it.
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(window_x, line.top() - LINE_HALF - 1.0),
                    Pos2::new(time_x, line.top() + LINE_HALF + 1.0),
                ),
                2.0,
                t.accent.gamma_multiply(0.45),
            );
            painter.circle_filled(
                Pos2::new(window_x, line.top()),
                WINDOW_HANDLE_RADIUS,
                t.pane,
            );
            painter.circle_stroke(
                Pos2::new(window_x, line.top()),
                WINDOW_HANDLE_RADIUS - 1.0,
                egui::Stroke::new(2.0, t.accent),
            );
        }
        let lit = self.layer(MapLayer::Sunlight);
        sun_handle(
            &painter,
            &t,
            Pos2::new(time_x, line.top()),
            lit.then(|| sun_cosine(place, unix)),
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
                _ => ("time.set", json!({ "time": time })),
            });
            // Held on the track: it does not move under the pointer.
            kept_end = kept_end.map(|_| end);
        }
        if let Some((from, to)) = looped {
            // The loop's row: its stretch and a handle at each end, under the track.
            let y = rect.top() + LOOP_Y;
            let (from_x, to_x) = (x_of(from), x_of(to));
            painter.line_segment(
                [Pos2::new(line.left(), y), Pos2::new(line.right(), y)],
                egui::Stroke::new(2.0, t.text_dim.gamma_multiply(0.25)),
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(from_x, y - 2.0), Pos2::new(to_x, y + 2.0)),
                2.0,
                t.accent.gamma_multiply(0.6),
            );
            for (at, other, id) in [(from_x, to, "loop_from"), (to_x, from, "loop_to")] {
                let handle = Rect::from_center_size(Pos2::new(at, y), LOOP_HANDLE);
                let held = ui.interact(handle.expand(3.0), response.id.with(id), Sense::drag());
                painter.rect_filled(handle, 2.0, t.pane);
                painter.rect_stroke(
                    handle,
                    2.0,
                    egui::Stroke::new(2.0, t.accent),
                    egui::StrokeKind::Inside,
                );
                if held.dragged()
                    && let Some(p) = held.interact_pointer_pos()
                {
                    *run = Some(("time.loop", json!({"from": time_at(p.x), "to": other})));
                    kept_end = kept_end.map(|_| end);
                }
                held.on_hover_text(lang.text("timeline-loop-track-help"))
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            }
        }
        // The time under the pointer, beside it.
        if let Some(at) = response.hover_pos() {
            let time = time_at(at.x);
            response.on_hover_text_at_pointer(format!(
                "{} {}",
                date_utc(time + zone),
                &format_utc(time + zone)[11..16]
            ));
        }
        kept_end
    }
}

/// The start of the day `unix` falls in, in a zone `zone` seconds ahead of UTC.
pub(super) fn local_midnight(unix: f64, zone: f64) -> f64 {
    ((unix + zone) / DAY_SECS).floor() * DAY_SECS - zone
}

/// The cosine of the sun's angle from the vertical at `place` (a unit vector) at `unix`.
pub(super) fn sun_cosine(place: glam::DVec3, unix: f64) -> f64 {
    let (lat, lon) = globecraft_core::math::subsolar(unix);
    globecraft_core::math::ll_to_v3(lat, lon).dot(place)
}

/// The track's colour where the sun's cosine is `cosine`: night blue, a warm glow
/// through twilight and at the horizon, pale gold by day.
fn daylight_colour(cosine: f64) -> egui::Color32 {
    let mix = |a: [u8; 3], b: [u8; 3], t: f64| {
        let t = t.clamp(0.0, 1.0);
        let [r, g, b] = std::array::from_fn(|i| {
            (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * t).round() as u8
        });
        egui::Color32::from_rgb(r, g, b)
    };
    if cosine < 0.0 {
        mix(NIGHT_COLOUR, HORIZON_COLOUR, 1.0 + cosine / TWILIGHT_COSINE)
    } else {
        mix(HORIZON_COLOUR, DAY_COLOUR, cosine / LOW_SUN_COSINE)
    }
}

/// The time's handle: round, with the sun as it stands at the place (rising or
/// setting on the horizon, up, or the moon at night) while the sun lights the globe.
fn sun_handle(painter: &egui::Painter, t: &Tokens, at: Pos2, cosine: Option<f64>) {
    painter.circle_filled(at, HANDLE_RADIUS, t.pane);
    painter.circle_stroke(at, HANDLE_RADIUS - 1.0, egui::Stroke::new(2.0, t.accent));
    let Some(cosine) = cosine else {
        painter.circle_filled(at, 3.0, t.accent);
        return;
    };
    let sun = egui::Color32::from_rgb(245, 180, 60);
    if cosine < SUN_SET_COSINE {
        // A crescent: a disc with a pane-coloured one over its side.
        painter.circle_filled(at, 4.5, egui::Color32::from_rgb(200, 210, 235));
        painter.circle_filled(at + vec2(2.2, -1.6), 4.0, t.pane);
        return;
    }
    let low = cosine < LOW_SUN_COSINE;
    // On the horizon only its upper half shows, over a line.
    let centre = if low { at + vec2(0.0, 2.5) } else { at };
    for ray in 0..8 {
        let angle = (ray as f32 * 45.0).to_radians();
        let direction = vec2(angle.cos(), -angle.sin());
        if low && direction.y > 0.1 {
            continue;
        }
        painter.line_segment(
            [centre + direction * 5.0, centre + direction * 7.5],
            egui::Stroke::new(1.3, sun),
        );
    }
    if low {
        let half: Vec<Pos2> = (0..=12)
            .map(|step| {
                let angle = std::f32::consts::PI * step as f32 / 12.0;
                centre + vec2(angle.cos(), -angle.sin()) * 3.6
            })
            .collect();
        painter.add(egui::Shape::convex_polygon(half, sun, egui::Stroke::NONE));
        painter.line_segment(
            [centre + vec2(-6.5, 0.8), centre + vec2(6.5, 0.8)],
            egui::Stroke::new(1.3, t.text),
        );
    } else {
        painter.circle_filled(centre, 3.6, sun);
    }
}

/// A new loop around `unix`: an eighth of the track's span before and after it.
pub(super) fn loop_around(unix: f64, span: f64) -> (f64, f64) {
    let half = span / 8.0;
    (unix - half, unix + half)
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

/// The ticks between `start` and `end`: every few hours or every day from midnight
/// UTC, and on the first of each month or of each year across longer spans (fixed
/// 30- and 365-day steps drifted off them).
fn tick_times(start: f64, end: f64, span: f64) -> Vec<f64> {
    let days = span / DAY_SECS;
    if days <= 62.0 {
        let tick = tick_secs(span);
        let first = (start / tick).ceil() * tick;
        return (0..)
            .map(|i| first + f64::from(i) * tick)
            .take_while(|at| *at <= end)
            .collect();
    }
    let months_apart = if days <= 400.0 { 1 } else { 12 };
    let (mut year, month, _) = civil_from_days((start / DAY_SECS).floor() as i64);
    // The first tick's month: the next month (or year) to begin after `start`.
    let mut month = i64::from(month);
    if months_apart == 12 {
        month = 1;
        year += 1;
    }
    let mut ticks = Vec::new();
    loop {
        let at = days_from_civil(year, month, 1) as f64 * DAY_SECS;
        if at > end {
            return ticks;
        }
        if at >= start {
            ticks.push(at);
        }
        month += months_apart;
        if month > 12 {
            (year, month) = (year + (month - 1) / 12, (month - 1) % 12 + 1);
        }
    }
}

/// One step back or forward on the timeline from `unix`: the track's next tick that
/// way (the 1st of a month or year across long spans, as the ticks), with the ticks in a
/// time zone `zone` seconds ahead of UTC.
pub(super) fn step_to(unix: f64, span: f64, forward: bool, zone: f64) -> f64 {
    /// A time this close to a tick is on it.
    const ON_TICK_SECS: f64 = 1.0;
    let ticks = tick_times(unix - span + zone, unix + span + zone, span)
        .into_iter()
        .map(|tick| tick - zone);
    let next = if forward {
        ticks.into_iter().find(|t| *t > unix + ON_TICK_SECS)
    } else {
        ticks.rev().find(|t| *t < unix - ON_TICK_SECS)
    };
    next.unwrap_or(unix)
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
    fn long_spans_tick_on_the_first_of_the_month_or_year() {
        let at = |date: &str| parse_iso(date).unwrap();
        let ticks = tick_times(at("2026-01-15"), at("2026-05-20"), 365.0 * DAY_SECS);
        let dates: Vec<String> = ticks.iter().map(|t| date_utc(*t)).collect();
        assert_eq!(
            dates,
            ["2026-02-01", "2026-03-01", "2026-04-01", "2026-05-01"]
        );
        let ticks = tick_times(at("2019-06-01"), at("2022-02-01"), 3650.0 * DAY_SECS);
        let dates: Vec<String> = ticks.iter().map(|t| date_utc(*t)).collect();
        assert_eq!(dates, ["2020-01-01", "2021-01-01", "2022-01-01"]);
        // Across December into January.
        let ticks = tick_times(at("2025-11-20"), at("2026-02-10"), 365.0 * DAY_SECS);
        let dates: Vec<String> = ticks.iter().map(|t| date_utc(*t)).collect();
        assert_eq!(dates, ["2025-12-01", "2026-01-01", "2026-02-01"]);
        // A week ticks every day at midnight.
        let ticks = tick_times(
            at("2026-03-01T12:00"),
            at("2026-03-04T12:00"),
            7.0 * DAY_SECS,
        );
        assert_eq!(ticks.len(), 3);
        assert_eq!(date_utc(ticks[0]), "2026-03-02");
    }

    #[test]
    fn the_steps_go_from_tick_to_tick() {
        let at = |date: &str| parse_iso(date).unwrap();
        let year = 365.0 * DAY_SECS;
        assert_eq!(
            date_utc(step_to(at("2026-03-15"), year, true, 0.0)),
            "2026-04-01"
        );
        assert_eq!(
            date_utc(step_to(at("2026-03-15"), year, false, 0.0)),
            "2026-03-01"
        );
        // From a tick to the one beyond.
        assert_eq!(
            date_utc(step_to(at("2026-03-01"), year, false, 0.0)),
            "2026-02-01"
        );
        let day = DAY_SECS;
        assert_eq!(
            format_utc(step_to(at("2026-03-15T10:20"), day, true, 0.0)),
            "2026-03-15 12:00 UTC"
        );
    }

    #[test]
    fn ticks_grow_with_the_span() {
        assert_eq!(tick_secs(DAY_SECS), 3.0 * HOUR_SECS);
        assert_eq!(tick_secs(7.0 * DAY_SECS), DAY_SECS);
        assert!(tick_secs(3650.0 * DAY_SECS) > tick_secs(365.0 * DAY_SECS));
    }
}
