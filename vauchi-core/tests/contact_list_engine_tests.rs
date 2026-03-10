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
            searchable_fields: vec![],
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "BO".into(),
            status: Some("Updated".into()),
            searchable_fields: vec![],
        },
        ContactItem {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: Some("Coworker".into()),
            avatar_initials: "CH".into(),
            status: None,
            searchable_fields: vec![],
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

// --- Field search tests ---

fn contacts_with_fields() -> Vec<ContactItem> {
    vec![
        ContactItem {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: Some("+1-555-0100".into()),
            avatar_initials: "AL".into(),
            status: None,
            searchable_fields: vec!["+1-555-0100".into(), "alice@example.com".into()],
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: Some("bob@work.com".into()),
            avatar_initials: "BO".into(),
            status: None,
            searchable_fields: vec!["bob@work.com".into(), "+1-555-0200".into()],
        },
        ContactItem {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: None,
            avatar_initials: "CH".into(),
            status: None,
            searchable_fields: vec![],
        },
    ]
}

#[test]
fn search_matches_field_values() {
    let mut engine = ContactListEngine::new(contacts_with_fields());
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "alice@".into(),
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
fn search_matches_phone_number() {
    let mut engine = ContactListEngine::new(contacts_with_fields());
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "555-0200".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert_eq!(contacts.len(), 1);
            assert_eq!(contacts[0].name, "Bob");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn search_matches_name_or_field() {
    let mut engine = ContactListEngine::new(contacts_with_fields());
    // "bob" matches Bob's name but not Alice's fields; "alice@" matches Alice's field
    // Here we search for something that only matches via field
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "work.com".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert_eq!(contacts.len(), 1);
            assert_eq!(contacts[0].name, "Bob");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn search_no_match_returns_empty() {
    let mut engine = ContactListEngine::new(contacts_with_fields());
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert!(contacts.is_empty());
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// --- Group filter tests ---

fn contacts_with_groups() -> (Vec<ContactItem>, Vec<(String, String)>) {
    let contacts = vec![
        ContactItem {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: None,
            avatar_initials: "AL".into(),
            status: None,
            searchable_fields: vec![],
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "BO".into(),
            status: None,
            searchable_fields: vec![],
        },
        ContactItem {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: None,
            avatar_initials: "CH".into(),
            status: None,
            searchable_fields: vec![],
        },
    ];
    let groups = vec![("g1".into(), "Family".into()), ("g2".into(), "Work".into())];
    (contacts, groups)
}

fn group_memberships() -> std::collections::HashMap<String, Vec<String>> {
    let mut m = std::collections::HashMap::new();
    // Alice and Charlie are in Family
    m.insert("g1".to_string(), vec!["c1".to_string(), "c3".to_string()]);
    // Bob is in Work
    m.insert("g2".to_string(), vec!["c2".to_string()]);
    m
}

#[test]
fn group_filter_shows_only_group_members() {
    let (contacts, groups) = contacts_with_groups();
    let memberships = group_memberships();
    let mut engine = ContactListEngine::with_groups(contacts, groups, memberships);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "filter_group:g1".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let contacts = extract_contacts(&screen);
            assert_eq!(contacts.len(), 2);
            assert_eq!(contacts[0].name, "Alice");
            assert_eq!(contacts[1].name, "Charlie");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn group_filter_clear_shows_all() {
    let (contacts, groups) = contacts_with_groups();
    let memberships = group_memberships();
    let mut engine = ContactListEngine::with_groups(contacts, groups, memberships);

    // Set filter
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "filter_group:g1".into(),
    });
    // Clear filter
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "filter_group_clear".into(),
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
fn group_filter_combines_with_search() {
    let (mut contacts, groups) = contacts_with_groups();
    // Give Alice a field
    contacts[0].searchable_fields = vec!["alice@example.com".into()];
    let memberships = group_memberships();
    let mut engine = ContactListEngine::with_groups(contacts, groups, memberships);

    // Filter to Family (Alice, Charlie)
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "filter_group:g1".into(),
    });
    // Search for "alice" — should match Alice (name + in group)
    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "alice".into(),
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
fn available_groups_shown_in_screen() {
    let (contacts, groups) = contacts_with_groups();
    let memberships = group_memberships();
    let engine = ContactListEngine::with_groups(contacts, groups, memberships);
    let screen = engine.current_screen();

    // Should have filter_group actions
    let group_actions: Vec<_> = screen
        .actions
        .iter()
        .filter(|a| a.id.starts_with("filter_group:"))
        .collect();
    assert_eq!(group_actions.len(), 2);
    assert_eq!(group_actions[0].label, "Family");
    assert_eq!(group_actions[1].label, "Work");
}
