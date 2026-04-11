// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact delete InlineConfirm behavior (SP-19 compliance).
//!
//! Imported contacts must use InlineConfirm → hard delete (irrevocable).
//! Exchanged contacts keep ShowToast + undo for archive (unchanged).

use vauchi_app::ui::{
    ActionResult, Component, ContactDetailEngine, ContactItem, FieldDisplay, UiFieldVisibility,
    UserAction, WorkflowEngine,
};

fn sample_contact() -> ContactItem {
    ContactItem {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("+41 79 123 45 67".into()),
        avatar_initials: "A".into(),
        status: None,
        searchable_fields: vec![],
        a11y: None,
    }
}

fn sample_fields() -> Vec<FieldDisplay> {
    vec![FieldDisplay {
        id: "f1".into(),
        field_type: "Phone".into(),
        label: "Mobile".into(),
        value: "+41 79 123 45 67".into(),
        visibility: UiFieldVisibility::Shown,
    }]
}

fn imported_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new()).with_imported(true)
}

fn exchanged_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new()).with_imported(false)
}

// @scenario: contact_detail.feature - Delete imported contact shows InlineConfirm
#[test]
fn delete_contact_shows_inline_confirm() {
    let mut engine = imported_engine();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    // Must update screen (not ShowToast)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "delete_contact on imported contact must return UpdateScreen, got: {result:?}"
    );

    // Screen must contain InlineConfirm component
    let screen = engine.current_screen();
    let has_inline_confirm = screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "delete_contact" && *destructive)
    });
    assert!(
        has_inline_confirm,
        "Screen must contain a destructive InlineConfirm with id 'delete_contact'"
    );
}

// @scenario: contact_detail.feature - Confirm delete completes engine
#[test]
fn confirm_delete_contact_completes() {
    let mut engine = imported_engine();

    // First trigger the InlineConfirm
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    // Then confirm
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_contact".into(),
    });

    assert_eq!(
        result,
        ActionResult::Complete,
        "confirm_delete_contact must return Complete"
    );
}

// @scenario: contact_detail.feature - Cancel delete removes InlineConfirm
#[test]
fn cancel_delete_contact_removes_inline_confirm() {
    let mut engine = imported_engine();

    // Trigger InlineConfirm
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_contact".into(),
    });

    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "cancel must return UpdateScreen"
    );

    // InlineConfirm must be gone
    let screen = engine.current_screen();
    let has_inline_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { .. }));
    assert!(
        !has_inline_confirm,
        "InlineConfirm must be removed after cancel"
    );
}

// @scenario: contact_detail.feature - Archive exchanged contact still uses ShowToast
#[test]
fn archive_contact_still_uses_show_toast() {
    let mut engine = exchanged_engine();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "archive_contact".into(),
    });

    assert_eq!(
        result,
        ActionResult::ShowToast {
            message: "Contact archived".into(),
            undo_action_id: Some("undo_archive_contact:c1".into()),
        },
        "archive_contact must still return ShowToast with undo"
    );
}
