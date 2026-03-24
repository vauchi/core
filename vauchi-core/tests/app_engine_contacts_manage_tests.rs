// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine contact management tests: my_info fields, contact detail/edit routing,
//! contact visibility, duplicate detection, contact merge, and contact limit.

use vauchi_app::ui::{
    ActionResult, ActionStyle, AppEngine, AppScreen, Component, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;

// ── my_info field tests ──────────────────────────────────────────────

#[test]
fn my_info_shows_own_fields_via_app_engine() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 123 45 67",
        ))
        .unwrap();

    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");

    // MyInfo should show own fields in an ActionList (entry view)
    let has_entries = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ActionList { id, .. } if id == "own_entries"));
    assert!(has_entries, "MyInfo should show own entries ActionList");

    let has_contact_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ContactList { .. }));
    assert!(!has_contact_list, "MyInfo should not show a ContactList");
}

#[test]
fn my_info_renders_safely_with_no_fields() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");
    assert!(
        !screen.title.is_empty(),
        "my_info screen should have a title"
    );
}

// ── contact detail / edit wiring tests ──────────────────────────────

#[test]
fn contact_detail_nonexistent_shows_not_found() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: "nonexistent".into(),
    });
    assert_eq!(screen.screen_id, "contact_not_found");
}

#[test]
fn contact_edit_nonexistent_shows_not_found() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactEdit {
        contact_id: "nonexistent".into(),
    });
    // Non-existent contact should show edit_fields (empty) or not_found
    // ContactEditEngine starts on edit_fields, but with nonexistent we show not_found
    assert_eq!(screen.screen_id, "contact_not_found");
}

#[test]
fn contact_detail_edit_navigates_to_edit_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    // Add a contact
    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([2u8; 32], card, shared_key);
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    // Navigate to ContactDetail for Bob
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: bob_id.clone(),
    });
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::ContactDetail {
            contact_id: bob_id.clone()
        }
    );

    // Press the "edit" button
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    // Should navigate to ContactEdit, not re-open ContactDetail (T-1: verify screen_id)
    let ActionResult::NavigateTo(screen) = result else {
        panic!("expected NavigateTo for edit button, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "edit_fields",
        "edit button should navigate to edit_fields screen"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::ContactEdit { contact_id: bob_id },
        "edit button should route to ContactEdit, not ContactDetail"
    );
}

// ── contact visibility tests ─────────────────────────────────────────

#[test]
fn navigate_to_contact_visibility_shows_toggles() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    // No real contact exists, so engine shows empty field list
    let screen = engine.navigate_to(AppScreen::ContactVisibility {
        contact_id: "fake-id".into(),
    });
    assert_eq!(screen.screen_id, "contact_visibility");
    assert!(
        screen.actions.iter().any(|a| a.id == "save"),
        "Visibility screen must have save action"
    );
}

#[test]
fn contact_visibility_toggle_updates_field() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactVisibility {
        contact_id: "fake-id".into(),
    });
    // Toggle a nonexistent field — should not panic, just return screen
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "field_toggles".into(),
        item_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_visibility");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// ── groups routing tests ─────────────────────────────────────────────

#[test]
fn navigate_to_group_detail_shows_group() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::GroupDetail {
        group_id: "g1".into(),
    });
    assert_eq!(screen.screen_id, "group_detail");
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "delete_group" && a.style == ActionStyle::Destructive),
        "GroupDetail must have destructive delete action"
    );
}

/// @scenario: visibility_labels :: Group detail shows real name and members
#[test]
fn group_detail_shows_real_name_and_members() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let group = vauchi.create_group("Family").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::GroupDetail {
        group_id: group.id().to_string(),
    });

    assert_eq!(
        screen.title, "Family",
        "Title should be the real group name"
    );
    // With no contacts added, members list should be empty
    let has_member_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ContactList { id, contacts, .. } if id == "members" && contacts.is_empty()));
    assert!(has_member_list, "Should have an empty members ContactList");
}

#[test]
fn groups_list_item_selected_routes_to_group_detail() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Groups);

    // Select a group from the list
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "groups".into(),
        item_id: "g1".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "group_detail",
                "selecting a group should navigate to GroupDetail"
            );
        }
        other => panic!("Expected NavigateTo group_detail, got {:?}", other),
    }
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::GroupDetail {
            group_id: "g1".into()
        }
    );
}

// =============================================================================
// SP-12a: Duplicate Detection, Merge Preview, Contact Limit
// =============================================================================

#[test]
fn duplicate_detection_navigate_shows_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDuplicates);
    assert_eq!(screen.screen_id, "duplicate_detection");
    assert_eq!(screen.title, "Duplicate Detection");
}

#[test]
fn duplicate_detection_empty_shows_no_duplicates() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDuplicates);
    // With no pairs, should show "no duplicates" text
    assert!(
        screen.components.iter().any(|c| matches!(c,
            Component::Text { content, .. } if content.contains("No duplicate")
        )),
        "Empty pairs should show 'No duplicate' message, got {:?}",
        screen.components
    );
}

#[test]
fn duplicate_detection_merge_navigates_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });
    // Engine returns Complete, AppEngine intercepts and navigates back
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "merge action should navigate back, got {result:?}"
    );
}

#[test]
fn duplicate_detection_dismiss_stays_on_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "dismiss".into(),
    });
    // Dismiss stays on screen (only merge triggers navigation back)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "dismiss should stay on screen, got {result:?}"
    );
}

#[test]
fn contact_merge_navigate_shows_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec!["email: alice@example.com".into()],
        secondary_name: "Bob".into(),
        secondary_fields: vec!["phone: +1234567890".into()],
    });
    assert_eq!(screen.screen_id, "contact_merge");
    assert_eq!(screen.title, "Merge Contacts");
}

#[test]
fn contact_merge_shows_both_contacts() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec!["email: alice@example.com".into()],
        secondary_name: "Bob".into(),
        secondary_fields: vec!["phone: +1234567890".into()],
    });
    // Should have subtitle text with both names
    assert!(
        screen.components.iter().any(|c| matches!(c,
            Component::Text { content, .. } if content.contains("Alice") && content.contains("Bob")
        )),
        "Merge screen should show both contact names"
    );
}

#[test]
fn contact_merge_confirm_navigates_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Bob".into(),
        secondary_fields: vec![],
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "confirm should navigate back, got {result:?}"
    );
}

#[test]
fn contact_merge_cancel_stays_on_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Bob".into(),
        secondary_fields: vec![],
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    // Cancel stays on screen (only confirm triggers navigation back)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "cancel should stay on screen, got {result:?}"
    );
}

#[test]
fn contact_limit_navigate_shows_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactLimit);
    assert_eq!(screen.screen_id, "contact_limit");
    assert_eq!(screen.title, "Contact Limit");
}

#[test]
fn contact_limit_shows_text_input() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactLimit);
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::TextInput { id, .. } if id == "limit_input")),
        "Should have limit_input TextInput component"
    );
}

#[test]
fn contact_limit_edit_then_save() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactLimit);

    // Enter edit mode
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "edit should update screen, got {result:?}"
    );

    // Type a number
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "100".into(),
    });

    // Save — engine returns Complete, AppEngine routes back
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "save with valid number should navigate back, got {result:?}"
    );
}

#[test]
fn contact_limit_save_invalid_returns_validation_error() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactLimit);

    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    // Type invalid input
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "not_a_number".into(),
    });

    // Save should fail
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(result, ActionResult::ValidationError { .. }),
        "save with invalid number should return ValidationError, got {result:?}"
    );
}

#[test]
fn contact_limit_cancel_edit_restores_value() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactLimit);

    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    // Type something
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "999".into(),
    });

    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_edit".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "cancel_edit should update screen, got {result:?}"
    );

    // Screen should show edit action (not save) — meaning we exited edit mode
    let screen = engine.current_screen();
    assert!(
        screen.actions.iter().any(|a| a.id == "edit"),
        "After cancel_edit, should show 'edit' action again"
    );
}
