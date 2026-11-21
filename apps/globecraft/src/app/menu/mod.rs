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
#[cfg(target_os = "macos")]
use commands::MenuState;
#[cfg(test)]
mod tests;

/// Whether the native menu owns the shortcut of command `id` (its accelerator consumes the keys).
#[cfg(target_os = "macos")]
pub(super) fn native_shortcut(id: &str) -> bool {
    Action::all()
        .iter()
        .any(|action| action.id == id && native::accelerator(*action).is_some())
}

#[cfg(target_os = "macos")]
impl App {
    pub(super) fn with_native_menu(mut self, cc: &eframe::CreationContext<'_>) -> Self {
        // AppKit menus need the main thread; test harnesses run the app on a worker.
        if cfg!(test) {
            return self;
        }
        match NativeMenu::new(cc, self.lang) {
            Ok(menu) => self.chrome.native_menu = Some(menu),
            Err(error) => self.status = format!("Native menu initialization failed: {error}"),
        }
        self
    }
    #[cfg(target_os = "macos")]
    pub(super) fn refresh_native_menu(&mut self, ctx: &egui::Context) {
        let state = self.menu_state(ctx);
        if let Some(menu) = &mut self.chrome.native_menu {
            menu.update(self.lang, state);
        }
    }
    #[cfg(target_os = "macos")]
    pub(super) fn poll_native_menu(&mut self, ctx: &egui::Context) {
        self.refresh_native_menu(ctx);
        let Some(menu) = &mut self.chrome.native_menu else {
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
        let history = matches!(action.id, "edit.undo" | "edit.redo");
        if history && ctx.egui_wants_keyboard_input() {
            // Native accelerators consume their key events. Hand text edits the corresponding
            // egui event so their own undo history wins.
            let command = if action.id == "edit.undo" {
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
        Action::all()
            .iter()
            .map(|action| {
                let (id, params) = action.command();
                let mut state = self.command_state(id, &params);
                // A text edit has its own history, which Undo and Redo then reach.
                if matches!(id, "edit.undo" | "edit.redo") && ctx.egui_wants_keyboard_input() {
                    state.enabled = true;
                }
                state
            })
            .collect()
    }
}
