// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact delete InlineConfirm behavior (SP-19 compliance).
//!
//! Imported contacts must use InlineConfirm → hard delete (irrevocable).
//! Exchanged contacts keep ShowToast + undo for archive (unchanged).

use vauchi_app::ui::{
    AccessibilityRole, ActionResult, Component, ContactDetailEngine, Field, Item,
    UiFieldVisibility, UserAction, WorkflowEngine,
};

fn sample_contact() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("+41 79 123 45 67".into()),
        avatar_initials: "A".into(),
        status: None,
        searchable_fields: vec![],
        actions: vec![],
        a11y: None,
    }
}

fn sample_fields() -> Vec<Field> {
    vec![Field {
        id: "f1".into(),
        field_type: "Phone".into(),
        label: "Mobile".into(),
        value: "+41 79 123 45 67".into(),
        visibility: UiFieldVisibility::Shown,
        a11y: None,
    }]
}

fn imported_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new()).with_imported(true)
}

fn exchanged_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new()).with_imported(false)
}

// @scenario: contact_detail.feature - Delete imported contact shows InlineConfirm
// @internal
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
// @internal
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
// @internal
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

// @scenario: accessibility :: ContactDetail personal note EditableText has populated a11y
//
// Verifies that the personal_note EditableText component carries a meaningful
// accessibility label and TextField role so screen readers can announce it.
// @internal
#[test]
fn contact_detail_personal_note_has_a11y() {
    let engine = exchanged_engine();
    let screen = engine.current_screen();

    let personal_note = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::EditableText { id, .. } if id == "personal_note"));

    let personal_note =
        personal_note.expect("Screen must contain a personal_note EditableText component");

    match personal_note {
        Component::EditableText { a11y, .. } => {
            let a11y = a11y
                .as_ref()
                .expect("personal_note EditableText must have a11y populated");
            assert_eq!(
                a11y.label.as_deref(),
                Some("Personal note, editable"),
                "a11y label must identify the field as editable"
            );
            assert_eq!(
                a11y.hint.as_deref(),
                Some("Double tap to edit"),
                "a11y hint must describe how to activate editing"
            );
            assert_eq!(
                a11y.role,
                Some(AccessibilityRole::TextField),
                "EditableText role must be TextField"
            );
        }
        other => panic!("expected EditableText, got {:?}", other),
    }
}

// @scenario: accessibility :: ContactDetail delete InlineConfirm has populated a11y
//
// Verifies that the delete confirmation InlineConfirm carries an Alert role
// and descriptive label so screen readers announce it as a destructive action.
// @internal
#[test]
fn contact_detail_delete_inline_confirm_has_a11y() {
    let mut engine = imported_engine();

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    let screen = engine.current_screen();
    let confirm = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "delete_contact"));

    let confirm = confirm.expect("Screen must contain delete_contact InlineConfirm");

    match confirm {
        Component::InlineConfirm { a11y, .. } => {
            let a11y = a11y
                .as_ref()
                .expect("delete InlineConfirm must have a11y populated");
            assert_eq!(
                a11y.label.as_deref(),
                Some("Confirm contact deletion"),
                "a11y label must identify the confirmation"
            );
            assert_eq!(
                a11y.role,
                Some(AccessibilityRole::Alert),
                "InlineConfirm for destructive action must have Alert role"
            );
        }
        other => panic!("expected InlineConfirm, got {:?}", other),
    }
}

// @scenario: contact_detail.feature - Archive exchanged contact still uses ShowToast
// @internal
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
