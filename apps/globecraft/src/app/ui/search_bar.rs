//! The place search field in the header; query state and services live in `app::search`.
use super::icons;
use super::theme::Tokens;
use crate::app::App;
use eframe::egui::{self, Key, Rect, vec2};

pub(in crate::app) const SEARCH_FIELD_ID: &str = "place-search";

impl App {
    /// Draws the field (frameless: the header paints its rounded box) and returns its rect.
    pub(super) fn search_bar(&mut self, ui: &mut egui::Ui) -> Rect {
        let lg = self.lang;
        let t = Tokens::get(ui.ctx());
        // Keys for the suggestion list must be consumed before the TextEdit sees them.
        let popup = self.search.suggestions_open && !self.search.suggestions.is_empty();
        let (mut down, mut up, mut esc) = (false, false, false);
        if popup {
            ui.input_mut(|i| {
                down = i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                up = i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                esc = i.consume_key(egui::Modifiers::NONE, Key::Escape);
            });
            let n = self.search.suggestions.len();
            if down {
                self.search.selected = Some(self.search.selected.map_or(0, |i| (i + 1) % n));
            }
            if up {
                self.search.selected =
                    Some(self.search.selected.map_or(n - 1, |i| (i + n - 1) % n));
            }
            if esc {
                self.search.suggestions_open = false;
            }
        }
        let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
        icons::paint(ui.painter(), icon, "search", t.text_dim, t.accent);
        let id = egui::Id::new(SEARCH_FIELD_ID);
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.search.query)
                .id(id)
                .frame(egui::Frame::NONE)
                .hint_text(lg.text("search-input-placeholder"))
                .desired_width(ui.available_width()),
        );
        if std::mem::take(&mut self.search.focus) {
            r.request_focus();
        }
        if r.changed() {
            self.on_query_changed();
        }
        if r.gained_focus()
            && !self.search.query.trim().is_empty()
            && !self.search.suggestions.is_empty()
        {
            self.search.suggestions_open = true;
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if enter {
            match (self.search.suggestions_open, self.search.selected) {
                (true, Some(i)) => self.choose_suggestion(i),
                _ => {
                    self.search.suggestions_open = false;
                    self.search.fly = true;
                    self.start_search(ui);
                }
            }
        }
        r.rect
    }
}
