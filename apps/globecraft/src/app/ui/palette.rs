//! Command Search (Ctrl+K): find any command by name and run it, or hand the words to the AI
//! assistant. Laid out like RustGIS's command search (`crates/ui-egui/src/chrome.rs`, Apache-2.0).
use super::icons;
use super::theme::{Tokens, regular};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, pos2, vec2};
use globecraft_core::{commands, language::fold_name, layers::MapLayer};
use serde_json::{Value, json};

const MAX_HITS: usize = 40;
const ROW_H: f32 = 30.0;

#[derive(Default)]
pub(in crate::app) struct CommandSearch {
    query: String,
    selected: usize,
}

/// A runnable entry: a command with fixed parameters.
struct Hit {
    id: &'static str,
    params: Value,
    label: String,
    detail: String,
    icon: &'static str,
}

fn hits(app: &App, query: &str) -> Vec<Hit> {
    let lang = app.lang;
    let q = fold_name(query.trim());
    let matches = |texts: &[&str]| q.is_empty() || texts.iter().any(|t| fold_name(t).contains(&q));
    let mut out = Vec::new();
    for spec in commands::COMMANDS {
        let schema: Value = serde_json::from_str(spec.params).unwrap_or(Value::Null);
        // Commands needing values the palette can't ask for are reached through their buttons.
        let needs_values = schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|r| !r.is_empty());
        if needs_values || spec.id.starts_with("info.") || spec.id == "camera.set" {
            continue;
        }
        let label = lang.text(spec.label);
        let help = lang.text(spec.help);
        if matches(&[label, help, spec.id]) {
            out.push(Hit {
                id: spec.id,
                params: Value::Null,
                label: label.to_string(),
                detail: lang.text(spec.tab.label_key()).to_string(),
                icon: spec.icon,
            });
        }
    }
    // Every layer, by name.
    for layer in MapLayer::ALL.into_iter().filter(|layer| layer.listed()) {
        let label = lang.text(layer.label_key());
        if matches(&[label, layer.code()]) {
            let on = app.layer(layer);
            out.push(Hit {
                id: "layer.show",
                params: json!({"layer": layer.code()}),
                label: label.to_string(),
                detail: lang
                    .text(if on {
                        "palette-layer-hide"
                    } else {
                        "palette-layer-show"
                    })
                    .to_string(),
                icon: layer.icon(),
            });
        }
    }
    // Basemaps and worlds.
    for provider in globecraft_data::tiles::Provider::basemaps() {
        let label = provider.name(lang);
        if matches(&[label, provider.code()]) {
            out.push(Hit {
                id: "layer.basemap",
                params: json!({"basemap": provider.code()}),
                label: label.to_string(),
                detail: lang.text("cmd-layer-basemap").to_string(),
                icon: "basemap",
            });
        }
    }
    for (body, key, icon) in [
        ("earth", "top-earth", "earth"),
        ("moon", "top-moon", "moon"),
        ("custom", "top-custom-planet", "planet"),
    ] {
        let label = lang.text(key);
        if matches(&[label, body]) {
            out.push(Hit {
                id: "world.body",
                params: json!({"body": body}),
                label: label.to_string(),
                detail: lang.text("cmd-world-body").to_string(),
                icon,
            });
        }
    }
    out.truncate(MAX_HITS);
    out
}

impl App {
    pub(in crate::app) fn command_search_window(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.command_search.take() else {
            return;
        };
        let t = Tokens::get(ctx);
        let lang = self.lang;
        let mut keep = true;
        let mut run: Option<(&'static str, Value)> = None;
        let mut ask: Option<String> = None;
        let screen = ctx.content_rect();
        let width = (screen.width() - 48.0).clamp(240.0, 560.0);
        let list_h = (screen.height() - 190.0).clamp(120.0, 380.0);
        let found = hits(self, &state.query);
        state.selected = state.selected.min(found.len().saturating_sub(1));
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if escape {
            keep = false;
        }
        if down && !found.is_empty() {
            state.selected = (state.selected + 1) % found.len();
        }
        if up && !found.is_empty() {
            state.selected = (state.selected + found.len() - 1) % found.len();
        }
        if enter {
            match found.get(state.selected) {
                Some(hit) => run = Some((hit.id, hit.params.clone())),
                None if !state.query.trim().is_empty() => {
                    ask = Some(state.query.trim().to_string())
                }
                None => {}
            }
        }
        let modal = egui::Modal::new(egui::Id::new("command_search")).show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, "search", t.text_dim, t.accent);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text(lang.text("palette-hint"))
                        .desired_width(width - 30.0)
                        .frame(egui::Frame::NONE),
                );
                edit.request_focus();
                if edit.changed() {
                    state.selected = 0;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(list_h)
                .show(ui, |ui| {
                    if found.is_empty() {
                        ui.label(egui::RichText::new(lang.text("palette-empty")).color(t.text_dim));
                    }
                    for (i, hit) in found.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                        resp.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &hit.label)
                        });
                        if resp.hovered() {
                            state.selected = i;
                        }
                        if i == state.selected {
                            ui.painter().rect_filled(r, CornerRadius::same(6), t.hover);
                            if up || down {
                                resp.scroll_to_me(None);
                            }
                        }
                        icons::paint(
                            ui.painter(),
                            Rect::from_center_size(
                                pos2(r.min.x + 16.0, r.center().y),
                                vec2(18.0, 18.0),
                            ),
                            hit.icon,
                            t.icon,
                            t.accent,
                        );
                        ui.painter().text(
                            pos2(r.min.x + 34.0, r.center().y),
                            Align2::LEFT_CENTER,
                            &hit.label,
                            regular(13.0),
                            t.text,
                        );
                        ui.painter().text(
                            pos2(r.max.x - 10.0, r.center().y),
                            Align2::RIGHT_CENTER,
                            &hit.detail,
                            regular(11.0),
                            t.text_dim,
                        );
                        if resp.clicked() {
                            run = Some((hit.id, hit.params.clone()));
                        }
                    }
                    if !state.query.trim().is_empty() {
                        ui.separator();
                        let text = lang.format(
                            "palette-ask-assistant",
                            &[("question", state.query.trim().to_string().into())],
                        );
                        if super::widgets::row_button(
                            ui,
                            true,
                            egui::Button::new(egui::RichText::new(text).color(t.accent_text))
                                .frame(false),
                        )
                        .clicked()
                        {
                            ask = Some(state.query.trim().to_string());
                        }
                    }
                });
        });
        if modal.should_close() {
            keep = false;
        }
        if let Some((id, params)) = run {
            keep = false;
            self.run_reporting(id, params);
        }
        if let Some(question) = ask {
            keep = false;
            self.run_reporting("ui.pane.assistant", json!({"ask": question}));
        }
        if keep {
            self.command_search = Some(state);
        }
    }
}
