//! Ribbon and pane widgets: large and small command buttons, menu buttons, groups and columns.
//! Each command control runs its command by id and greys out when the command is unavailable.
//!
//! Metrics and painting follow RustGIS's ribbon widgets (`crates/ui-egui/src/widgets.rs`,
//! Apache-2.0); see CREDITS.md.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use eframe::egui::{self, Align2, CornerRadius, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::commands;
use serde_json::Value;
use std::sync::Arc;

/// Height of the ribbon's control area (without group labels).
pub(in crate::app) const CONTENT_H: f32 = 72.0;
pub(in crate::app) const LABEL_H: f32 = 18.0;

pub(in crate::app) fn text_w(ui: &Ui, s: &str, font: egui::FontId) -> f32 {
    galley(ui, s, font, egui::Color32::WHITE).size().x
}

/// One laid-out line, to measure a label and then paint it without laying it out twice.
fn galley(ui: &Ui, s: &str, font: egui::FontId, color: egui::Color32) -> Arc<egui::Galley> {
    ui.fonts_mut(|f| f.layout_no_wrap(s.to_owned(), font, color))
}

/// How a command control looks; `None` fields come from the command catalog.
#[derive(Default)]
pub(in crate::app) struct Look<'a> {
    pub icon: Option<&'a str>,
    pub label: Option<&'a str>,
    pub help: Option<&'a str>,
    /// Checked state; defaults to the command's own (toggles and selections).
    pub checked: Option<bool>,
}

/// A control's text and state for this frame. Borrowed: controls are drawn every frame, so
/// nothing is allocated until a tooltip or menu actually needs it.
struct Resolved<'a> {
    icon: &'a str,
    label: &'a str,
    help: &'a str,
    checked: bool,
    enabled: bool,
    /// Command whose shortcut the tooltip shows; empty for plain menus.
    command: &'a str,
}

fn resolve<'a>(app: &App, id: &'a str, params: &Value, look: &Look<'a>) -> Resolved<'a> {
    let spec = commands::find(id);
    let lang = app.lang;
    let state = app.command_state(id, params);
    Resolved {
        icon: look.icon.or(spec.map(|s| s.icon)).unwrap_or_default(),
        label: look
            .label
            .or_else(|| spec.map(|s| lang.text(s.label)))
            .unwrap_or(id),
        help: look
            .help
            .or_else(|| spec.map(|s| lang.text(s.help)))
            .unwrap_or_default(),
        checked: look.checked.unwrap_or(state.checked),
        enabled: state.enabled,
        command: id,
    }
}

/// A rich tooltip: title (with the command's shortcut) and description. Line breaks of
/// two-line ribbon labels become spaces.
fn tooltip(resp: Response, title: &str, desc: &str, command: &str) -> Response {
    resp.on_hover_ui(|ui| {
        ui.set_max_width(280.0);
        let shortcut = if command.is_empty() {
            String::new()
        } else {
            crate::app::commands::shortcut_text(command)
        };
        let title = title.replace('\n', " ");
        let head = if shortcut.is_empty() {
            title
        } else {
            format!("{title}  ({shortcut})")
        };
        ui.label(egui::RichText::new(head).font(semibold(12.5)));
        if !desc.is_empty() {
            ui.label(egui::RichText::new(desc).size(11.5).weak());
        }
    })
}

fn bg(ui: &Ui, r: Rect, resp: &Response, checked: bool, t: &Tokens) {
    let fill = if resp.is_pointer_button_down_on() {
        t.pressed
    } else if checked {
        t.checked
    } else if resp.hovered() {
        t.hover
    } else {
        return;
    };
    ui.painter().rect_filled(r, CornerRadius::same(6), fill);
    if checked {
        ui.painter().rect_stroke(
            r,
            CornerRadius::same(6),
            Stroke::new(1.0, t.accent.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
    }
}

/// A small downward caret marking a menu.
pub(in crate::app) fn caret(p: &egui::Painter, c: egui::Pos2, col: egui::Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![
            c + vec2(-3.5, -1.8),
            c + vec2(3.5, -1.8),
            c + vec2(0.0, 2.2),
        ],
        col,
        Stroke::NONE,
    ));
}

/// Paints a large button: a 32 px icon over a one- or two-line label.
fn paint_big(ui: &mut Ui, r: &Resolved, menu: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let on = r.enabled || menu;
    let col = if on { t.text } else { t.text_disabled };
    // At most two lines: a label is short, and a second line holds its remainder.
    let mut lines = r
        .label
        .splitn(2, '\n')
        .map(|l| galley(ui, l, regular(11.5), col));
    let first = lines.next();
    let second = lines.next();
    let lw = first
        .iter()
        .chain(second.iter())
        .map(|g| g.size().x + if menu { 9.0 } else { 0.0 })
        .fold(0.0, f32::max);
    let w = (lw + 14.0).max(50.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, CONTENT_H - 2.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            on,
            r.checked,
            r.label.replace('\n', " "),
        )
    });
    let line_count = 1 + usize::from(second.is_some());
    bg(ui, rect, &resp, r.checked, &t);
    let ic = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 21.0), vec2(32.0, 32.0));
    let (c, a) = if on {
        (t.icon, t.accent)
    } else {
        (t.text_disabled, t.text_disabled)
    };
    icons::paint(ui.painter(), ic, r.icon, c, a);
    let mut y = rect.min.y + 42.0;
    for (i, g) in first.into_iter().chain(second).enumerate() {
        let tw = g.size().x;
        if menu && i + 1 == line_count {
            let x0 = rect.center().x - (tw + 9.0) / 2.0;
            caret(ui.painter(), pos2(x0 + tw + 5.0, y + 7.5), col);
            ui.painter().galley(pos2(x0, y), g, col);
        } else {
            ui.painter()
                .galley(pos2(rect.center().x - tw / 2.0, y), g, col);
        }
        y += 13.5;
    }
    tooltip(resp, r.label, r.help, r.command)
}

/// A large command button. Returns the response; a click runs the command.
pub(in crate::app) fn big(ui: &mut Ui, app: &mut App, id: &str, params: Value) -> Response {
    big_with(ui, app, id, params, Look::default())
}

pub(in crate::app) fn big_with(
    ui: &mut Ui,
    app: &mut App,
    id: &str,
    params: Value,
    look: Look<'_>,
) -> Response {
    let r = resolve(app, id, &params, &look);
    let resp = paint_big(ui, &r, false);
    if resp.clicked() && r.enabled {
        app.run_reporting(id, params);
    }
    resp
}

/// A large button that opens a menu.
pub(in crate::app) fn big_menu(
    ui: &mut Ui,
    app: &mut App,
    icon: &str,
    label: &str,
    help: &str,
    menu: impl FnOnce(&mut Ui, &mut App),
) {
    let r = Resolved {
        icon,
        label,
        help,
        checked: false,
        enabled: true,
        command: "",
    };
    let resp = paint_big(ui, &r, true);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(200.0);
        menu(ui, app);
    });
}

/// Paints a small button: an 18 px icon with an optional label.
fn paint_small(ui: &mut Ui, r: &Resolved, show_label: bool, menu: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let on = r.enabled || menu;
    let text = if on { t.text } else { t.text_disabled };
    let label = show_label.then(|| galley(ui, r.label, regular(11.5), text));
    let tw = label
        .as_ref()
        .map_or(0.0, |g| g.size().x + 8.0 + if menu { 12.0 } else { 0.0 });
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0 + tw, 22.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, on, r.checked, r.label)
    });
    bg(ui, rect, &resp, r.checked, &t);
    let ic = Rect::from_center_size(pos2(rect.min.x + 13.0, rect.center().y), vec2(18.0, 18.0));
    let (c, a) = if on {
        (t.icon, t.accent)
    } else {
        (t.text_disabled, t.text_disabled)
    };
    icons::paint(ui.painter(), ic, r.icon, c, a);
    if let Some(g) = label {
        let top = rect.center().y - g.size().y / 2.0;
        ui.painter().galley(pos2(rect.min.x + 26.0, top), g, text);
    }
    if menu {
        caret(
            ui.painter(),
            pos2(rect.max.x - 9.0, rect.center().y),
            t.text,
        );
    }
    tooltip(resp, r.label, r.help, r.command)
}

/// A small command button with its label.
pub(in crate::app) fn small(ui: &mut Ui, app: &mut App, id: &str, params: Value) -> Response {
    small_with(ui, app, id, params, Look::default())
}

pub(in crate::app) fn small_with(
    ui: &mut Ui,
    app: &mut App,
    id: &str,
    params: Value,
    look: Look<'_>,
) -> Response {
    let r = resolve(app, id, &params, &look);
    let resp = paint_small(ui, &r, true, false);
    if resp.clicked() && r.enabled {
        app.run_reporting(id, params);
    }
    resp
}

/// A small icon-only command button (the Quick Access Toolbar).
pub(in crate::app) fn icon_command(
    ui: &mut Ui,
    app: &mut App,
    id: &str,
    params: Value,
) -> Response {
    let r = resolve(app, id, &params, &Look::default());
    let resp = paint_small(ui, &r, false, false);
    if resp.clicked() && r.enabled {
        app.run_reporting(id, params);
    }
    resp
}

/// A small button that opens a menu.
pub(in crate::app) fn small_menu(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    help: &str,
    menu: impl FnOnce(&mut Ui),
) {
    let r = Resolved {
        icon,
        label,
        help,
        checked: false,
        enabled: true,
        command: "",
    };
    let resp = paint_small(ui, &r, true, true);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(200.0);
        menu(ui);
    });
}

/// A frameless button as a row of a menu or list, with a hover fill under it.
pub(in crate::app) fn row_button(ui: &mut Ui, on: bool, button: egui::Button<'_>) -> Response {
    let under = ui.painter().add(egui::Shape::Noop);
    let resp = ui.add_enabled(on, button);
    if on && (resp.hovered() || resp.is_pointer_button_down_on()) {
        let t = Tokens::get(ui.ctx());
        let fill = if resp.is_pointer_button_down_on() {
            t.pressed
        } else {
            t.hover
        };
        ui.painter().set(
            under,
            egui::Shape::rect_filled(
                resp.rect.expand2(vec2(4.0, 0.0)),
                CornerRadius::same(5),
                fill,
            ),
        );
    }
    resp
}

const MENU_ICON_W: f32 = 22.0;

/// A menu row that runs a command (disabled when the command is), with its icon and shortcut.
pub(in crate::app) fn menu_item(ui: &mut Ui, app: &mut App, id: &str, params: Value) {
    menu_item_with(ui, app, id, params, Look::default());
}

pub(in crate::app) fn menu_item_with(
    ui: &mut Ui,
    app: &mut App,
    id: &str,
    params: Value,
    look: Look<'_>,
) {
    let r = resolve(app, id, &params, &look);
    let mut button = egui::Button::new(menu_text(ui, r.icon, r.label))
        .frame(false)
        .selected(r.checked)
        .min_size(vec2(0.0, 24.0));
    let shortcut = crate::app::commands::shortcut_text(id);
    if !shortcut.is_empty() {
        button = button.right_text(shortcut);
    }
    let resp = row_button(ui, r.enabled, button);
    menu_icon(ui, &resp, r.icon);
    let resp = if r.help.is_empty() {
        resp
    } else {
        resp.on_hover_text(r.help)
    };
    if resp.clicked() {
        app.run_reporting(id, params);
        ui.close();
    }
}

fn menu_text(ui: &Ui, icon: &str, label: &str) -> egui::WidgetText {
    if icon.is_empty() {
        return label.into();
    }
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Button.resolve(ui.style());
    job.append(
        label,
        MENU_ICON_W,
        egui::TextFormat::simple(font, egui::Color32::PLACEHOLDER),
    );
    job.into()
}

fn menu_icon(ui: &Ui, resp: &Response, icon: &str) {
    if icon.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            resp.rect.left_center()
                + vec2(ui.spacing().button_padding.x + MENU_ICON_W / 2.0 - 2.0, 0.0),
            vec2(16.0, 16.0),
        ),
        icon,
        t.icon,
        t.accent,
    );
}

/// A ribbon group: controls laid out left to right over a centred label, with a separator.
pub(in crate::app) fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    let start = ui.cursor().min;
    let label = galley(ui, title, regular(11.0), t.group_label);
    let label_w = label.size().x;
    let inner = ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
        let r = ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.set_min_height(CONTENT_H);
            ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
            add(ui);
        });
        let w = r.response.rect.width().max(label_w + 12.0);
        ui.allocate_exact_size(vec2(w, LABEL_H), Sense::hover());
        w
    });
    let w = inner.inner;
    let label_pos = pos2(
        start.x + (w - label_w) / 2.0,
        start.y + CONTENT_H + LABEL_H / 2.0 - 1.0 - label.size().y / 2.0,
    );
    ui.painter().galley(label_pos, label, t.group_label);
    ui.add_space(6.0);
    let x = ui.cursor().min.x;
    ui.painter().line_segment(
        [
            pos2(x, start.y + 6.0),
            pos2(x, start.y + CONTENT_H + LABEL_H - 4.0),
        ],
        Stroke::new(1.0, t.border),
    );
    ui.add_space(7.0);
}

/// A column of up to three small controls.
pub(in crate::app) fn col(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        ui.add_space(1.0);
        add(ui);
    });
}

/// A filled action button (Send, Allow…), with its label centred on the capital letters.
pub(in crate::app) fn primary_button(
    ui: &mut Ui,
    label: &str,
    enabled: bool,
    min_size: egui::Vec2,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = semibold(13.0);
    let size = vec2(
        min_size.x.max(text_w(ui, label, font.clone()) + 28.0),
        min_size.y.max(28.0),
    );
    let (r, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let fill = if !enabled {
        t.accent.gamma_multiply(0.45)
    } else if resp.is_pointer_button_down_on() {
        t.accent.gamma_multiply(0.8)
    } else if resp.hovered() {
        t.accent.gamma_multiply(0.9)
    } else {
        t.accent
    };
    ui.painter().rect_filled(r, CornerRadius::same(6), fill);
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        egui::Color32::WHITE,
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A small icon button (close, settings…), drawn with the app's own icons.
pub(in crate::app) fn icon_button(ui: &mut Ui, icon: &str, hover: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, hover));
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    ui.painter().rect_stroke(
        r,
        CornerRadius::same(5),
        Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    icons::paint(ui.painter(), r.shrink(5.0), icon, t.text, t.accent);
    resp.on_hover_text(hover)
}

/// A section heading in a pane.
pub(in crate::app) fn section_heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).font(semibold(12.5)).color(t.text));
}
