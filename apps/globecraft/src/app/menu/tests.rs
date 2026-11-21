use super::*;
use globecraft_core::{language::Lang, layers::MapLayer};

#[test]
fn menu_catalog_has_translated_labels_populated_sections_and_unique_shortcuts() {
    use commands::Section;
    for lang in Lang::ALL {
        for section in Section::ALL {
            assert!(!section.label(lang).is_empty());
            assert!(Action::all().iter().any(|action| action.section == section));
        }
        for action in Action::all() {
            assert!(!action.label(lang).is_empty());
            assert_ne!(action.label(lang), action.id, "{action:?} has no name");
        }
    }
    let shortcuts: Vec<_> = Action::all()
        .iter()
        .filter_map(|action| action.shortcut())
        .collect();
    for (index, shortcut) in shortcuts.iter().enumerate() {
        assert!(!shortcuts[index + 1..].contains(shortcut));
    }
}

#[test]
fn every_menu_action_runs_a_known_command() {
    for action in Action::all() {
        let (id, _) = action.command();
        assert!(
            globecraft_core::commands::find(id).is_some(),
            "{action:?} runs unknown command {id}"
        );
    }
}

/// The menu bar lists every layer, as Mac users look there first (HIG: The menu bar).
#[test]
fn every_offered_layer_has_a_menu_item_and_menus_keep_their_order() {
    for layer in MapLayer::ALL
        .into_iter()
        .filter(|l| crate::app::App::offers(*l))
    {
        assert!(
            Action::all()
                .iter()
                .any(|action| action.layer == Some(layer)),
            "{layer:?}"
        );
    }
    let sections: Vec<_> = Action::all().iter().map(|action| action.section).collect();
    let mut sorted = sections.clone();
    sorted.dedup();
    let mut unique = sorted.clone();
    unique.sort_by_key(|s| commands::Section::ALL.iter().position(|x| x == s));
    unique.dedup();
    assert_eq!(
        sorted, unique,
        "each menu's items are together, in menu order"
    );
}
