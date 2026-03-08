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
            avatar_initials: "A".into(),
            status: None,
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "B".into(),
            status: Some("Updated".into()),
        },
    ]
}

#[test]
fn home_screen_id() {
    let engine = HomeEngine::new(vec![], HomeProgress::default());
    assert_eq!(engine.current_screen().screen_id, "home");
}

#[test]
fn home_with_contacts_shows_contact_list() {
    let engine = HomeEngine::new(
        sample_contacts(),
        HomeProgress {
            completed_steps: 6,
            total_steps: 6,
        },
    );
    let screen = engine.current_screen();
    let has_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ContactList { .. }));
    assert!(has_list, "Home should have a ContactList component");
}

#[test]
fn home_limits_recent_to_five() {
    let contacts: Vec<ContactItem> = (0..10)
        .map(|i| ContactItem {
            id: format!("c{i}"),
            name: format!("Contact {i}"),
            subtitle: None,
            avatar_initials: format!("{i}"),
            status: None,
        })
        .collect();
    let engine = HomeEngine::new(
        contacts,
        HomeProgress {
            completed_steps: 6,
            total_steps: 6,
        },
    );
    let screen = engine.current_screen();
    if let Some(Component::ContactList { contacts, .. }) = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::ContactList { .. }))
    {
        assert_eq!(contacts.len(), 5);
    } else {
        panic!("Expected ContactList");
    }
}

#[test]
fn home_with_no_contacts_hides_view_all() {
    let engine = HomeEngine::new(
        vec![],
        HomeProgress {
            completed_steps: 6,
            total_steps: 6,
        },
    );
    let screen = engine.current_screen();
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "add_contact");
}

#[test]
fn home_with_contacts_shows_view_all() {
    let engine = HomeEngine::new(
        sample_contacts(),
        HomeProgress {
            completed_steps: 6,
            total_steps: 6,
        },
    );
    let screen = engine.current_screen();
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[1].id, "view_all");
}

#[test]
fn home_shows_setup_progress_when_incomplete() {
    let engine = HomeEngine::new(
        vec![],
        HomeProgress {
            completed_steps: 3,
            total_steps: 6,
        },
    );
    let screen = engine.current_screen();
    let has_progress = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::StatusIndicator { id, .. } if id == "setup_progress"));
    assert!(has_progress, "Should show setup progress when incomplete");
}

#[test]
fn home_hides_setup_progress_when_complete() {
    let engine = HomeEngine::new(
        vec![],
        HomeProgress {
            completed_steps: 6,
            total_steps: 6,
        },
    );
    let screen = engine.current_screen();
    let has_progress = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::StatusIndicator { .. }));
    assert!(!has_progress, "Should hide setup progress when complete");
}

#[test]
fn home_select_contact_opens_it() {
    let mut engine = HomeEngine::new(sample_contacts(), HomeProgress::default());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "recent_contacts".into(),
        item_id: "c1".into(),
    });
    match result {
        ActionResult::OpenContact { contact_id } => assert_eq!(contact_id, "c1"),
        other => panic!("Expected OpenContact, got {other:?}"),
    }
}
