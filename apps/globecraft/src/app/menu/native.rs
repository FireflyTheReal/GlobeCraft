//! Menu objects stay on the event-loop thread; callbacks only enqueue commands.
use super::{
    Action,
    commands::{MenuState, Section},
};
use globecraft_core::language::Lang;
use muda::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
    accelerator::{Accelerator, Code, Modifiers},
};
use std::sync::mpsc;

enum Item {
    Plain(MenuItem),
    Toggle(CheckMenuItem),
}
impl Item {
    fn menu_item(&self) -> &dyn IsMenuItem {
        match self {
            Self::Plain(item) => item,
            Self::Toggle(item) => item,
        }
    }
    fn id(&self) -> &MenuId {
        self.menu_item().id()
    }
    fn set_accelerator(&self, accelerator: Option<Accelerator>) {
        // A failed update keeps the old key in the menu; the egui shortcut still works.
        let _ = match self {
            Self::Plain(item) => item.set_accelerator(accelerator),
            Self::Toggle(item) => item.set_accelerator(accelerator),
        };
    }
    fn update(&self, label: &str, enabled: bool, checked: Option<bool>) {
        match self {
            Self::Plain(item) => {
                item.set_text(label);
                item.set_enabled(enabled);
            }
            Self::Toggle(item) => {
                item.set_text(label);
                item.set_enabled(enabled);
                if let Some(checked) = checked {
                    item.set_checked(checked);
                }
            }
        }
    }
}

pub(in crate::app) struct NativeMenu {
    menu: Menu,
    sections: Vec<(Section, Submenu)>,
    items: Vec<(Action, Item)>,
    lang: Lang,
    state: MenuState,
    /// The accelerators shown, to update them when the user changes a shortcut.
    accelerators: Vec<Option<Accelerator>>,
    pub(super) events: mpsc::Receiver<Action>,
}
impl NativeMenu {
    pub(super) fn new(cc: &eframe::CreationContext<'_>, lang: Lang) -> Result<Self, String> {
        let menu = Menu::new();
        let state = MenuState::default();
        let mut items = Vec::new();
        let mut sections = Vec::new();
        for section in Section::ALL {
            let submenu = Submenu::new(section.label(lang), true);
            for action in Action::ALL
                .into_iter()
                .filter(|action| action.section() == section)
            {
                if action == Action::Quit {
                    submenu
                        .append_items(&[
                            &PredefinedMenuItem::separator(),
                            &PredefinedMenuItem::hide(None),
                            &PredefinedMenuItem::hide_others(None),
                            &PredefinedMenuItem::show_all(None),
                            &PredefinedMenuItem::separator(),
                        ])
                        .map_err(|error| error.to_string())?;
                }
                let accelerator = accelerator(action);
                let item = if let Some(checked) = state.checked(action) {
                    Item::Toggle(CheckMenuItem::new(
                        action.label(lang),
                        state.enabled(action),
                        checked,
                        accelerator,
                    ))
                } else {
                    Item::Plain(MenuItem::new(
                        action.label(lang),
                        state.enabled(action),
                        accelerator,
                    ))
                };
                submenu
                    .append(item.menu_item())
                    .map_err(|error| error.to_string())?;
                items.push((action, item));
            }
            menu.append(&submenu).map_err(|error| error.to_string())?;
            if section == Section::Window {
                submenu.set_as_windows_menu_for_nsapp();
            }
            if section == Section::Help {
                submenu.set_as_help_menu_for_nsapp();
            }
            sections.push((section, submenu));
        }
        menu.init_for_nsapp();
        let ids: Vec<_> = items
            .iter()
            .map(|(action, item)| (item.id().clone(), *action))
            .collect();
        let (send, events) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, action)) = ids.iter().find(|(id, _)| *id == event.id)
                && send.send(*action).is_ok()
            {
                ctx.request_repaint();
            }
        }));
        let accelerators = items
            .iter()
            .map(|(action, _)| accelerator(*action))
            .collect();
        Ok(Self {
            menu,
            sections,
            items,
            accelerators,
            lang,
            state,
            events,
        })
    }
    pub(super) fn update(&mut self, lang: Lang, state: MenuState) {
        if lang != self.lang {
            for (section, submenu) in &self.sections {
                submenu.set_text(section.label(lang));
            }
        }
        if lang != self.lang || state != self.state {
            for (action, item) in &self.items {
                item.update(
                    action.label(lang),
                    state.enabled(*action),
                    state.checked(*action),
                );
            }
            self.lang = lang;
            self.state = state;
        }
        for ((action, item), shown) in self.items.iter().zip(&mut self.accelerators) {
            let current = accelerator(*action);
            if current != *shown {
                item.set_accelerator(current);
                *shown = current;
            }
        }
    }
}
/// The native accelerator of an action's current shortcut, if AppKit can show its key.
pub(super) fn accelerator(action: Action) -> Option<Accelerator> {
    use eframe::egui::Key;
    let shortcut = action.shortcut()?;
    let key = match shortcut.logical_key {
        Key::A => Code::KeyA,
        Key::B => Code::KeyB,
        Key::C => Code::KeyC,
        Key::D => Code::KeyD,
        Key::E => Code::KeyE,
        Key::F => Code::KeyF,
        Key::G => Code::KeyG,
        Key::H => Code::KeyH,
        Key::I => Code::KeyI,
        Key::J => Code::KeyJ,
        Key::K => Code::KeyK,
        Key::L => Code::KeyL,
        Key::M => Code::KeyM,
        Key::N => Code::KeyN,
        Key::O => Code::KeyO,
        Key::P => Code::KeyP,
        Key::Q => Code::KeyQ,
        Key::R => Code::KeyR,
        Key::S => Code::KeyS,
        Key::T => Code::KeyT,
        Key::U => Code::KeyU,
        Key::V => Code::KeyV,
        Key::W => Code::KeyW,
        Key::X => Code::KeyX,
        Key::Y => Code::KeyY,
        Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0,
        Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2,
        Key::Num3 => Code::Digit3,
        Key::Num4 => Code::Digit4,
        Key::Num5 => Code::Digit5,
        Key::Num6 => Code::Digit6,
        Key::Num7 => Code::Digit7,
        Key::Num8 => Code::Digit8,
        Key::Num9 => Code::Digit9,
        Key::F1 => Code::F1,
        Key::F2 => Code::F2,
        Key::F3 => Code::F3,
        Key::F4 => Code::F4,
        Key::F5 => Code::F5,
        Key::F6 => Code::F6,
        Key::F7 => Code::F7,
        Key::F8 => Code::F8,
        Key::F9 => Code::F9,
        Key::F10 => Code::F10,
        Key::F11 => Code::F11,
        Key::F12 => Code::F12,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::Minus => Code::Minus,
        Key::Equals => Code::Equal,
        Key::Slash => Code::Slash,
        Key::Semicolon => Code::Semicolon,
        _ => return None,
    };
    let m = shortcut.modifiers;
    let mut modifiers = Modifiers::empty();
    if m.command || m.mac_cmd {
        modifiers |= Modifiers::META;
    } else if m.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if m.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if m.alt {
        modifiers |= Modifiers::ALT;
    }
    Some(Accelerator::new(modifiers, key))
}
impl Drop for NativeMenu {
    fn drop(&mut self) {
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        self.menu.remove_for_nsapp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_catalog_shortcut_has_a_native_accelerator() {
        for action in Action::ALL {
            assert_eq!(
                accelerator(action).is_some(),
                action.shortcut().is_some(),
                "{action:?}"
            );
        }
    }
}
