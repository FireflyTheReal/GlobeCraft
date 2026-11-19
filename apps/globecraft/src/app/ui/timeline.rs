//! The timeline (roadmap 4.H, C4), laid out as ArcGIS Map Viewer's time slider: a
//! round play button, previous and next steps, then the track (`timeline_track`) with
//! the span's start and end dates at its ends, the app's time (and the window of
//! events) above its middle and dates under its ticks, and a settings menu (back to
//! now, speed, window of events, span) and a close button at the right. Every control
//! runs a `time.*` command.
use super::icons;
use super::theme::Tokens;
use crate::app::App;
use crate::app::time::{RANGES, SPEEDS, WINDOWS};
use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Ui, vec2};
use globecraft_core::time::{DAY_SECS, HOUR_SECS};
use serde_json::{Value, json};

/// Above the scale bar and the credits line.
const BOTTOM_INSET: f32 = 40.0;
const ICON: f32 = 24.0;
/// The play button, round, as Map Viewer's.
const PLAY: f32 = 34.0;
/// The bar spans the globe less this on each side, within these widths.
const SIDE_MARGIN: f32 = 24.0;
const MIN_WIDTH: f32 = 480.0;
const MAX_WIDTH: f32 = 980.0;
/// Room the buttons left and right of the track take.
const LEFT_CONTROLS: f32 = PLAY + 2.0 * ICON + 24.0;
const RIGHT_CONTROLS: f32 = 2.0 * ICON + 16.0;

impl App {
    pub(in crate::app) fn timeline_bar(&mut self, ctx: &egui::Context, globe: Rect) {
        if !self.timeline.open {
            return;
        }
        let lang = self.lang;
        let mut run: Option<(&'static str, Value)> = None;
        let unix = self.scene_unix();
        let live = self.time.is_live();
        let playing = self.time.speed() != 0.0;
        let width = (globe.width() - 2.0 * SIDE_MARGIN).clamp(MIN_WIDTH, MAX_WIDTH);
        let mut track_end = self.timeline.track_end;
        egui::Area::new(egui::Id::new("timeline_bar"))
            .order(egui::Order::Middle)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(Pos2::new(globe.center().x, globe.bottom() - BOTTOM_INSET))
            .constrain_to(globe)
            .show(ctx, |ui| {
                let t = Tokens::get(ui.ctx());
                egui::Frame::popup(ui.style())
                    .fill(t.pane)
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
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
                            // One step back and forward: to the track's next tick.
                            let span = self.timeline.range_days * DAY_SECS;
                            for (icon, forward, key) in [
                                ("time_prev", false, "timeline-step-back-help"),
                                ("time_next", true, "timeline-step-forward-help"),
                            ] {
                                if icon_button(ui, &t, icon, lang.text(key), false) {
                                    let to = super::timeline_track::step_to(unix, span, forward);
                                    run = Some(("time.set", json!({ "time": to })));
                                }
                            }
                            ui.add_space(8.0);
                            let track_w = width - LEFT_CONTROLS - RIGHT_CONTROLS;
                            track_end = self.timeline_track(ui, track_w, unix, live, &mut run);
                            ui.add_space(4.0);
                            self.timeline_settings(ui, &t, live, &mut run);
                            if icon_button(ui, &t, "close", lang.text("cmd-ui-timeline"), false) {
                                run = Some(("ui.timeline", json!({"on": false})));
                            }
                        });
                    });
            });
        self.timeline.track_end = track_end;
        if let Some((id, params)) = run {
            // A changed time renders like a moving camera until it settles.
            self.touch();
            self.run_reporting(id, params);
            ctx.request_repaint();
        }
    }

    /// The settings menu at the bar's right: back to now, speed, window of events, span.
    fn timeline_settings(
        &self,
        ui: &mut Ui,
        t: &Tokens,
        live: bool,
        run: &mut Option<(&'static str, Value)>,
    ) {
        let lang = self.lang;
        let (rect, response) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), rect.shrink(4.0), "settings", t.icon, t.accent);
        let response = response.on_hover_text(lang.text("timeline-settings-help"));
        egui::Popup::menu(&response).show(|ui| {
            if ui
                .add_enabled(!live, egui::Button::new(lang.text("timeline-now-action")))
                .on_hover_text(lang.text("timeline-now-help"))
                .clicked()
            {
                *run = Some(("time.set", json!({"time": "now"})));
                ui.close();
            }
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
            ui.label(egui::RichText::new(lang.text("timeline-window-label")).color(t.text_dim));
            let window_hours = self.time.window_secs().map_or(0.0, |s| s / HOUR_SECS);
            for (hours, key) in WINDOWS {
                if ui.radio(hours == window_hours, lang.text(key)).clicked() {
                    *run = Some(("time.window", json!({ "hours": hours })));
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
        });
    }
}

fn icon_button(ui: &mut Ui, t: &Tokens, icon: &str, help: &str, checked: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, help));
    if checked {
        ui.painter()
            .rect_filled(r, CornerRadius::same(5), t.checked);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(4.0), icon, t.icon, t.accent);
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
        Rect::from_center_size(centre, vec2(ICON - 6.0, ICON - 6.0)),
        icon,
        t.accent,
        t.accent,
    );
    resp.on_hover_text(help).clicked()
}
