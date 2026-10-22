use super::*;
use globecraft_core::language::Lang;

#[test]
fn menu_catalog_has_translated_labels_populated_sections_and_unique_shortcuts() {
    use commands::Section;
    for lang in Lang::ALL {
        for section in Section::ALL {
            assert!(!section.label(lang).is_empty());
            assert!(Action::ALL.iter().any(|action| action.section() == section));
        }
        for action in Action::ALL {
            assert!(!action.label(lang).is_empty());
        }
    }
    let shortcuts: Vec<_> = Action::ALL
        .into_iter()
        .filter_map(|action| action.shortcut())
        .collect();
    for (index, shortcut) in shortcuts.iter().enumerate() {
        assert!(!shortcuts[index + 1..].contains(shortcut));
    }
}

#[test]
fn menu_enablement_and_checkmarks_follow_app_state() {
    let empty = MenuState::default();
    assert!(!empty.enabled(Action::Screenshot));
    assert!(!empty.enabled(Action::Undo));
    assert!(!empty.enabled(Action::Redo));
    assert!(empty.enabled(Action::Open));
    assert!(empty.enabled(Action::Settings));
    let state = MenuState {
        image: true,
        undo: true,
        redo: true,
        sidebar: true,
        ruler: true,
        grid: true,
        navigation: true,
        ..MenuState::default()
    };
    assert!(state.enabled(Action::Screenshot));
    assert!(state.enabled(Action::Undo));
    assert!(state.enabled(Action::Redo));
    for action in [
        Action::Sidebar,
        Action::Ruler,
        Action::Grid,
        Action::NavigationControls,
    ] {
        assert_eq!(state.checked(action), Some(true));
        assert_eq!(empty.checked(action), Some(false));
    }
    assert_eq!(state.checked(Action::Placemark), None);
}

#[test]
fn every_menu_action_runs_a_known_command() {
    for action in Action::ALL {
        let (id, _) = action.command();
        assert!(
            globecraft_core::commands::find(id).is_some(),
            "{action:?} runs unknown command {id}"
        );
    }
}
