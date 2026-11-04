//! The timeline (roadmap 4.H): a bar along the bottom of the globe with the app's date
//! and time, steps of an hour and a day, play at a speed, back to now, a time-of-day
//! slider and the window of past events timed points show. Every control runs a
//! `time.*` command.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use crate::app::time::{SPEEDS, WINDOWS};
use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Ui, vec2};
use globecraft_core::time::{DAY_SECS, HOUR_SECS, format_utc};
use serde_json::{Value, json};

/// Above the scale bar and the credits line.
const BOTTOM_INSET: f32 = 40.0;
const ICON: f32 = 24.0;
/// Narrower globes leave out the time-of-day slider.
const SLIDER_MIN_GLOBE_WIDTH: f32 = 720.0;

impl App {
    pub(in crate::app) fn timeline_bar(&mut self, ctx: &egui::Context, globe: Rect) {
        if !self.timeline_open {
            return;
        }
        let lang = self.lang;
        let mut run: Option<(&'static str, Value)> = None;
        let unix = self.scene_unix();
        let live = self.time.is_live();
        let playing = self.time.speed() != 0.0;
        let with_slider = globe.width() >= SLIDER_MIN_GLOBE_WIDTH;
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
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                            // Date and time, or Live.
                            let text = format_utc(unix);
                            ui.label(egui::RichText::new(text).font(semibold(13.0)).color(t.text));
                            if live {
                                live_badge(ui, &t, lang.text("timeline-live"));
                            }
                            ui.separator();
                            for (icon, hours, key) in [
                                ("time_first", -24.0, "timeline-day-back-help"),
                                ("time_prev", -1.0, "timeline-hour-back-help"),
                            ] {
                                if icon_button(ui, &t, icon, lang.text(key), false) {
                                    run = Some(("time.step", json!({"hours": hours})));
                                }
                            }
                            let (icon, id) = if playing {
                                ("time_pause", "time.pause")
                            } else {
                                ("run", "time.play")
                            };
                            let help = lang.text(if playing {
                                "cmd-time-pause"
                            } else {
                                "cmd-time-play"
                            });
                            if icon_button(ui, &t, icon, help, playing) {
                                run = Some((id, Value::Null));
                            }
                            for (icon, hours, key) in [
                                ("time_next", 1.0, "timeline-hour-forward-help"),
                                ("time_last", 24.0, "timeline-day-forward-help"),
                            ] {
                                if icon_button(ui, &t, icon, lang.text(key), false) {
                                    run = Some(("time.step", json!({"hours": hours})));
                                }
                            }
                            // Speed.
                            let speed_key = SPEEDS
                                .iter()
                                .find(|(s, _)| *s == self.timeline_speed)
                                .map_or("timeline-speed-hour", |(_, k)| k);
                            ui.menu_button(lang.text(speed_key), |ui| {
                                ui.label(
                                    egui::RichText::new(lang.text("timeline-speed-label"))
                                        .color(t.text_dim),
                                );
                                for (speed, key) in SPEEDS {
                                    let on = speed == self.timeline_speed;
                                    if ui.radio(on, lang.text(key)).clicked() {
                                        run = Some((
                                            "time.play",
                                            json!({"hours_per_second": speed / HOUR_SECS}),
                                        ));
                                        ui.close();
                                    }
                                }
                            })
                            .response
                            .on_hover_text(lang.text("timeline-speed-label"));
                            if ui
                                .add_enabled(
                                    !live,
                                    egui::Button::new(lang.text("timeline-now-action")),
                                )
                                .on_hover_text(lang.text("timeline-now-help"))
                                .clicked()
                            {
                                run = Some(("time.set", json!({"time": "now"})));
                            }
                            if with_slider {
                                ui.separator();
                                let day = (unix / DAY_SECS).floor() * DAY_SECS;
                                let mut hours = (unix - day) / HOUR_SECS;
                                let slider =
                                    egui::Slider::new(&mut hours, 0.0..=24.0).show_value(false);
                                if ui
                                    .add_sized(vec2(160.0, ICON), slider)
                                    .on_hover_text(lang.text("timeline-time-of-day-help"))
                                    .changed()
                                {
                                    let time = day + (hours * HOUR_SECS).min(DAY_SECS - 60.0);
                                    run = Some(("time.set", json!({"time": time})));
                                }
                            }
                            ui.separator();
                            // Event window.
                            let window_hours =
                                self.time.window_secs().map_or(0.0, |s| s / HOUR_SECS);
                            let window_key = WINDOWS
                                .iter()
                                .find(|(h, _)| *h == window_hours)
                                .map_or("timeline-window-all", |(_, k)| k);
                            let label = format!(
                                "{}: {}",
                                lang.text("timeline-window-label"),
                                lang.text(window_key)
                            );
                            ui.menu_button(label, |ui| {
                                for (hours, key) in WINDOWS {
                                    if ui.radio(hours == window_hours, lang.text(key)).clicked() {
                                        run = Some(("time.window", json!({"hours": hours})));
                                        ui.close();
                                    }
                                }
                            })
                            .response
                            .on_hover_text(lang.text("cmd-time-window-help"));
                            if icon_button(ui, &t, "close", lang.text("cmd-ui-timeline"), false) {
                                run = Some(("ui.timeline", json!({"visible": false})));
                            }
                        });
                    });
            });
        if let Some((id, params)) = run {
            self.run_reporting(id, params);
            ctx.request_repaint();
        }
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

fn live_badge(ui: &mut Ui, t: &Tokens, text: &str) {
    let font = regular(11.0);
    let w = super::widgets::text_w(ui, text, font.clone()) + 18.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    ui.painter()
        .rect_filled(r, CornerRadius::same(9), t.accent.gamma_multiply(0.18));
    ui.painter().circle_filled(
        Pos2::new(r.left() + 8.0, r.center().y),
        3.0,
        egui::Color32::from_rgb(230, 60, 60),
    );
    ui.painter().text(
        Pos2::new(r.left() + 14.0, r.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        t.text,
    );
}
