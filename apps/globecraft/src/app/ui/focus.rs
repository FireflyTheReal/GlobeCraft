//! Keyboard reach (roadmap 6.6, layout redesign step 5). egui already moves the focus
//! with Tab and the arrow keys, and Enter or Space press the focused control; this
//! module adds what the custom-painted interface needs on top: a visible focus ring
//! on every control, and F6 to jump between the window's regions (the ribbon, the left
//! pane, the right pane, the globe).
use super::theme::Tokens;
use crate::app::App;
use eframe::egui::{self, CornerRadius, Id, Stroke, StrokeKind};
use globecraft_data::settings::sidebar::SidebarPanel;

/// The id of a ribbon tab's button.
pub(in crate::app) fn ribbon_tab_id(tab: globecraft_core::commands::Tab) -> Id {
    Id::new(("ribbon_tab", tab.code()))
}

/// The id of a left pane's tab.
pub(in crate::app) fn left_tab_id(panel: SidebarPanel) -> Id {
    Id::new(("left_pane_tab", panel as u8))
}

/// The id of a right pane's tab.
pub(in crate::app) fn right_tab_id(pane: globecraft_data::settings::workspace::RightPane) -> Id {
    Id::new(("right_pane_tab", pane as u8))
}

impl App {
    /// The window's regions in F6 order: the shown ribbon tab, the shown left pane's tab
    /// and the shown right pane's tab, where they are open. `None` is the globe.
    fn regions(&self) -> Vec<Option<Id>> {
        let mut regions = vec![Some(ribbon_tab_id(self.settings.workspace.ribbon_tab))];
        if self.settings.sidebar.open {
            regions.push(Some(left_tab_id(self.settings.sidebar.panel)));
        }
        if let Some(pane) = self.settings.workspace.right_pane {
            regions.push(Some(right_tab_id(pane)));
        }
        regions.push(None);
        regions
    }

    /// Moves the keyboard focus to the next region (`back`: the previous one).
    pub(in crate::app) fn focus_next_region(&mut self, ctx: &egui::Context, back: bool) {
        let regions = self.regions();
        let focused = ctx.memory(|m| m.focused());
        let at = regions.iter().position(|region| *region == focused);
        let next = match (at, back) {
            (Some(i), false) => (i + 1) % regions.len(),
            (Some(i), true) => (i + regions.len() - 1) % regions.len(),
            (None, false) => 0,
            (None, true) => regions.len() - 1,
        };
        ctx.memory_mut(|m| match (regions[next], focused) {
            (Some(id), _) => m.request_focus(id),
            (None, Some(id)) => m.surrender_focus(id),
            (None, None) => {}
        });
    }
}

/// A ring around the control that has the keyboard focus, over everything: the
/// interface paints its own controls, so egui's focus look does not reach them. Text
/// fields and the globe (which take drags) show their focus themselves.
pub(in crate::app) fn paint_focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|m| m.focused()) else {
        return;
    };
    let Some(response) = ctx.read_response(id) else {
        return;
    };
    if !response.sense.senses_click() || response.sense.senses_drag() {
        return;
    }
    let t = Tokens::get(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        Id::new("focus_ring"),
    ));
    painter.rect_stroke(
        response.rect.expand(1.0),
        CornerRadius::same(6),
        Stroke::new(2.0, t.accent),
        StrokeKind::Outside,
    );
}
