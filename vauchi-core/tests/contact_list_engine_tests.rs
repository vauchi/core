// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

fn sample_contacts() -> Vec<ContactItem> {
    vec![
        ContactItem {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: Some("Friend".into()),
            avatar_initials: "AL".into(),
            status: None,
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "BO".into(),
            status: Some("Updated".into()),
        },
        ContactItem {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: Some("Coworker".into()),
            avatar_initials: "CH".into(),
            status: None,
        },
    ]
}

fn extract_contacts(screen: &ScreenModel) -> &Vec<ContactItem> {
    match screen
        .components
        .iter()
        .find(|c| matches!(c, Component::ContactList { .. }))
    {
        Some(Component::ContactList { contacts, .. }) => contacts,
        _ => panic!("Expected ContactList component"),
    }
}

#[test]
fn contact_list_shows_all_contacts() {
    let engine = ContactListEngine::new(sample_contacts());
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "contact_list");
    assert_eq!(screen.title, "Contacts");
    assert!(screen.progress.is_none());

    let contacts = extract_contacts(&screen);
    assert_eq!(contacts.len(), 3);
    assert_eq!(contacts[0].name, "Alice");
    assert_eq!(contacts[1].name, "Bob");
    assert_eq!(contacts[2].name, "Charlie");
}

#[test]
fn contact_list_search_filters_by_name() {
    let mut engine = ContactListEngine::new(sample_contacts());
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "Ali".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert_eq!(contacts.len(), 1);
            assert_eq!(contacts[0].name, "Alice");
            assert_eq!(contacts[0].id, "c1");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn contact_list_search_case_insensitive() {
    let mut engine = ContactListEngine::new(sample_contacts());
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "ali".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert_eq!(contacts.len(), 1);
            assert_eq!(contacts[0].name, "Alice");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn contact_list_search_empty_restores_all() {
    let mut engine = ContactListEngine::new(sample_contacts());

    // First filter down
    let _ = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "Ali".into(),
    });

    // Then clear the search
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: String::new(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert_eq!(contacts.len(), 3);
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn contact_list_select_opens_contact() {
    let mut engine = ContactListEngine::new(sample_contacts());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "contacts".into(),
        item_id: "c2".into(),
    });

    match result {
        ActionResult::OpenContact { contact_id } => assert_eq!(contact_id, "c2"),
        other => panic!("Expected OpenContact, got {other:?}"),
    }
}

#[test]
fn contact_list_empty_has_no_contacts() {
    let engine = ContactListEngine::new(vec![]);
    let screen = engine.current_screen();

    let contacts = extract_contacts(&screen);
    assert!(contacts.is_empty(), "Empty engine should have no contacts");

    // Verify searchable is still true
    match &screen.components[0] {
        Component::ContactList { searchable, .. } => {
            assert!(
                searchable,
                "ContactList should be searchable even when empty"
            );
        }
        _ => panic!("Expected ContactList component"),
    }
}
