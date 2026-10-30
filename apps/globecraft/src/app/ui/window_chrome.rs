//! Merged titlebar behaviour: native macOS traffic lights, drawn Windows/Linux
//! window controls, window dragging and edge resizing for undecorated windows.
#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::icons;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::theme::Tokens;
use crate::app::App;
use eframe::egui::{self, Ui};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use eframe::egui::{Color32, Rect, Sense, pos2, vec2};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use globecraft_core::language::Lang;

/// Width of one drawn window control on Windows and Linux.
#[cfg(any(target_os = "windows", target_os = "linux"))]
const WINDOW_CONTROL_WIDTH: f32 = 34.0;
/// The three controls occupy the right edge of the header.
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(super) const WINDOW_CONTROLS_WIDTH: f32 = 3.0 * WINDOW_CONTROL_WIDTH;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) const WINDOW_CONTROLS_WIDTH: f32 = 0.0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(super) const WINDOW_CONTROLS_GAP: f32 = 4.0;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) const WINDOW_CONTROLS_GAP: f32 = 0.0;
/// Pointer distance from a window edge that starts a resize on Windows and Linux.
#[cfg(any(target_os = "windows", target_os = "linux"))]
const WINDOW_RESIZE_EDGE_POINTS: f32 = 6.0;

impl App {
    /// Room the native macOS traffic lights take at the left of the header.
    pub(in crate::app) fn traffic_lights_width(&self, ctx: &egui::Context) -> f32 {
        #[cfg(target_os = "macos")]
        {
            self.traffic_lights_width_native / ctx.zoom_factor()
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = ctx;
            0.0
        }
    }

    /// Dragging an empty part of the header moves the window; double-clicking it
    /// toggles maximize. Buttons added after the header's background interaction
    /// stay on top, so pressing them still clicks the button.
    pub(in crate::app) fn header_drag(&self, ui: &Ui, drag: &egui::Response) {
        let primary = egui::PointerButton::Primary;
        if drag.drag_started_by(primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        } else if drag.double_clicked_by(primary) {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
    }

    /// Minimize, maximize/restore and close, drawn at the right of the header on
    /// Windows and Linux. macOS uses its native traffic lights instead.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(in crate::app) fn window_controls(&self, ui: &mut Ui, t: &Tokens, full: Rect, lang: Lang) {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        let controls = [
            ("window_minimize", "window-minimize"),
            (
                if maximized {
                    "window_restore"
                } else {
                    "window_maximize"
                },
                if maximized {
                    "window-restore"
                } else {
                    "window-maximize"
                },
            ),
            ("close", "window-close"),
        ];
        let resize_edge = edge_direction(ui).is_some();
        for (index, (icon, tooltip)) in controls.into_iter().enumerate() {
            let left = full.max.x - WINDOW_CONTROLS_WIDTH + index as f32 * WINDOW_CONTROL_WIDTH;
            let rect = Rect::from_min_max(
                pos2(left, full.min.y),
                pos2(left + WINDOW_CONTROL_WIDTH, full.max.y),
            );
            // The outer few points resize the window instead of pressing the
            // control, matching the resize border of a decorated window.
            let sense = if resize_edge {
                Sense::hover()
            } else {
                Sense::click()
            };
            let response = ui.interact(rect, egui::Id::new(("window_control", index)), sense);
            let tip = lang.text(tooltip);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
            let close = index == 2;
            let hovered = response.hovered();
            if hovered {
                let fill = if close { t.red } else { t.hover };
                ui.painter().rect_filled(rect, 0.0, fill);
            }
            let ink = if close && hovered {
                Color32::WHITE
            } else {
                t.icon
            };
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(12.0, 12.0)),
                icon,
                ink,
                ink,
            );
            if response.clicked() {
                let command = match index {
                    0 => egui::ViewportCommand::Minimized(true),
                    1 => egui::ViewportCommand::Maximized(!maximized),
                    _ => egui::ViewportCommand::Close,
                };
                ui.ctx().send_viewport_cmd(command);
            }
            response.on_hover_text(tip);
        }
    }
}

/// Starts an edge resize on Windows and Linux, where the window has no native
/// decorations to provide one.
pub(in crate::app) fn resize_edges(ui: &Ui) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let Some(direction) = edge_direction(ui) else {
            return;
        };
        ui.ctx().set_cursor_icon(match direction {
            egui::ResizeDirection::North | egui::ResizeDirection::South => {
                egui::CursorIcon::ResizeVertical
            }
            egui::ResizeDirection::East | egui::ResizeDirection::West => {
                egui::CursorIcon::ResizeHorizontal
            }
            egui::ResizeDirection::NorthEast | egui::ResizeDirection::SouthWest => {
                egui::CursorIcon::ResizeNeSw
            }
            egui::ResizeDirection::NorthWest | egui::ResizeDirection::SouthEast => {
                egui::CursorIcon::ResizeNwSe
            }
        });
        if ui.input(|i| i.pointer.primary_pressed()) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let _ = ui;
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn edge_direction(ui: &Ui) -> Option<egui::ResizeDirection> {
    let ctx = ui.ctx();
    if ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    }) {
        return None;
    }
    let pos = ctx.input(|i| i.pointer.hover_pos())?;
    let rect = ctx.input(|i| i.viewport_rect());
    if !rect.contains(pos) {
        return None;
    }
    let edge = WINDOW_RESIZE_EDGE_POINTS;
    let north = pos.y - rect.min.y <= edge;
    let south = rect.max.y - pos.y <= edge;
    let west = pos.x - rect.min.x <= edge;
    let east = rect.max.x - pos.x <= edge;
    use egui::ResizeDirection as Direction;
    match (north, south, west, east) {
        (true, _, true, _) => Some(Direction::NorthWest),
        (true, _, _, true) => Some(Direction::NorthEast),
        (_, true, true, _) => Some(Direction::SouthWest),
        (_, true, _, true) => Some(Direction::SouthEast),
        (true, _, _, _) => Some(Direction::North),
        (_, true, _, _) => Some(Direction::South),
        (_, _, true, _) => Some(Direction::West),
        (_, _, _, true) => Some(Direction::East),
        _ => None,
    }
}
