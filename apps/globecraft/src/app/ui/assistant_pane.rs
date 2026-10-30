//! The AI Assistant pane: a chat with a model of the user's choosing, which acts on the globe by
//! running GlobeCraft commands (see the `globecraft-assistant` crate).
//!
//! The pane's layout, approval card and Markdown renderer follow RustGIS's assistant pane
//! (`crates/ui-egui/src/assistantpane.rs`, Apache-2.0); see CREDITS.md.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use super::widgets::{icon_button, primary_button};
use crate::app::App;
use eframe::egui::{self, Align, Layout, RichText, Ui, vec2};
use globecraft_assistant::{Entry, PRESETS, provider};
use globecraft_core::language::Lang;

const INPUT_HEIGHT: f32 = 44.0;
const INPUT_MAX_HEIGHT: f32 = 160.0;
/// Example requests offered in an empty conversation (Fluent keys).
const SUGGESTIONS: [&str; 5] = [
    "assistant-suggestion-fly",
    "assistant-suggestion-layers",
    "assistant-suggestion-measure",
    "assistant-suggestion-placemark",
    "assistant-suggestion-moon",
];

/// A transcript note, translated where the catalog knows its template.
fn note_text(lang: Lang, note: &provider::Note) -> String {
    let Some(key) = note_key(note.template) else {
        return note.to_string();
    };
    let args: Vec<_> = note
        .args
        .iter()
        .map(|(name, value)| (*name, value.as_str().into()))
        .collect();
    if args.is_empty() {
        lang.text(key).to_string()
    } else {
        lang.format(key, &args)
    }
}

/// The catalog key for every English template in `provider::NOTE_TEMPLATES`.
fn note_key(template: &str) -> Option<&'static str> {
    Some(match template {
        "The reply reached its length limit." => "assistant-note-length",
        "The provider's content filter stopped the reply." => "assistant-note-filter",
        "The reply stopped early ({reason})." => "assistant-note-stopped-early",
        "The model declined this request." => "assistant-note-declined",
        "The model declined this request: {why}" => "assistant-note-declined-reason",
        "The reply stopped before completion; no actions were run." => "assistant-note-incomplete",
        "Gemini stopped the reply ({reason})." => "assistant-note-gemini-stopped",
        "Summarising the earlier conversation to keep it within the model's context…" => {
            "assistant-note-summarising"
        }
        "Compacted the earlier conversation." => "assistant-note-compacted",
        "The provider returned HTTP {status}; trying again in {seconds} s." => {
            "assistant-note-http-retry"
        }
        "The connection failed; trying again in {seconds} s." => "assistant-note-connection-retry",
        "The model sent an empty reply; ask again, or try another model." => "assistant-note-empty",
        "Stopped after {steps} steps; say \"continue\" to go on." => "assistant-note-step-limit",
        "Stopped." => "assistant-note-stopped",
        _ => return None,
    })
}

impl App {
    pub(in crate::app) fn assistant_pane(&mut self, ui: &mut Ui, t: &Tokens) {
        self.ensure_agent();
        let lang = self.lang;
        let busy = self.assistant.busy();
        // The model and buttons at the top.
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (label, model) = self
                        .assistant
                        .agent
                        .as_ref()
                        .map(|a| {
                            (
                                provider::preset(&a.config.provider).map_or("", |p| p.label),
                                a.config.model.clone(),
                            )
                        })
                        .unwrap_or_default();
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint(ui.painter(), r, "assistant", t.text_dim, t.accent);
                    let w = (ui.available_width() - 3.0 * 30.0).max(0.0);
                    let model_label = egui::Label::new(
                        RichText::new(format!("{label} · {model}"))
                            .small()
                            .color(t.text_dim),
                    )
                    .truncate()
                    .sense(egui::Sense::click());
                    if ui
                        .allocate_ui(vec2(w, 16.0), |ui| ui.add(model_label))
                        .inner
                        .on_hover_text(lang.text("assistant-model-tooltip"))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.assistant.show_settings = !self.assistant.show_settings;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, "settings", lang.text("assistant-settings-title"))
                            .clicked()
                        {
                            self.assistant.show_settings = !self.assistant.show_settings;
                        }
                        let has_chat = self
                            .assistant
                            .agent
                            .as_ref()
                            .is_some_and(|a| !a.transcript.is_empty());
                        if has_chat
                            && icon_button(ui, "delete", lang.text("assistant-new-chat")).clicked()
                        {
                            self.assistant_clear();
                        }
                        if has_chat
                            && !busy
                            && icon_button(ui, "history", lang.text("assistant-compact-tooltip"))
                                .clicked()
                        {
                            self.assistant_compact();
                        }
                    });
                });
            });
        // The composer first, so tall settings scroll above it instead of under it.
        egui::Panel::bottom("assistant_composer")
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(t.pane)
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(ui, |ui| self.assistant_composer(ui, t, busy));
        if self.assistant.show_settings {
            egui::ScrollArea::vertical()
                .id_salt("assistant_settings_scroll")
                .auto_shrink([false, true])
                .max_height((ui.available_height() * 0.6).max(0.0))
                .show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(12, 4))
                        .show(ui, |ui| self.assistant_settings(ui, t));
                });
            ui.separator();
        }
        egui::ScrollArea::vertical()
            .id_salt("assistant_transcript")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        self.assistant_transcript(ui, t);
                    });
            });
    }

    fn assistant_settings(&mut self, ui: &mut Ui, t: &Tokens) {
        let lang = self.lang;
        let Some(mut cfg) = self.assistant.agent.as_ref().map(|a| a.config.clone()) else {
            return;
        };
        let before = cfg.clone();
        let remember_before = self.settings.assistant.remember_keys;
        ui.label(RichText::new(lang.text("assistant-provider-section")).font(semibold(12.5)));
        egui::Grid::new("assistant_settings_grid")
            .num_columns(2)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                ui.label(lang.text("assistant-provider-label"));
                let current = provider::preset(&cfg.provider).map_or("", |p| p.label);
                let mut chosen = None;
                egui::ComboBox::from_id_salt("assistant_provider")
                    .selected_text(current)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for p in PRESETS {
                            if ui.selectable_label(p.id == cfg.provider, p.label).clicked() {
                                chosen = Some(p.id);
                            }
                        }
                    });
                if let Some(id) = chosen.filter(|id| *id != cfg.provider) {
                    cfg = self.assistant_config(id);
                }
                ui.end_row();
                ui.label(lang.text("assistant-model-label"));
                ui.horizontal(|ui| {
                    let models = self.assistant.models.clone();
                    ui.add(egui::TextEdit::singleline(&mut cfg.model).desired_width(140.0));
                    if !models.is_empty() {
                        egui::ComboBox::from_id_salt("assistant_models")
                            .selected_text("")
                            .width(24.0)
                            .height(320.0)
                            .show_ui(ui, |ui| {
                                for m in &models {
                                    if ui.selectable_label(*m == cfg.model, m).clicked() {
                                        cfg.model = m.clone();
                                    }
                                }
                            });
                    }
                    let listing = self.assistant.listing_models();
                    if ui
                        .add_enabled(
                            !listing,
                            egui::Button::new(lang.text("assistant-list-models")).small(),
                        )
                        .clicked()
                    {
                        self.assistant_list_models();
                    }
                    if listing {
                        ui.spinner();
                    }
                });
                ui.end_row();
                ui.label(lang.text("assistant-address-label"));
                ui.add(egui::TextEdit::singleline(&mut cfg.base_url).desired_width(220.0));
                ui.end_row();
                let keyless = provider::preset(&cfg.provider).is_some_and(|p| p.keyless);
                ui.label(lang.text("assistant-key-label"));
                ui.add(
                    egui::TextEdit::singleline(&mut cfg.key)
                        .password(true)
                        .desired_width(220.0)
                        .hint_text(if keyless {
                            lang.text("assistant-key-optional")
                        } else {
                            ""
                        }),
                );
                ui.end_row();
            });
        if let Some(e) = &self.assistant.models_error {
            ui.label(RichText::new(e).small().color(t.red));
        }
        if let Some(url) = provider::preset(&cfg.provider)
            .map(|p| p.key_url)
            .filter(|u| !u.is_empty())
        {
            ui.hyperlink_to(RichText::new(lang.text("assistant-get-key")).small(), url);
        }
        ui.checkbox(
            &mut self.settings.assistant.remember_keys,
            lang.text("assistant-remember-key"),
        );
        ui.label(
            RichText::new(lang.text("assistant-privacy-note"))
                .small()
                .color(t.text_dim),
        );
        if let Some(why) = cfg.missing() {
            ui.label(RichText::new(why).small().color(t.amber));
        }
        if self.settings.assistant.remember_keys != remember_before {
            self.save_assistant_keys();
        }
        if cfg != before {
            self.apply_assistant_config(cfg);
        }
    }

    fn assistant_composer(&mut self, ui: &mut Ui, t: &Tokens, busy: bool) {
        let lang = self.lang;
        // An action waiting for approval comes first.
        let pending = self.assistant.agent.as_ref().and_then(|a| a.pending());
        if let Some((what, why, can_always)) = pending {
            egui::Frame::NONE
                .fill(t.amber.gamma_multiply(0.15))
                .corner_radius(6)
                .inner_margin(8)
                .stroke(egui::Stroke::new(1.0, t.amber))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(lang.text("assistant-approve-title")).font(semibold(12.5)),
                    );
                    ui.label(
                        RichText::new(
                            lang.format("assistant-approve-reason", &[("why", why.into())]),
                        )
                        .small(),
                    );
                    ui.label(RichText::new(what).monospace().small());
                    if can_always {
                        ui.checkbox(
                            &mut self.assistant.always,
                            lang.text("assistant-approve-always"),
                        );
                    } else {
                        self.assistant.always = false;
                    }
                    ui.horizontal(|ui| {
                        let allow = primary_button(
                            ui,
                            lang.text("assistant-allow"),
                            true,
                            vec2(70.0, 26.0),
                        )
                        .clicked();
                        let deny = ui.button(lang.text("assistant-decline")).clicked();
                        if allow || deny {
                            self.assistant_decide(allow);
                        }
                    });
                });
            ui.add_space(6.0);
        }
        let id = egui::Id::new("assistant_input");
        let focused = ui.memory(|m| m.has_focus(id));
        let mut submit = false;
        if focused && !busy {
            // Enter sends; Shift+Enter starts a new line.
            submit = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            // Up and Down step through earlier messages while the box holds one of them.
            let shown = self
                .assistant
                .history_pos
                .and_then(|p| self.assistant.history.get(p))
                .is_some_and(|h| *h == self.assistant.input);
            if self.assistant.input.is_empty() || shown {
                let n = self.assistant.history.len();
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                    && n > 0
                {
                    let p = self
                        .assistant
                        .history_pos
                        .map_or(n - 1, |p| p.saturating_sub(1));
                    self.assistant.history_pos = Some(p);
                    self.assistant.input = self.assistant.history[p].clone();
                } else if ui
                    .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
                    && let Some(p) = self.assistant.history_pos
                {
                    self.assistant.history_pos = (p + 1 < n).then_some(p + 1);
                    self.assistant.input = self
                        .assistant
                        .history_pos
                        .and_then(|p| self.assistant.history.get(p))
                        .cloned()
                        .unwrap_or_default();
                }
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("assistant_input_scroll")
            .max_height(INPUT_MAX_HEIGHT)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.assistant.input)
                        .id(id)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .min_size(vec2(0.0, INPUT_HEIGHT))
                        .font(regular(13.0))
                        .hint_text(lang.text("assistant-input-hint")),
                );
            });
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 84.0).max(0.0);
            ui.allocate_ui(vec2(w, 20.0), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(lang.text("assistant-input-help"))
                            .small()
                            .color(t.text_dim),
                    )
                    .truncate(),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if busy {
                    if ui.button(lang.text("assistant-stop")).clicked() {
                        self.assistant_stop();
                    }
                } else {
                    let can = !self.assistant.input.trim().is_empty();
                    if primary_button(ui, lang.text("assistant-send"), can, vec2(64.0, 26.0))
                        .clicked()
                    {
                        submit = true;
                    }
                }
            });
        });
        if submit && !busy {
            let text = self.assistant.input.clone();
            self.assistant_send(text);
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    fn assistant_transcript(&mut self, ui: &mut Ui, t: &Tokens) {
        let lang = self.lang;
        let Some(agent) = self.assistant.agent.as_ref() else {
            return;
        };
        if agent.transcript.is_empty() {
            ui.label(RichText::new(lang.text("assistant-intro")).color(t.text_dim));
            ui.add_space(8.0);
            let mut chosen = None;
            for key in SUGGESTIONS {
                let b = egui::Button::new(RichText::new(lang.text(key)).small()).corner_radius(12);
                if ui.add(b).clicked() {
                    chosen = Some(lang.text(key).to_string());
                }
            }
            if let Some(text) = chosen {
                self.assistant_send(text);
            }
            return;
        }
        for e in &agent.transcript {
            match e {
                Entry::User(text) => {
                    ui.add_space(6.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        egui::Frame::NONE
                            .fill(t.selection)
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.set_max_width(ui.available_width() * 0.85);
                                ui.add(
                                    egui::Label::new(RichText::new(text).color(t.text))
                                        .halign(Align::LEFT),
                                );
                            });
                    });
                    ui.add_space(4.0);
                }
                Entry::Assistant(text) => {
                    markdown(ui, text, t);
                    ui.add_space(4.0);
                }
                Entry::Tool {
                    name,
                    summary,
                    result,
                    error,
                } => {
                    let (mark, col) = if *error {
                        ("✗", t.red)
                    } else {
                        ("✓", t.green)
                    };
                    let head = format!("{mark} {}", summary.chars().take(90).collect::<String>());
                    let room = (ui.available_width()
                        - ui.spacing().indent
                        - ui.spacing().button_padding.x)
                        .max(0.0);
                    let head =
                        egui::WidgetText::from(RichText::new(head).monospace().small().color(col))
                            .into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                room,
                                egui::TextStyle::Button,
                            );
                    egui::CollapsingHeader::new(head)
                        .id_salt(("assistant_tool", name, summary.as_str(), result.len()))
                        .show(ui, |ui| {
                            ui.label(RichText::new(summary).monospace().small());
                            ui.label(
                                RichText::new(result.chars().take(2000).collect::<String>())
                                    .monospace()
                                    .small()
                                    .color(t.text_dim),
                            );
                        });
                }
                Entry::Error(e) => {
                    egui::Frame::NONE
                        .fill(t.red.gamma_multiply(0.12))
                        .corner_radius(6)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(e.to_string()).small().color(t.red));
                        });
                }
                Entry::Note(n) => {
                    ui.label(
                        RichText::new(note_text(lang, n))
                            .small()
                            .italics()
                            .color(t.text_dim),
                    );
                }
            }
        }
        if agent.waiting() {
            if let Some(text) = agent.preview() {
                markdown(ui, text, t);
                ui.spinner();
                return;
            }
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new(lang.text("assistant-thinking"))
                        .small()
                        .color(t.text_dim),
                );
            });
        }
    }
}

// A small Markdown renderer: headings, lists, quotes, code blocks, tables (as text), **bold**,
// *italic*, `code` and links (shown as text). Model output is text, never markup to run.

pub(in crate::app) fn markdown(ui: &mut Ui, text: &str, t: &Tokens) {
    let mut code: Option<Vec<&str>> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match code.take() {
                Some(lines) => code_block(ui, &lines.join("\n"), t),
                None => code = Some(Vec::new()),
            }
            continue;
        }
        if let Some(lines) = code.as_mut() {
            lines.push(line);
            continue;
        }
        let trimmed = line.trim_start();
        let indent = ((line.len() - trimmed.len()) as f32 * 4.0).min(48.0);
        if trimmed.is_empty() {
            ui.add_space(4.0);
        } else if let Some(h) = ["### ", "## ", "# "]
            .iter()
            .find_map(|p| trimmed.strip_prefix(p))
        {
            ui.add_space(2.0);
            ui.label(inline(ui, h, t, 14.0, true));
        } else if let Some(item) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|p| trimmed.strip_prefix(p))
        {
            bullet(ui, indent, "•", item, t);
        } else if let Some((num, item)) = numbered(trimmed) {
            bullet(ui, indent, &format!("{num}."), item, t);
        } else if let Some(q) = trimmed.strip_prefix("> ") {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("▎").color(t.border_strong));
                ui.label(inline(ui, q, t, 13.0, false));
            });
        } else if trimmed.starts_with('|') {
            if !trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                let cells: Vec<&str> = trimmed
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                ui.label(RichText::new(cells.join("  │  ")).monospace().small());
            }
        } else {
            ui.label(inline(ui, trimmed, t, 13.0, false));
        }
    }
    if let Some(lines) = code {
        code_block(ui, &lines.join("\n"), t);
    }
}

fn numbered(s: &str) -> Option<(&str, &str)> {
    let (num, rest) = s.split_once(". ")?;
    (!num.is_empty() && num.len() <= 3 && num.chars().all(|c| c.is_ascii_digit()))
        .then_some((num, rest))
}

fn bullet(ui: &mut Ui, indent: f32, mark: &str, item: &str, t: &Tokens) {
    ui.horizontal_top(|ui| {
        ui.add_space(4.0 + indent);
        ui.label(RichText::new(mark).color(t.text_dim));
        ui.label(inline(ui, item, t, 13.0, false));
    });
}

fn code_block(ui: &mut Ui, code: &str, t: &Tokens) {
    egui::Frame::NONE
        .fill(t.input)
        .stroke(egui::Stroke::new(1.0, t.border))
        .corner_radius(4)
        .inner_margin(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(code).monospace().small());
        });
}

/// A line with **bold**, *italic*, `code` and [links](url) (as their text) laid out.
fn inline(ui: &Ui, text: &str, t: &Tokens, size: f32, heading: bool) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let base = |bold: bool, italic: bool, code: bool| {
        let font = if code {
            egui::FontId::monospace(size - 1.0)
        } else if bold || heading {
            semibold(size)
        } else {
            regular(size)
        };
        TextFormat {
            font_id: font,
            color: if code { t.accent_text } else { t.text },
            italics: italic,
            background: if code {
                t.input
            } else {
                egui::Color32::TRANSPARENT
            },
            ..Default::default()
        }
    };
    let (mut bold, mut italic) = (false, false);
    let mut buf = String::new();
    let flush = |job: &mut LayoutJob, buf: &mut String, bold: bool, italic: bool| {
        if !buf.is_empty() {
            job.append(buf, 0.0, base(bold, italic, false));
            buf.clear();
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("**").or_else(|| rest.strip_prefix("__")) {
            flush(&mut job, &mut buf, bold, italic);
            bold = !bold;
            rest = r;
        } else if c == '`'
            && let Some(end) = rest.get(1..).and_then(|r| r.find('`'))
        {
            flush(&mut job, &mut buf, bold, italic);
            job.append(
                rest.get(1..1 + end).unwrap_or(""),
                0.0,
                base(bold, italic, true),
            );
            rest = rest.get(end + 2..).unwrap_or("");
        } else if c == '*' && rest.get(1..).is_some_and(|r| !r.starts_with(' ')) {
            flush(&mut job, &mut buf, bold, italic);
            italic = !italic;
            rest = rest.get(1..).unwrap_or("");
        } else if c == '['
            && let Some(close) = rest.find("](")
            && let Some(end) = rest.get(close..).and_then(|r| r.find(')'))
        {
            buf.push_str(rest.get(1..close).unwrap_or(""));
            rest = rest.get(close + end + 1..).unwrap_or("");
        } else {
            buf.push(c);
            rest = rest.get(c.len_utf8()..).unwrap_or("");
        }
    }
    flush(&mut job, &mut buf, bold, italic);
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A note with every `{placeholder}` filled, so translated templates that
    /// need values can be formatted without knowing each template's arguments.
    fn filled_note(template: &'static str) -> provider::Note {
        let mut args = Vec::new();
        let mut rest = template;
        while let Some((_, tail)) = rest.split_once('{') {
            let Some((name, after)) = tail.split_once('}') else {
                break;
            };
            args.push((
                name.trim().trim_start_matches('$').trim(),
                "example".to_owned(),
            ));
            rest = after;
        }
        provider::Note { template, args }
    }

    #[test]
    fn every_assistant_note_template_has_a_usable_translation() {
        for template in provider::NOTE_TEMPLATES.iter().copied() {
            let key = note_key(template).unwrap_or_else(|| panic!("missing key for {template}"));
            let note = filled_note(template);
            for lang in Lang::ALL {
                let text = note_text(lang, &note);
                assert_ne!(text, key, "{} lacks {key}", lang.code());
                assert!(
                    !text.contains('{'),
                    "{} left placeholders in {key}: {text}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn parameterized_notes_fill_the_translated_message() {
        let stopped = provider::Note::with(
            "The reply stopped early ({reason}).",
            [("reason", "safety".to_owned())],
        );
        assert_eq!(
            note_text(Lang::En, &stopped),
            "The reply stopped early (safety)."
        );
        assert_eq!(
            note_text(Lang::PtBr, &stopped),
            "A resposta parou cedo (safety)."
        );

        let declined = provider::Note::with(
            "The model declined this request: {why}",
            [("why", "policy".to_owned())],
        );
        assert_eq!(
            note_text(Lang::En, &declined),
            "The model declined this request: policy"
        );
    }
}
