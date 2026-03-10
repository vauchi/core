// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

// ── Test helpers ────────────────────────────────────────────────────

fn sample_contact() -> ContactItem {
    ContactItem {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("alice@example.com".into()),
        avatar_initials: "AL".into(),
        status: None,
    }
}

fn sample_fields() -> Vec<FieldDisplay> {
    vec![FieldDisplay {
        id: "f1".into(),
        label: "Email".into(),
        value: "alice@example.com".into(),
        field_type: "email".into(),
        visibility: UiFieldVisibility::Shown,
    }]
}

fn make_detail_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields())
}

// ── ContactDetailEngine tests ───────────────────────────────────────

#[test]
fn contact_detail_screen_id() {
    let engine = make_detail_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_detail");
}

#[test]
fn contact_detail_shows_contact_name_as_title() {
    let engine = make_detail_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Alice");
}

#[test]
fn contact_detail_edit_returns_edit_contact() {
    let mut engine = make_detail_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    match result {
        ActionResult::EditContact { contact_id } => {
            assert_eq!(contact_id, "c1");
        }
        other => panic!("Expected EditContact, got {:?}", other),
    }
}

#[test]
fn contact_detail_back_completes() {
    let mut engine = make_detail_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn contact_detail_unknown_action_returns_update_screen() {
    let mut engine = make_detail_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_detail");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// ── ContactNotFoundEngine tests ─────────────────────────────────────

#[test]
fn contact_not_found_screen_id() {
    let engine = ContactNotFoundEngine::new("missing_id".into());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_not_found");
}

#[test]
fn contact_not_found_shows_contact_id_in_error() {
    let engine = ContactNotFoundEngine::new("xyz_123".into());
    let screen = engine.current_screen();

    match &screen.components[0] {
        Component::InfoPanel { items, .. } => {
            let error_item = &items[0];
            assert!(
                error_item.detail.contains("xyz_123"),
                "Error detail should contain the contact id, got: {}",
                error_item.detail
            );
        }
        other => panic!("Expected InfoPanel, got {:?}", other),
    }
}

#[test]
fn contact_not_found_back_completes() {
    let mut engine = ContactNotFoundEngine::new("missing".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}
