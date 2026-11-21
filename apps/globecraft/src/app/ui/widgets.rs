//! Ribbon and pane widgets: large and small command buttons, menu buttons, groups and columns.
//! Each command control runs its command by id and greys out when the command is unavailable.
//!
//! Metrics and painting follow RustGIS's ribbon widgets (`crates/ui-egui/src/widgets.rs`,
//! Apache-2.0); see CREDITS.md.
use super::icons;
use super::theme::{Tokens, regular, semibold};
use crate::app::App;
use eframe::egui::{self, CornerRadius, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use globecraft_core::commands;
use serde_json::Value;
use std::sync::Arc;

/// Height of the ribbon's control area (without group labels).
pub(in crate::app) const CONTENT_H: f32 = 72.0;
/// Tall enough for the group labels' menus to be 20 pt targets.
pub(in crate::app) const LABEL_H: f32 = 21.0;
/// The compact ribbon's one row: a small button's height.
pub(in crate::app) const COMPACT_H: f32 = 22.0;
const COMPACT_KEY: &str = "ribbon-compact";

/// Whether the ribbon draws as one row of small buttons (`ui.ribbonCompact`), for the
/// widgets below that have no `App` at hand. Set by the ribbon each frame.
pub(in crate::app) fn set_compact(ctx: &egui::Context, compact: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(COMPACT_KEY), compact));
}
fn compact(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(egui::Id::new(COMPACT_KEY)).unwrap_or(false))
}

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
    /// The command's parameters: with them, it is the tool whose shortcut is shown.
    params: Option<&'a Value>,
}

fn resolve<'a>(app: &App, id: &'a str, params: &'a Value, look: &Look<'a>) -> Resolved<'a> {
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
        params: Some(params),
    }
}

/// A rich tooltip: title (with the command's shortcut) and description. Line breaks of
/// two-line ribbon labels become spaces.
fn tooltip(resp: Response, r: &Resolved) -> Response {
    let (title, desc, command) = (r.label, r.help, r.command);
    resp.on_hover_ui(|ui| {
        ui.set_max_width(280.0);
        let shortcut = if command.is_empty() {
            String::new()
        } else {
            let key = crate::app::tools::tool_key(command, r.params.unwrap_or(&Value::Null));
            crate::app::commands::tool_shortcut_text(&key)
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
    tooltip(resp, r)
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
    if !app.list_tool(id, &params, r.icon, r.label, r.enabled, r.checked) {
        return ui.response();
    }
    let resp = if compact(ui.ctx()) {
        paint_small(ui, &r, true, false)
    } else {
        paint_big(ui, &r, false)
    };
    if resp.clicked() && r.enabled {
        app.run_reporting(id, params);
    }
    resp
}

/// Where a large button's label begins below its top: with a drop-down, the label and
/// its caret open the menu.
const BIG_LABEL_TOP_POINTS: f32 = 38.0;

/// A large command button with a drop-down (the Clouds layer and its detail): its
/// label and caret open `menu`, the rest of it runs the command.
pub(in crate::app) fn big_with_menu(
    ui: &mut Ui,
    app: &mut App,
    id: &str,
    params: Value,
    look: Look<'_>,
    menu: impl FnOnce(&mut Ui, &mut App),
) -> Response {
    let r = resolve(app, id, &params, &look);
    if !app.list_tool(id, &params, r.icon, r.label, r.enabled, r.checked) {
        return ui.response();
    }
    let enabled = r.enabled;
    let resp = if compact(ui.ctx()) {
        // One row: the button runs the command, its caret opens the menu.
        let resp = paint_small(ui, &r, true, true);
        let caret =
            Rect::from_min_max(pos2(resp.rect.max.x - 16.0, resp.rect.min.y), resp.rect.max);
        let opener = ui.interact(caret, resp.id.with("menu"), Sense::click());
        egui::Popup::menu(&opener).show(|ui| menu(ui, app));
        resp
    } else {
        let resp = paint_big(ui, &r, true);
        // Over the button, so its clicks go to the menu.
        let label = Rect::from_min_max(
            pos2(resp.rect.min.x, resp.rect.min.y + BIG_LABEL_TOP_POINTS),
            resp.rect.max,
        );
        let opener = ui.interact(label, resp.id.with("menu"), Sense::click());
        egui::Popup::menu(&opener).show(|ui| menu(ui, app));
        resp
    };
    if resp.clicked() && enabled {
        app.run_reporting(id, params);
    }
    resp
}

/// One option of a choice (a basemap, a theme, a world): a command with parameters.
pub(in crate::app) struct Choice<'a> {
    pub id: &'a str,
    pub params: Value,
    pub icon: &'a str,
    pub label: &'a str,
}

/// The option of `choices` in effect, or the first.
fn current_choice<'c, 'a>(app: &App, choices: &'c [Choice<'a>]) -> Option<&'c Choice<'a>> {
    choices
        .iter()
        .find(|c| app.command_state(c.id, &c.params).checked)
        .or_else(|| choices.first())
}

/// The menu of a choice: every option, the one in effect marked.
fn choice_menu(ui: &mut Ui, app: &mut App, choices: &[Choice]) {
    ui.set_min_width(200.0);
    for choice in choices {
        menu_item_with(
            ui,
            app,
            choice.id,
            choice.params.clone(),
            Look {
                icon: Some(choice.icon),
                label: Some(choice.label),
                ..Look::default()
            },
        );
    }
}

/// A large button for a set of choices (basemap, world): it shows the one in effect and
/// opens a menu of all of them, instead of one button per option.
pub(in crate::app) fn choice_big(ui: &mut Ui, app: &mut App, help: &str, choices: &[Choice]) {
    let Some(current) = current_choice(app, choices) else {
        return;
    };
    let r = Resolved {
        icon: current.icon,
        label: current.label,
        help,
        checked: false,
        enabled: true,
        command: "",
        params: None,
    };
    let resp = if compact(ui.ctx()) {
        paint_small(ui, &r, true, true)
    } else {
        paint_big(ui, &r, true)
    };
    egui::Popup::menu(&resp).show(|ui| choice_menu(ui, app, choices));
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
    tooltip(resp, r)
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
    if !app.list_tool(id, &params, r.icon, r.label, r.enabled, r.checked) {
        return ui.response();
    }
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
    let layer = params
        .get("layer")
        .and_then(Value::as_str)
        .and_then(globecraft_core::layers::MapLayer::from_code);
    let name = (!params.is_null())
        .then(|| super::ribbon_tools::tool_name(app, &crate::app::tools::tool_key(id, &params)));
    let look = Look {
        icon: layer.map(|l| l.icon()),
        label: name.as_deref(),
        ..Look::default()
    };
    let r = resolve(app, id, &params, &look);
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
        params: None,
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

/// A ribbon group: controls laid out left to right over its title, with a separator. The
/// title opens a menu of all the group's tools (see `ribbon_tools.rs`); `id` names the group
/// in the user's pins.
pub(in crate::app) fn group(
    ui: &mut Ui,
    app: &mut App,
    id: &'static str,
    title: &str,
    add: impl FnOnce(&mut Ui, &mut App),
) {
    let t = Tokens::get(ui.ctx());
    let start = ui.cursor().min;
    if compact(ui.ctx()) {
        compact_group(ui, app, id, title, add);
        return;
    }
    let label = galley(ui, title, regular(11.0), t.group_label);
    let label_w = label.size().x + GROUP_CARET_W;
    app.chrome.ribbon_tools.begin(id);
    forget_last_column(ui.ctx());
    // Tools narrower than the title sit in its middle (the lone Earth button under
    // "Worlds" sat at the left, user 2026-11-20), from their width last frame.
    let content_key = egui::Id::new(("ribbon_group_content_w", id));
    let title_w = label_w + 12.0;
    let pad = ui
        .ctx()
        .data(|d| d.get_temp::<f32>(content_key))
        .map_or(0.0, |content| ((title_w - content) / 2.0).max(0.0).floor());
    let inner = ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
        let r = ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.set_min_height(CONTENT_H);
            ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
            if pad > 0.0 {
                ui.add_space(pad - ui.spacing().item_spacing.x);
            }
            let from = ui.cursor().min.x;
            add(ui, app);
            app.group_extras(ui, id);
            ui.min_rect().max.x - from
        });
        let content_w = r.inner;
        ui.ctx().data_mut(|d| d.insert_temp(content_key, content_w));
        let w = (content_w + 2.0 * pad).max(title_w);
        let (_, title_resp) = ui.allocate_exact_size(vec2(w, LABEL_H), Sense::hover());
        (w, title_resp.rect)
    });
    let rows = app.chrome.ribbon_tools.end();
    let (w, title_rect) = inner.inner;
    let label_pos = pos2(
        start.x + (w - label_w) / 2.0,
        start.y + CONTENT_H + LABEL_H / 2.0 - 1.0 - label.size().y / 2.0,
    );
    // A group of one tool has nothing more to show, unless it has side menus.
    if rows.len() < 2 && !super::ribbon_side::has_side_menus(id) {
        ui.painter().galley(label_pos, label, t.group_label);
    } else {
        // The title is a button: hovered, it is filled like the tools above it.
        // The whole title row is the button, as wide as the group.
        let hit = Rect::from_min_size(pos2(start.x, title_rect.min.y), vec2(w, LABEL_H - 1.0));
        // Named and explained as a menu of more tools, which the caret alone only hints at.
        let more = app
            .lang
            .format("ribbon-group-more", &[("group", title.into())]);
        let resp = ui
            .interact(hit, ui.id().with(("ribbon_group", id)), Sense::click())
            .on_hover_text(&more);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &more));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
        if resp.hovered() || open {
            ui.painter().rect_filled(
                hit,
                CornerRadius::same(4),
                if open { t.pressed } else { t.hover },
            );
        }
        let color = if resp.hovered() || open {
            t.text
        } else {
            t.group_label
        };
        let caret_at = pos2(label_pos.x + label.size().x + 6.0, hit.center().y);
        ui.painter().galley(label_pos, label, color);
        caret(ui.painter(), caret_at, color);
        egui::Popup::menu(&resp).show(|ui| app.group_menu(ui, id, &rows));
    }
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

/// A group in the compact ribbon: its tools in a row, then a caret for its menu instead
/// of a title, then the divider.
fn compact_group(
    ui: &mut Ui,
    app: &mut App,
    id: &'static str,
    title: &str,
    add: impl FnOnce(&mut Ui, &mut App),
) {
    let t = Tokens::get(ui.ctx());
    let top = ui.cursor().min.y;
    app.chrome.ribbon_tools.begin(id);
    forget_last_column(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        add(ui, app);
        app.group_extras(ui, id);
    });
    let rows = app.chrome.ribbon_tools.end();
    if rows.len() >= 2 || super::ribbon_side::has_side_menus(id) {
        let more = app
            .lang
            .format("ribbon-group-more", &[("group", title.into())]);
        let (rect, resp) = ui.allocate_exact_size(vec2(20.0, COMPACT_H), Sense::click());
        let resp = resp.on_hover_text(&more);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &more));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
        if resp.hovered() || open {
            let fill = if open { t.pressed } else { t.hover };
            ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
        }
        caret(ui.painter(), rect.center(), t.text_dim);
        egui::Popup::menu(&resp).show(|ui| app.group_menu(ui, id, &rows));
    }
    ui.add_space(4.0);
    let x = ui.cursor().min.x;
    ui.painter().line_segment(
        [pos2(x, top + 3.0), pos2(x, top + COMPACT_H - 3.0)],
        Stroke::new(1.0, t.border),
    );
    ui.add_space(5.0);
}

/// Room for the caret after a group's title.
const GROUP_CARET_W: f32 = 11.0;

/// A column of up to three small controls.
pub(in crate::app) fn col(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    if compact(ui.ctx()) {
        // Side by side in the one row; pinned tools then start a column of their own.
        let rect = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
                add(ui);
            })
            .response
            .rect;
        let last = LastColumn {
            rect,
            rows: COLUMN_ROWS,
        };
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new(LAST_COLUMN_KEY), last));
        return;
    }
    let rect = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            ui.add_space(1.0);
            add(ui);
        })
        .response
        .rect;
    // Kept for the group's pinned tools, which fill the last column first.
    let rows = (rect.height() / SMALL_ROW_POINTS).round() as usize;
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(LAST_COLUMN_KEY), LastColumn { rect, rows }));
}

/// A small button's height with the gap under it.
const SMALL_ROW_POINTS: f32 = 23.0;
/// Small buttons in a column.
pub(in crate::app) const COLUMN_ROWS: usize = 3;
const LAST_COLUMN_KEY: &str = "ribbon-last-column";

/// The last column a group laid out: where it is and how many small buttons it holds.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct LastColumn {
    pub rect: Rect,
    pub rows: usize,
}

/// The last column laid out since `forget_last_column`.
pub(in crate::app) fn last_column(ctx: &egui::Context) -> Option<LastColumn> {
    ctx.data(|d| d.get_temp::<LastColumn>(egui::Id::new(LAST_COLUMN_KEY)))
}

pub(in crate::app) fn forget_last_column(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<LastColumn>(egui::Id::new(LAST_COLUMN_KEY)));
}

/// A filled action button (Send, Allow…), with its label centred on the capital letters.
#[cfg(feature = "assistant")]
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
        egui::Align2::CENTER_CENTER,
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

/// Tells screen readers a tab is a tab, selected or not ("Map, tab, 2 of 4"): egui has
/// no tab widget, so it reports tabs as buttons.
pub(in crate::app) fn tab_role(tab: &Response, selected: bool) {
    tab.ctx.accesskit_node_builder(tab.id, |node| {
        node.set_role(egui::accesskit::Role::Tab);
        node.set_selected(selected);
    });
}

/// Names a text field for screen readers after its hint: egui hands the hint on only as a
/// placeholder, which leaves the field itself without a name.
pub(in crate::app) fn name_field(field: &Response, name: &str) {
    field
        .ctx
        .accesskit_node_builder(field.id, |node| node.set_label(name));
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
