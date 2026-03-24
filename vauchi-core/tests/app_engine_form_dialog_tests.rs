// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine FormDialog tests: add/edit field, edit name, edit relay URL,
//! submission, validation errors, and navigation after completion.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, FormDialogType, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::FieldType;

// ── FormDialogEngine tests ────────────────────────────────────────────

#[test]
fn form_dialog_add_field_shows_type_list() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });
    assert_eq!(screen.screen_id, "form_add_field");
    let has_action_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ActionList { id, .. } if id == "entry_types"));
    assert!(
        has_action_list,
        "Should have an ActionList for entry type selection"
    );
}

#[test]
fn form_dialog_add_field_type_selection_shows_value_inputs() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select "email" type from flat list
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_add_field");
            let text_inputs: Vec<_> = screen
                .components
                .iter()
                .filter(|c| matches!(c, Component::TextInput { .. }))
                .collect();
            assert_eq!(
                text_inputs.len(),
                3,
                "Should have 3 text inputs (value + display name + comment)"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_edit_name_tracks_text_changes() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Old Name".into(),
        },
    });
    // Change the display name
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "New Name".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_edit_name");
            // Verify the TextInput now shows "New Name"
            let has_new_value = screen.components.iter().any(|c| {
                matches!(c, Component::TextInput { id, value, .. } if id == "display_name" && value == "New Name")
            });
            assert!(has_new_value, "TextInput should reflect updated value");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_submit_navigates_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    // Navigate to Home first, then to the form — so back goes to Home
    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditRelayUrl {
            current_url: "wss://old.relay".into(),
        },
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    // AppEngine intercepts Complete and navigates back
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "my_info");
        }
        other => panic!("Expected NavigateTo(home), got {other:?}"),
    }
}

// ── FormDialog completion tests ──────────────────────────────────────

#[test]
fn form_dialog_edit_name_saves_display_name() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to FormDialog for EditName
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });

    // Type new name
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Bob".into(),
    });

    // Submit — should save and navigate back
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditName submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_edit_name_empty_returns_validation_error() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });

    // Clear the name (set to empty)
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(
            result,
            ActionResult::ValidationError {
                ref component_id,
                ..
            } if component_id == "display_name"
        ),
        "Empty name should return ValidationError, got {result:?}"
    );
}

#[test]
fn form_dialog_add_field_saves_to_own_card() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select type, then enter value
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "phone".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+41 79 123 45 67".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "AddField submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_add_field_empty_value_returns_validation_error() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select a type first
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "phone".into(),
    });

    // Leave value empty, submit
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(
            result,
            ActionResult::ValidationError {
                ref component_id,
                ..
            } if component_id == "field_value"
        ),
        "Empty value should return ValidationError, got {result:?}"
    );
}

#[test]
fn form_dialog_edit_field_saves_value() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    // Add a field first so we have a field_id to edit
    let field = vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Phone,
        "Phone",
        "+41 79 000 00 00",
    );
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();

    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditField {
            field_id: field_id.clone(),
            field_label: "Phone".into(),
            current_value: "+1 555 123 4567".into(),
            current_note: None,
        },
    });

    // Change value
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+41 79 999 99 99".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditField submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_cancel_navigates_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to MyInfo first so back-stack has something
    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    // Cancel should navigate back to MyInfo
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "my_info");
        }
        other => panic!("Expected NavigateTo(my_info), got {other:?}"),
    }
}

#[test]
fn form_dialog_cancel_does_not_save_modified_name() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });

    // User types a new name
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Eve".into(),
    });

    // User presses Cancel — should NOT save "Eve"
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // Verify the name is still "Alice" — cancel must not persist
    let identity = engine.vauchi().identity().unwrap();
    assert_eq!(identity.display_name(), "Alice");
}

#[test]
fn form_dialog_edit_relay_url_navigates_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditRelayUrl {
            current_url: "wss://relay.vauchi.app".into(),
        },
    });

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "relay_url".into(),
        value: "wss://custom.relay.example.com".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    // EditRelayUrl is TUI-specific config — AppEngine just navigates back
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditRelayUrl submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_cancel_add_field_does_not_save() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let field_count_before = engine
        .vauchi()
        .own_card()
        .unwrap()
        .map(|c| c.fields().len())
        .unwrap_or(0);

    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // User fills in field data
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+41 79 000 00 00".into(),
    });

    // User presses Cancel — field must NOT be added
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    assert!(matches!(result, ActionResult::NavigateTo(_)));

    let field_count_after = engine
        .vauchi()
        .own_card()
        .unwrap()
        .map(|c| c.fields().len())
        .unwrap_or(0);
    assert_eq!(
        field_count_before, field_count_after,
        "cancel must not add a field"
    );
}

// ── Social network field routing tests ──────────────────────────────

/// @scenario: contact_card_management :: Add social network field from catalog
#[test]
fn form_dialog_add_social_field_stores_as_social_type() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select "social:github" from the catalog type list
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "social:github".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "octocat".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "Should navigate back after submit, got {result:?}"
    );

    // Verify: field is stored as FieldType::Social, NOT Custom
    let card = engine.vauchi().own_card().unwrap().unwrap();
    let social_fields: Vec<_> = card
        .fields()
        .iter()
        .filter(|f| f.field_type() == FieldType::Social)
        .collect();
    assert_eq!(
        social_fields.len(),
        1,
        "Expected 1 Social field, got {} (fields: {:?})",
        social_fields.len(),
        card.fields()
            .iter()
            .map(|f| format!("{:?}:{}", f.field_type(), f.label()))
            .collect::<Vec<_>>()
    );
    assert_eq!(social_fields[0].value(), "octocat");
}

/// @scenario: contact_card_management :: Social field label uses display name
#[test]
fn form_dialog_add_social_field_uses_display_name_as_label() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select "social:github", don't set a custom label
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "social:github".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "torvalds".into(),
    });

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    let card = engine.vauchi().own_card().unwrap().unwrap();
    let social_field = card
        .fields()
        .iter()
        .find(|f| f.field_type() == FieldType::Social)
        .expect("Should have a Social field");

    assert_eq!(
        social_field.label(),
        "GitHub",
        "Label should be 'GitHub' (catalog display name), not '{}'",
        social_field.label()
    );
}

// ── field_note persistence tests ──────────────────────────────────────

/// AddField: a note entered in field_note is persisted on the ContactField.
#[test]
fn form_dialog_add_field_with_note_persists_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "alice@example.com".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_note".into(),
        value: "Work account".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "AddField submit should navigate back, got {result:?}"
    );

    let card = engine.vauchi().own_card().unwrap().unwrap();
    let email_field = card
        .fields()
        .iter()
        .find(|f| f.value() == "alice@example.com")
        .expect("Email field should have been added");

    assert_eq!(
        email_field.note(),
        Some("Work account"),
        "Field note should be 'Work account', got {:?}",
        email_field.note()
    );
}

/// AddField: an empty note leaves the field note as None.
#[test]
fn form_dialog_add_field_without_note_leaves_note_none() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "bob@example.com".into(),
    });
    // No field_note change — note stays empty

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    let card = engine.vauchi().own_card().unwrap().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.value() == "bob@example.com")
        .expect("Email field should have been added");

    assert_eq!(
        field.note(),
        None,
        "Field note should be None when not set, got {:?}",
        field.note()
    );
}

/// EditField: a note entered in field_note is persisted on the existing ContactField.
#[test]
fn form_dialog_edit_field_with_note_persists_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let field = vauchi_core::contact_card::ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+41 79 000 00 00",
    );
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();

    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditField {
            field_id: field_id.clone(),
            field_label: "Mobile".into(),
            current_value: "+41 79 000 00 00".into(),
            current_note: None,
        },
    });

    // Keep same value, set a note
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+41 79 000 00 00".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_note".into(),
        value: "Private number".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditField submit should navigate back, got {result:?}"
    );

    let card = engine.vauchi().own_card().unwrap().unwrap();
    let updated = card
        .fields()
        .iter()
        .find(|f| f.id() == field_id)
        .expect("Field should still exist");

    assert_eq!(
        updated.note(),
        Some("Private number"),
        "Field note should be 'Private number', got {:?}",
        updated.note()
    );
}

/// EditField: an existing note is pre-populated in the form's field_note input.
#[test]
fn form_dialog_edit_field_prefills_current_note() {
    let engine = vauchi_app::ui::FormDialogEngine::new(FormDialogType::EditField {
        field_id: "f1".into(),
        field_label: "Phone".into(),
        current_value: "+1 555 0000".into(),
        current_note: Some("Office direct line".into()),
    });
    let screen = engine.current_screen();

    let note_prefilled = screen.components.iter().any(|c| {
        matches!(c,
            Component::TextInput { id, value, .. }
                if id == "field_note" && value == "Office direct line"
        )
    });
    assert!(
        note_prefilled,
        "field_note input should be prefilled with existing note"
    );
}
