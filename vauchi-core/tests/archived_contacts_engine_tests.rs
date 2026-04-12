// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ArchivedContactsEngine and AppEngine wiring.
//!
//! @feature: contacts_management
//! @scenario: View and unarchive archived contacts
//! @internal

use vauchi_app::ui::{
    ActionResult, ActionStyle, AppEngine, AppScreen, ArchivedContactsEngine, Component,
    ContactListEngine, ScreenAction, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

// ── ArchivedContactsEngine unit tests ────────────────────────────────

// @internal
#[test]
fn archived_contacts_screen_id_is_archived_contacts() {
    let engine = ArchivedContactsEngine::new(vec![]);
    assert_eq!(engine.current_screen().screen_id, "archived_contacts");
}

// @internal
#[test]
fn archived_contacts_title_is_archived_contacts() {
    let engine = ArchivedContactsEngine::new(vec![]);
    assert_eq!(engine.current_screen().title, "Archived Contacts");
}

// @internal
#[test]
fn archived_contacts_empty_state_shows_no_archived_text() {
    let engine = ArchivedContactsEngine::new(vec![]);
    let screen = engine.current_screen();

    let has_no_archived_text = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::Text { content, .. } if content.contains("No archived")));
    assert!(
        has_no_archived_text,
        "empty state should show 'No archived' text"
    );
}

// @internal
#[test]
fn archived_contacts_empty_state_has_no_actions() {
    let engine = ArchivedContactsEngine::new(vec![]);
    let screen = engine.current_screen();
    assert!(
        screen.actions.is_empty(),
        "empty archived contacts should have no screen actions"
    );
}

// @internal
#[test]
fn archived_contacts_non_empty_shows_action_list() {
    let contacts = vec![
        ("id1".to_string(), "Alice".to_string()),
        ("id2".to_string(), "Bob".to_string()),
    ];
    let engine = ArchivedContactsEngine::new(contacts);
    let screen = engine.current_screen();

    let has_action_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ActionList { .. }));
    assert!(has_action_list, "non-empty state should show ActionList");
}

// @internal
#[test]
fn archived_contacts_items_have_unarchive_ids() {
    let contacts = vec![("abc123".to_string(), "Alice".to_string())];
    let engine = ArchivedContactsEngine::new(contacts);
    let screen = engine.current_screen();

    let item_found = screen.components.iter().any(|c| {
        if let Component::ActionList { items, .. } = c {
            items.iter().any(|item| item.id == "unarchive_abc123")
        } else {
            false
        }
    });
    assert!(
        item_found,
        "action list item should have id 'unarchive_<contact_id>'"
    );
}

// @internal
#[test]
fn archived_contacts_items_have_display_name_as_label() {
    let contacts = vec![("id1".to_string(), "Alice Smith".to_string())];
    let engine = ArchivedContactsEngine::new(contacts);
    let screen = engine.current_screen();

    let label_found = screen.components.iter().any(|c| {
        if let Component::ActionList { items, .. } = c {
            items.iter().any(|item| item.label == "Alice Smith")
        } else {
            false
        }
    });
    assert!(
        label_found,
        "action list item label should be the display name"
    );
}

// @internal
#[test]
fn archived_contacts_items_have_tap_to_unarchive_detail() {
    let contacts = vec![("id1".to_string(), "Alice".to_string())];
    let engine = ArchivedContactsEngine::new(contacts);
    let screen = engine.current_screen();

    let detail_found = screen.components.iter().any(|c| {
        if let Component::ActionList { items, .. } = c {
            items
                .iter()
                .any(|item| item.detail.as_deref() == Some("Tap to unarchive"))
        } else {
            false
        }
    });
    assert!(
        detail_found,
        "action list item detail should be 'Tap to unarchive'"
    );
}

// @internal
#[test]
fn archived_contacts_unarchive_action_returns_complete() {
    let contacts = vec![("id1".to_string(), "Alice".to_string())];
    let mut engine = ArchivedContactsEngine::new(contacts);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unarchive_id1".to_string(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "unarchive action should return Complete"
    );
}

// @internal
#[test]
fn archived_contacts_unknown_action_returns_update_screen() {
    let mut engine = ArchivedContactsEngine::new(vec![]);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown_action".to_string(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "unknown action should return UpdateScreen"
    );
}

// ── AppScreen::ArchivedContacts tests ────────────────────────────────

// @internal
#[test]
fn app_screen_archived_contacts_has_correct_screen_id() {
    assert_eq!(AppScreen::ArchivedContacts.screen_id(), "archived_contacts");
}

// @internal
#[test]
fn app_screen_archived_contacts_roundtrips_from_screen_id() {
    let parsed = AppScreen::from_screen_id("archived_contacts");
    assert_eq!(parsed, Some(AppScreen::ArchivedContacts));
}

// ── ContactListEngine view_archived action ───────────────────────────

// @scenario: contacts_management :: Contacts screen offers view archived action
// @internal
#[test]
fn contacts_screen_has_view_archived_action() {
    let contact_engine = ContactListEngine::new(vec![]);
    let screen = contact_engine.current_screen();

    let action = screen
        .actions
        .iter()
        .find(|a: &&ScreenAction| a.id == "view_archived")
        .expect("Contacts screen should have 'view_archived' action");
    assert_eq!(action.label, "Archived Contacts");
    assert_eq!(action.style, ActionStyle::Secondary);
    assert!(action.enabled);
}

// ── AppScreen::ArchivedContacts ──────────────────────────────────────

// @scenario: contacts_management :: Navigate to archived contacts from contacts list
// @internal
#[test]
fn app_engine_navigates_to_archived_contacts_from_contacts() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to contacts tab
    engine.navigate_to(AppScreen::Contacts);

    // Press view_archived
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "view_archived".to_string(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(ref s) if s.screen_id == "archived_contacts"),
        "view_archived should navigate to archived_contacts, got: {result:?}"
    );
}
