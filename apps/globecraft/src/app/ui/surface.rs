//! One look for everything that opens over the globe (layout redesign step 2):
//! *dialogs* for tasks with a beginning and an end (Settings, Add Data, About, Keyboard
//! Shortcuts), centred, in three widths; *cards* for one selected thing (a flight, a
//! satellite, a point), at the globe's top-left corner, one at a time; *popovers* for a
//! quick edit next to what it edits (the altitude). All share the header (title and ✕),
//! the frame, the padding and the footer row of buttons on the right.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use eframe::egui::{
    self, Align2, CornerRadius, Margin, Rect, Response, Sense, Stroke, Ui, pos2, vec2,
};
use globecraft_core::language::Lang;

/// Widths of dialogs, in points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Width {
    /// About, the altitude editor.
    Small,
    /// Keyboard shortcuts, cards.
    Medium,
    /// Settings, Add Data.
    Large,
}
impl Width {
    fn points(self) -> f32 {
        match self {
            Self::Small => 360.0,
            Self::Medium => 520.0,
            Self::Large => 760.0,
        }
    }
}

/// Where a surface sits.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) enum Place {
    /// A dialog: centred in the window.
    Centre,
    /// A card: in the top-left corner of the globe's rectangle.
    Card(Rect),
    /// A popover: its `pivot` corner at `at`.
    At { at: egui::Pos2, pivot: Align2 },
}

/// The gap between a card and the globe's edges.
pub(in crate::app) const CARD_MARGIN_POINTS: f32 = 12.0;
/// The width of a card.
pub(in crate::app) const CARD_WIDTH_POINTS: f32 = 320.0;
const HEADER_HEIGHT_POINTS: f32 = 36.0;
const PADDING_POINTS: i8 = 14;
const CORNER_RADIUS: u8 = 10;

/// A dialog, card or popover. `show` returns `false` once the user closed it (✕, or
/// Escape for dialogs and popovers).
pub(in crate::app) struct Surface<'a> {
    id: egui::Id,
    title: &'a str,
    subtitle: Option<&'a str>,
    width: f32,
    place: Place,
    height: Option<f32>,
    min_size: Option<egui::Vec2>,
}

impl<'a> Surface<'a> {
    pub(in crate::app) fn dialog(id: &str, title: &'a str, width: Width) -> Self {
        Self {
            id: egui::Id::new(id),
            title,
            subtitle: None,
            width: width.points(),
            place: Place::Centre,
            height: None,
            min_size: None,
        }
    }
    pub(in crate::app) fn card(id: &str, title: &'a str, globe: Rect) -> Self {
        Self {
            id: egui::Id::new(id),
            title,
            subtitle: None,
            width: CARD_WIDTH_POINTS,
            place: Place::Card(globe),
            height: None,
            min_size: None,
        }
    }
    pub(in crate::app) fn popover(id: &str, title: &'a str, at: egui::Pos2, pivot: Align2) -> Self {
        Self {
            id: egui::Id::new(id),
            title,
            subtitle: None,
            width: Width::Small.points(),
            place: Place::At { at, pivot },
            height: None,
            min_size: None,
        }
    }
    /// A second line under the title (a flight's route).
    pub(in crate::app) fn subtitle(mut self, subtitle: &'a str) -> Self {
        self.subtitle = Some(subtitle);
        self
    }
    /// A fixed body height (with the footer), for dialogs whose pages differ in length.
    pub(in crate::app) fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }
    /// Lets the user resize the dialog down to `min_size`.
    pub(in crate::app) fn resizable(mut self, min_size: egui::Vec2) -> Self {
        self.min_size = Some(min_size);
        self
    }

    /// Draws the surface; `body` fills it below the header. Returns whether it is still
    /// open, and `body`'s value.
    pub(in crate::app) fn show<R>(
        self,
        ctx: &egui::Context,
        lang: Lang,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> (bool, Option<R>) {
        let t = Tokens::get(ctx);
        let frame = egui::Frame::NONE
            .fill(t.pane)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS))
            .shadow(ctx.global_style().visuals.window_shadow)
            .inner_margin(Margin::same(0));
        let mut window = egui::Window::new(self.title)
            .id(self.id)
            .title_bar(false)
            .collapsible(false)
            .frame(frame);
        window = match self.min_size {
            Some(min) => window
                .resizable(true)
                .min_size(min)
                .default_size(vec2(self.width, self.height.unwrap_or(min.y))),
            None => window
                .resizable(false)
                .min_width(self.width)
                .max_width(self.width),
        };
        window = match self.place {
            Place::Centre => window.anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)),
            Place::Card(globe) => window
                .fixed_pos(globe.left_top() + vec2(CARD_MARGIN_POINTS, CARD_MARGIN_POINTS))
                .constrain_to(globe),
            Place::At { at, pivot } => window.pivot(pivot).fixed_pos(at),
        };
        let mut open = true;
        let escape_closes = !matches!(self.place, Place::Card(_));
        let response = window.show(ctx, |ui| {
            if self.min_size.is_none() {
                ui.set_width(self.width);
            }
            let close_tip = lang.text("settings-close-action");
            if header(ui, &t, self.title, self.subtitle, close_tip) {
                open = false;
            }
            egui::Frame::NONE
                .inner_margin(Margin {
                    left: PADDING_POINTS,
                    right: PADDING_POINTS,
                    top: 10,
                    bottom: PADDING_POINTS,
                })
                .show(ui, |ui| {
                    if let Some(height) = self.height
                        && self.min_size.is_none()
                    {
                        ui.set_height(height);
                    }
                    body(ui)
                })
                .inner
        });
        if escape_closes
            && !egui::Popup::is_any_open(ctx)
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            open = false;
        }
        (open, response.and_then(|r| r.inner))
    }
}

/// The header: title (and subtitle) on the left, ✕ on the right, a line under it.
/// Returns whether ✕ was clicked.
fn header(ui: &mut Ui, t: &Tokens, title: &str, subtitle: Option<&str>, close_tip: &str) -> bool {
    let height = HEADER_HEIGHT_POINTS + if subtitle.is_some() { 16.0 } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius {
            nw: CORNER_RADIUS,
            ne: CORNER_RADIUS,
            sw: 0,
            se: 0,
        },
        t.pane_header,
    );
    painter.line_segment(
        [
            pos2(rect.min.x, rect.max.y - 0.5),
            pos2(rect.max.x, rect.max.y - 0.5),
        ],
        Stroke::new(1.0, t.border),
    );
    let left = rect.min.x + f32::from(PADDING_POINTS);
    let title_y = rect.min.y + HEADER_HEIGHT_POINTS / 2.0;
    let close = Rect::from_center_size(pos2(rect.max.x - 20.0, title_y), vec2(22.0, 22.0));
    painter
        .with_clip_rect(Rect::from_min_max(
            rect.min,
            pos2(close.min.x - 6.0, rect.max.y),
        ))
        .text(
            pos2(left, title_y),
            Align2::LEFT_CENTER,
            title,
            semibold(14.0),
            t.text,
        );
    if let Some(subtitle) = subtitle {
        painter
            .with_clip_rect(Rect::from_min_max(
                rect.min,
                pos2(close.min.x - 6.0, rect.max.y),
            ))
            .text(
                pos2(left, title_y + 17.0),
                Align2::LEFT_CENTER,
                subtitle,
                regular(12.0),
                t.text_dim,
            );
    }
    let resp = close_button(ui, t, close, close_tip);
    resp.clicked()
}

fn close_button(ui: &mut Ui, t: &Tokens, rect: Rect, tip: &str) -> Response {
    let resp = ui.interact(rect, ui.id().with("surface-close"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), t.hover);
    }
    icons::paint(
        ui.painter(),
        rect.shrink(5.0),
        "close",
        t.text_dim,
        t.accent,
    );
    resp.on_hover_text(tip)
}

/// The footer of a dialog: a line, then `buttons` laid out from the right (the first is
/// the rightmost, so add the main action first).
pub(in crate::app) fn footer(ui: &mut Ui, buttons: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), buttons);
}
