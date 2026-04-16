// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Test helpers ────────────────────────────────────────────────────

fn sample_preview() -> MergePreview {
    MergePreview {
        primary_name: "Alice".into(),
        primary_fields: vec!["Email: alice@example.com".into(), "Phone: +1234".into()],
        secondary_name: "Alicia".into(),
        secondary_fields: vec!["Email: alicia@work.com".into()],
    }
}

fn make_engine() -> ContactMergeEngine {
    ContactMergeEngine::new(sample_preview())
}

// ── Tests ───────────────────────────────────────────────────────────

// @internal
#[test]
fn contact_merge_screen_id() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_merge");
}

// @internal
#[test]
fn contact_merge_title_is_merge_contacts() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Merge Contacts");
}

// @internal
#[test]
fn contact_merge_shows_both_contact_names() {
    let engine = make_engine();
    let screen = engine.current_screen();

    // The first Text component contains both names
    match &screen.components[0] {
        Component::Text { content, .. } => {
            assert!(
                content.contains("Alice"),
                "Expected content to contain 'Alice', got: {}",
                content
            );
            assert!(
                content.contains("Alicia"),
                "Expected content to contain 'Alicia', got: {}",
                content
            );
        }
        other => panic!("Expected Text component, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_merge_confirm_completes() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn contact_merge_cancel_stays_on_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_merge");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_merge_unknown_action_returns_update_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_merge");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}
