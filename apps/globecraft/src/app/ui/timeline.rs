//! The timeline (roadmap 4.H, C4), one rounded bar over the globe's bottom after Bing
//! Maps' sun slider (Julien, 2026-11-21: "make it more intuitive/simplistic like the
//! reference"): the sun on or off, play, the track (`timeline_track`: the place's own
//! day by default, its line coloured by day and night there, the handle holding the
//! sun as it stands), the date (a calendar), the time in the zone of the place in view
//! with "Now" while away from the real time, the less used settings in one menu
//! (steps, loop, speed, window of events, span, hide), and a chevron that folds the bar
//! down to the sun, play and the time. Every control runs a command, so the menu,
//! the keyboard, the assistant and the bar move the same time.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use crate::app::time::{RANGES, SPEEDS, WINDOWS};
use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Ui, vec2};
use globecraft_core::layers::MapLayer;
use globecraft_core::time::{DAY_SECS, HOUR_SECS, civil_from_days, days_from_civil, format_utc};
use serde_json::{Value, json};

/// Above the scale bar and the credits line.
const BOTTOM_INSET: f32 = 40.0;
const ICON: f32 = 24.0;
/// The play button, round.
const PLAY: f32 = 30.0;
/// The bar spans the globe less this on each side, within these widths.
const SIDE_MARGIN: f32 = 24.0;
const MIN_WIDTH: f32 = 460.0;
const MAX_WIDTH: f32 = 900.0;
/// Room the controls left and right of the track take (the date and time are
/// measured).
const LEFT_CONTROLS: f32 = ICON + PLAY + 16.0;
const RIGHT_CONTROLS: f32 = 2.0 * ICON + 20.0;
/// The calendar's day cells, and its icon on the date button.
const DAY_CELL: f32 = 26.0;
const CALENDAR_ICON: f32 = 15.0;

impl App {
    /// The timeline over the globe's bottom, when it is open: where it is drawn.
    pub(in crate::app) fn timeline_bar(
        &mut self,
        ctx: &egui::Context,
        globe: Rect,
    ) -> Option<Rect> {
        if !self.timeline.open {
            return None;
        }
        let lang = self.lang;
        let mut run: Option<(&'static str, Value)> = None;
        let unix = self.scene_unix();
        let (zone, zone_name) = self.place_zone(unix);
        let live = self.time.is_live();
        let playing = self.time.speed() != 0.0;
        let folded = self.timeline.folded;
        let width = (globe.width() - 2.0 * SIDE_MARGIN).clamp(MIN_WIDTH, MAX_WIDTH);
        let mut track_end = self.timeline.track_end;
        let bar = egui::Area::new(egui::Id::new("timeline_bar"))
            .order(egui::Order::Middle)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(Pos2::new(globe.center().x, globe.bottom() - BOTTOM_INSET))
            .constrain_to(globe)
            .show(ctx, |ui| {
                let t = Tokens::get(ui.ctx());
                egui::Frame::popup(ui.style())
                    .fill(t.pane)
                    .corner_radius(24.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                            let lit = self.layer(MapLayer::Sunlight);
                            let sun_help = lang.text("timeline-sun-help");
                            if icon_button(ui, &t, "sun", sun_help, lit) {
                                run = Some(("layer.show", json!({"layer": "sunlight"})));
                            }
                            let help = lang.text(if playing {
                                "cmd-time-pause"
                            } else {
                                "cmd-time-play"
                            });
                            if play_button(ui, &t, playing, help) {
                                run = Some(if playing {
                                    ("time.pause", Value::Null)
                                } else {
                                    ("time.play", Value::Null)
                                });
                            }
                            let date = format_utc(unix + zone)[..10].to_owned();
                            let clock = format_utc(unix + zone)[11..16].to_owned();
                            if !folded {
                                let right = RIGHT_CONTROLS
                                    + super::widgets::text_w(ui, &date, regular(12.5))
                                    + super::widgets::text_w(ui, "00:00 XXXX", semibold(13.0))
                                    + if live { 0.0 } else { 44.0 }
                                    + ICON
                                    + 24.0;
                                let track_w = width - LEFT_CONTROLS - right;
                                ui.add_space(4.0);
                                track_end =
                                    self.timeline_track(ui, track_w, (unix, zone), &mut run);
                                ui.add_space(4.0);
                                self.date_button(ui, &t, &date, (unix, zone), &mut run);
                            }
                            ui.label(
                                egui::RichText::new(clock)
                                    .font(semibold(13.0))
                                    .color(t.text),
                            )
                            .on_hover_text(lang.text("timeline-zone-help"));
                            ui.label(
                                egui::RichText::new(zone_name)
                                    .font(regular(11.5))
                                    .color(t.text_dim),
                            );
                            if !live
                                && ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(lang.text("timeline-now-action"))
                                                .color(t.accent),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(lang.text("timeline-now-help"))
                                    .clicked()
                            {
                                run = Some(("time.set", json!({"time": "now"})));
                            }
                            if !folded {
                                self.timeline_settings(ui, &t, (unix, zone), &mut run);
                            }
                            let (icon, key) = if folded {
                                ("chevron_right", "timeline-unfold-help")
                            } else {
                                ("chevron_left", "timeline-fold-help")
                            };
                            if icon_button(ui, &t, icon, lang.text(key), false) {
                                self.timeline.folded = !folded;
                            }
                        });
                    });
            })
            .response
            .rect;
        self.timeline.track_end = track_end;
        if let Some((id, params)) = run {
            // A changed time renders like a moving camera until it settles.
            self.touch();
            self.run_reporting(id, params);
            ctx.request_repaint();
        }
        Some(bar)
    }

    /// The date with a calendar icon; a click opens a month to pick a day from, the
    /// time of day kept.
    fn date_button(
        &self,
        ui: &mut Ui,
        t: &Tokens,
        date: &str,
        (unix, zone): (f64, f64),
        run: &mut Option<(&'static str, Value)>,
    ) {
        let font = regular(12.5);
        let text_w = super::widgets::text_w(ui, date, font.clone());
        let (rect, response) =
            ui.allocate_exact_size(vec2(CALENDAR_ICON + 10.0 + text_w, ICON), Sense::click());
        let help = self.lang.text("timeline-date-help");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, help));
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(12), t.hover);
        }
        let icon = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.center().y - CALENDAR_ICON / 2.0),
            vec2(CALENDAR_ICON, CALENDAR_ICON),
        );
        paint_calendar(ui.painter(), icon, t.icon);
        ui.painter().text(
            Pos2::new(icon.right() + 5.0, rect.center().y),
            Align2::LEFT_CENTER,
            date,
            font,
            t.text,
        );
        let month = egui::Id::new("timeline_calendar_month");
        if response.clicked() {
            // Opens on the month shown.
            ui.data_mut(|data| data.remove::<(i64, i64)>(month));
        }
        let response = response.on_hover_text(help);
        egui::Popup::from_toggle_button_response(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.calendar(ui, t, (unix, zone), month, run));
    }

    /// A month of days (Monday first), the day shown marked and today ringed; a day
    /// picked moves the time to it, the time of day kept.
    fn calendar(
        &self,
        ui: &mut Ui,
        t: &Tokens,
        (unix, zone): (f64, f64),
        month_id: egui::Id,
        run: &mut Option<(&'static str, Value)>,
    ) {
        let lang = self.lang;
        let shown_day = ((unix + zone) / DAY_SECS).floor() as i64;
        let today = ((globecraft_core::math::now_unix() + zone) / DAY_SECS).floor() as i64;
        let (year, month, _) = civil_from_days(shown_day);
        let (mut year, mut month) = ui
            .data(|data| data.get_temp::<(i64, i64)>(month_id))
            .unwrap_or((year, i64::from(month)));
        ui.horizontal(|ui| {
            if ui.small_button("‹").clicked() {
                (year, month) = if month == 1 {
                    (year - 1, 12)
                } else {
                    (year, month - 1)
                };
            }
            ui.add_sized(
                vec2(7.0 * DAY_CELL - 52.0, 18.0),
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} {year}",
                        lang.text(&format!("timeline-month-{month}"))
                    ))
                    .font(semibold(13.0)),
                ),
            );
            if ui.small_button("›").clicked() {
                (year, month) = if month == 12 {
                    (year + 1, 1)
                } else {
                    (year, month + 1)
                };
            }
        });
        ui.data_mut(|data| data.insert_temp(month_id, (year, month)));
        let first = days_from_civil(year, month, 1);
        let (next_year, next_month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        let length = days_from_civil(next_year, next_month, 1) - first;
        // 1970-01-01 was a Thursday: Monday is 0.
        let lead = (first + 3).rem_euclid(7);
        let time_of_day = (unix + zone).rem_euclid(DAY_SECS);
        egui::Grid::new("timeline_calendar")
            .spacing(vec2(0.0, 2.0))
            .show(ui, |ui| {
                for name in lang.text("timeline-weekdays").split_whitespace() {
                    ui.add_sized(
                        vec2(DAY_CELL, 16.0),
                        egui::Label::new(
                            egui::RichText::new(name)
                                .font(regular(10.5))
                                .color(t.text_dim),
                        ),
                    );
                }
                ui.end_row();
                for cell in 0..42 {
                    let day = cell - lead + 1;
                    if (1..=length).contains(&day) {
                        let days = first + day - 1;
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(DAY_CELL, DAY_CELL), Sense::click());
                        let painter = ui.painter();
                        if days == shown_day {
                            painter.circle_filled(rect.center(), DAY_CELL / 2.0 - 1.0, t.accent);
                        } else if response.hovered() {
                            painter.circle_filled(rect.center(), DAY_CELL / 2.0 - 1.0, t.hover);
                        }
                        if days == today {
                            painter.circle_stroke(
                                rect.center(),
                                DAY_CELL / 2.0 - 1.5,
                                egui::Stroke::new(1.0, t.text_dim),
                            );
                        }
                        painter.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            day.to_string(),
                            regular(12.0),
                            if days == shown_day {
                                egui::Color32::WHITE
                            } else {
                                t.text
                            },
                        );
                        if response.clicked() {
                            let time = days as f64 * DAY_SECS + time_of_day - zone;
                            *run = Some(("time.set", json!({ "time": time })));
                            ui.close();
                        }
                    } else {
                        ui.allocate_exact_size(vec2(DAY_CELL, DAY_CELL), Sense::hover());
                    }
                    if cell % 7 == 6 {
                        ui.end_row();
                    }
                    if cell % 7 == 6 && day >= length {
                        break;
                    }
                }
            });
    }

    /// The settings menu: steps, loop, back to now, speed, window of events, span, and
    /// hiding the bar.
    fn timeline_settings(
        &self,
        ui: &mut Ui,
        t: &Tokens,
        (unix, zone): (f64, f64),
        run: &mut Option<(&'static str, Value)>,
    ) {
        let lang = self.lang;
        let (rect, response) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(5), t.hover);
        }
        icons::paint(
            ui.painter(),
            rect.shrink(4.0),
            "settings",
            t.icon,
            t.icon_accent,
        );
        let response = response.on_hover_text(lang.text("timeline-settings-help"));
        let span = self.timeline.range_days * DAY_SECS;
        egui::Popup::menu(&response).show(|ui| {
            // One step back and forward: to the track's next tick.
            ui.horizontal(|ui| {
                for (icon, forward, key) in [
                    ("time_prev", false, "timeline-step-back-help"),
                    ("time_next", true, "timeline-step-forward-help"),
                ] {
                    if icon_button(ui, t, icon, lang.text(key), false) {
                        let to = super::timeline_track::step_to(unix, span, forward, zone);
                        *run = Some(("time.set", json!({ "time": to })));
                    }
                }
                // The loop: on around the time, off again.
                let looped = self.time.loop_range().is_some();
                if icon_button(ui, t, "loop", lang.text("timeline-loop-help"), looped) {
                    *run = Some(if looped {
                        ("time.loop", json!({}))
                    } else {
                        let (from, to) = super::timeline_track::loop_around(unix, span);
                        ("time.loop", json!({ "from": from, "to": to }))
                    });
                }
            });
            ui.separator();
            ui.label(egui::RichText::new(lang.text("timeline-speed-label")).color(t.text_dim));
            for (speed, key) in SPEEDS {
                if ui
                    .radio(speed == self.timeline.speed, lang.text(key))
                    .clicked()
                {
                    *run = Some(("time.speed", json!({"hours_per_second": speed / HOUR_SECS})));
                    ui.close();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new(lang.text("cmd-time-range")).color(t.text_dim));
            for (days, key) in RANGES {
                if ui
                    .radio(days == self.timeline.range_days, lang.text(key))
                    .clicked()
                {
                    *run = Some(("time.range", json!({ "days": days })));
                    ui.close();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new(lang.text("timeline-window-label")).color(t.text_dim));
            let window_hours = self.time.window_secs().map_or(0.0, |s| s / HOUR_SECS);
            for (hours, key) in WINDOWS {
                if ui.radio(hours == window_hours, lang.text(key)).clicked() {
                    *run = Some(("time.window", json!({ "hours": hours })));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(lang.text("timeline-hide-action")).clicked() {
                *run = Some(("ui.timeline", json!({"on": false})));
                ui.close();
            }
        });
    }
}

fn icon_button(ui: &mut Ui, t: &Tokens, icon: &str, help: &str, checked: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, help));
    if checked {
        ui.painter()
            .rect_filled(r, CornerRadius::same(12), t.checked);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(12), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(4.0), icon, t.icon, t.icon_accent);
    resp.on_hover_text(help).clicked()
}

/// The round play (or pause) button.
fn play_button(ui: &mut Ui, t: &Tokens, playing: bool, help: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(PLAY, PLAY), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, help));
    let centre = r.center();
    let fill = if resp.hovered() { t.hover } else { t.pane };
    ui.painter().circle_filled(centre, PLAY / 2.0, fill);
    ui.painter()
        .circle_stroke(centre, PLAY / 2.0 - 1.0, egui::Stroke::new(1.5, t.accent));
    let icon = if playing { "time_pause" } else { "run" };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(centre, vec2(ICON - 8.0, ICON - 8.0)),
        icon,
        t.accent,
        t.accent,
    );
    resp.on_hover_text(help).clicked()
}

/// A calendar page: a rounded frame, its rings and a grid of days.
fn paint_calendar(painter: &egui::Painter, r: Rect, ink: egui::Color32) {
    let page = Rect::from_min_max(r.min + vec2(0.0, 2.0), r.max);
    painter.rect_filled(page, CornerRadius::same(3), ink);
    let inner = Rect::from_min_max(page.min + vec2(1.5, 4.0), page.max - vec2(1.5, 1.5));
    painter.rect_filled(
        inner,
        CornerRadius::same(1),
        egui::Color32::from_black_alpha(140),
    );
    for x in [0.3, 0.7] {
        let x = r.left() + r.width() * x;
        painter.line_segment(
            [Pos2::new(x, r.top()), Pos2::new(x, r.top() + 4.0)],
            egui::Stroke::new(1.5, ink),
        );
    }
    for row in 0..2 {
        for column in 0..3 {
            let at = Pos2::new(
                inner.left() + inner.width() * (0.2 + 0.3 * column as f32),
                inner.top() + inner.height() * (0.3 + 0.4 * row as f32),
            );
            painter.circle_filled(at, 1.0, ink);
        }
    }
}
