// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn sample_contacts() -> Vec<IndexedItem> {
    vec![
        IndexedItem::from(Item {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: Some("Friend".into()),
            initials: "AL".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
        IndexedItem::from(Item {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            initials: "BO".into(),
            status: Some("Updated".into()),
            actions: vec![],
            a11y: None,
        }),
        IndexedItem::from(Item {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: Some("Coworker".into()),
            initials: "CH".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
    ]
}

fn extract_contacts(screen: &ScreenModel) -> &Vec<Item> {
    match screen
        .components
        .iter()
        .find(|c| matches!(c, Component::List { .. }))
    {
        Some(Component::List { items, .. }) => items,
        _ => panic!("Expected List component"),
    }
}

// @internal
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

// @internal
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

// @internal
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

// @internal
#[test]
fn contact_list_search_empty_restores_all() {
    let mut engine = ContactListEngine::new(sample_contacts());

    let _ = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "Ali".into(),
    });

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

// @internal
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

// @scenario: onboarding.feature - Empty state with guidance
// @internal
#[test]
fn contact_list_empty_shows_guidance() {
    let engine = ContactListEngine::new(vec![]);
    let screen = engine.current_screen();

    let info = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::InfoPanel { .. }));
    assert!(
        info.is_some(),
        "Empty contacts should show InfoPanel guidance, got: {:?}",
        screen.components
    );
    match info.unwrap() {
        Component::InfoPanel {
            id, title, items, ..
        } => {
            assert_eq!(id, "empty_state");
            assert!(!title.is_empty(), "InfoPanel should have a title");
            assert!(!items.is_empty(), "InfoPanel should have guidance items");
        }
        _ => unreachable!(),
    }

    let exchange_action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "go_exchange");
    assert!(
        exchange_action.is_some(),
        "Empty contacts should have a 'go exchange' action"
    );
}

// @scenario: onboarding.feature - Search with no results still shows ContactList
// @internal
#[test]
fn contact_list_search_no_results_shows_empty_list_not_guidance() {
    let mut engine = ContactListEngine::new(sample_contacts());
    let _ = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "nonexistent".into(),
    });
    let screen = engine.current_screen();

    // Search with no results should show empty ContactList (not InfoPanel)
    // because the user is actively searching — guidance would be confusing
    let has_contact_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::List { .. }));
    assert!(
        has_contact_list,
        "Search with no results should show ContactList, not InfoPanel"
    );
}

// --- Field search tests ---

fn contacts_with_fields() -> Vec<IndexedItem> {
    vec![
        IndexedItem::new(
            Item {
                id: "c1".into(),
                name: "Alice".into(),
                subtitle: Some("+1-555-0100".into()),
                initials: "AL".into(),
                status: None,
                actions: vec![],
                a11y: None,
            },
            vec!["+1-555-0100".into(), "alice@example.com".into()],
        ),
        IndexedItem::new(
            Item {
                id: "c2".into(),
                name: "Bob".into(),
                subtitle: Some("bob@work.com".into()),
                initials: "BO".into(),
                status: None,
                actions: vec![],
                a11y: None,
            },
            vec!["bob@work.com".into(), "+1-555-0200".into()],
        ),
        IndexedItem::from(Item {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: None,
            initials: "CH".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
    ]
}

// @internal
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

// @internal
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

// @internal
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

// @internal
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

fn contacts_with_groups() -> (Vec<IndexedItem>, Vec<(String, String)>) {
    let contacts = vec![
        IndexedItem::from(Item {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: None,
            initials: "AL".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
        IndexedItem::from(Item {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            initials: "BO".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
        IndexedItem::from(Item {
            id: "c3".into(),
            name: "Charlie".into(),
            subtitle: None,
            initials: "CH".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
    ];
    let groups = vec![("g1".into(), "Family".into()), ("g2".into(), "Work".into())];
    (contacts, groups)
}

fn group_memberships() -> std::collections::HashMap<String, Vec<String>> {
    let mut m = std::collections::HashMap::new();
    m.insert("g1".to_string(), vec!["c1".to_string(), "c3".to_string()]);
    m.insert("g2".to_string(), vec!["c2".to_string()]);
    m
}

// @internal
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

// @internal
#[test]
fn group_filter_clear_shows_all() {
    let (contacts, groups) = contacts_with_groups();
    let memberships = group_memberships();
    let mut engine = ContactListEngine::with_groups(contacts, groups, memberships);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "filter_group:g1".into(),
    });
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

// @internal
#[test]
fn group_filter_combines_with_search() {
    let (mut contacts, groups) = contacts_with_groups();
    contacts[0].searchable = vec!["alice@example.com".into()];
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

// @internal
#[test]
fn available_groups_shown_in_screen() {
    let (contacts, groups) = contacts_with_groups();
    let memberships = group_memberships();
    let engine = ContactListEngine::with_groups(contacts, groups, memberships);
    let screen = engine.current_screen();

    let group_actions: Vec<_> = screen
        .contextual_actions
        .iter()
        .filter(|a| a.id.starts_with("filter_group:"))
        .collect();
    assert_eq!(group_actions.len(), 2);
    assert_eq!(group_actions[0].label, "Family");
    assert_eq!(group_actions[1].label, "Work");
}

// @scenario: accessibility :: contact items preserve a11y through engine
// @internal
#[test]
fn contact_item_a11y_is_preserved_through_engine() {
    let contacts = vec![IndexedItem::from(Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: None,
        initials: "AL".into(),
        status: None,
        actions: vec![],
        a11y: Some(A11y {
            label: Some("Contact: Alice".into()),
            hint: Some("Double tap to view contact details".into()),
            role: None,
        }),
    })];
    let engine = ContactListEngine::new(contacts);
    let screen = engine.current_screen();

    let items = extract_contacts(&screen);
    assert_eq!(items.len(), 1);
    let a11y = items[0].a11y.as_ref().expect("Item should carry a11y");
    assert_eq!(
        a11y.label.as_deref(),
        Some("Contact: Alice"),
        "a11y label should include the contact name"
    );
    assert_eq!(
        a11y.hint.as_deref(),
        Some("Double tap to view contact details"),
        "a11y hint should guide screen reader interaction"
    );
}
