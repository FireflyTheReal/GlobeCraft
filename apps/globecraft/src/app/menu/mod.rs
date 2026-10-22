//! Native desktop menus (macOS). Menu callbacks queue actions and wake egui; each action runs
//! its command through `App::run` on egui's thread. Windows and Linux use the ribbon's File menu
//! and the same commands.
#[cfg(target_os = "macos")]
use super::App;
#[cfg(target_os = "macos")]
use eframe::egui;
#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub(super) use native::NativeMenu;

mod commands;
pub(super) use commands::Action;
use commands::MenuState;
#[cfg(test)]
mod tests;

/// Whether the native menu owns the shortcut of command `id` (its accelerator consumes the keys).
#[cfg(target_os = "macos")]
pub(super) fn native_shortcut(id: &str) -> bool {
    Action::ALL
        .into_iter()
        .any(|action| action.command().0 == id && action.shortcut().is_some())
}

#[cfg(target_os = "macos")]
impl App {
    pub(super) fn with_native_menu(mut self, cc: &eframe::CreationContext<'_>) -> Self {
        match NativeMenu::new(cc, self.lang) {
            Ok(menu) => self.native_menu = Some(menu),
            Err(error) => self.status = format!("Native menu initialization failed: {error}"),
        }
        self
    }
    #[cfg(target_os = "macos")]
    pub(super) fn refresh_native_menu(&mut self, ctx: &egui::Context) {
        let state = self.menu_state(ctx);
        if let Some(menu) = &mut self.native_menu {
            menu.update(self.lang, state);
        }
    }
    #[cfg(target_os = "macos")]
    pub(super) fn poll_native_menu(&mut self, ctx: &egui::Context) {
        self.refresh_native_menu(ctx);
        let Some(menu) = &mut self.native_menu else {
            return;
        };
        let mut actions = Vec::new();
        while let Ok(action) = menu.events.try_recv() {
            actions.push(action);
        }
        for action in actions {
            self.run_menu_action(action, ctx);
        }
    }
    #[cfg(target_os = "macos")]
    pub(super) fn run_menu_action(&mut self, action: Action, ctx: &egui::Context) {
        if matches!(action, Action::Undo | Action::Redo) && ctx.egui_wants_keyboard_input() {
            // Native accelerators consume their key events. Hand text edits the corresponding
            // egui event so their own undo history wins.
            let command = if action == Action::Undo {
                super::ruler_history::HistoryCommand::Undo
            } else {
                super::ruler_history::HistoryCommand::Redo
            };
            ctx.input_mut(|input| {
                input
                    .events
                    .push(super::ruler_history::text_shortcut_event(command))
            });
            return;
        }
        let (id, params) = action.command();
        self.run_reporting(id, params);
    }

    #[cfg(target_os = "macos")]
    fn menu_state(&self, ctx: &egui::Context) -> MenuState {
        let checked = |action: Action| {
            let (id, params) = action.command();
            self.command_state(id, &params).checked
        };
        MenuState {
            image: self.image.is_some(),
            undo: ctx.egui_wants_keyboard_input() || self.ruler_paths.can_undo(),
            redo: ctx.egui_wants_keyboard_input() || self.ruler_paths.can_redo(),
            sidebar: checked(Action::Sidebar),
            navigation: checked(Action::NavigationControls),
            grid: checked(Action::Grid),
            atmosphere: checked(Action::Atmosphere),
            sunlight: checked(Action::Sunlight),
            ruler: checked(Action::Ruler),
            explorer: checked(Action::Explorer),
            fullscreen: checked(Action::Fullscreen),
        }
    }
}
