// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Tests: ContactListEngine has find_duplicates action ─────────────

#[test]
fn contacts_screen_has_find_duplicates_action() {
    let engine = ContactListEngine::new(vec![]);
    let screen = engine.current_screen();
    let action = screen
        .actions
        .iter()
        .find(|a| a.id == "find_duplicates")
        .expect("Contacts screen should have find_duplicates action");
    assert_eq!(action.label, "Find Duplicates");
}

// ── Test helpers ────────────────────────────────────────────────────

fn sample_pairs() -> Vec<DuplicatePair> {
    vec![
        DuplicatePair {
            id1: "c1".into(),
            name1: "Alice".into(),
            id2: "c2".into(),
            name2: "Alicia".into(),
            similarity: 0.85,
        },
        DuplicatePair {
            id1: "c3".into(),
            name1: "Bob".into(),
            id2: "c4".into(),
            name2: "Robert".into(),
            similarity: 0.72,
        },
    ]
}

fn make_engine_with_pairs() -> DuplicateDetectionEngine {
    DuplicateDetectionEngine::new(sample_pairs())
}

fn make_empty_engine() -> DuplicateDetectionEngine {
    DuplicateDetectionEngine::new(vec![])
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn duplicate_detection_screen_id() {
    let engine = make_engine_with_pairs();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "duplicate_detection");
}

#[test]
fn duplicate_detection_empty_shows_no_duplicates() {
    let engine = make_empty_engine();
    let screen = engine.current_screen();

    match &screen.components[0] {
        Component::Text { content, .. } => {
            assert!(
                content.contains("No duplicate"),
                "Expected 'No duplicate' message, got: {}",
                content
            );
        }
        other => panic!("Expected Text component, got {:?}", other),
    }
}

#[test]
fn duplicate_detection_empty_disables_actions() {
    let engine = make_empty_engine();
    let screen = engine.current_screen();

    for action in &screen.actions {
        assert!(
            !action.enabled,
            "Action '{}' should be disabled when no pairs exist",
            action.id
        );
    }
}

#[test]
fn duplicate_detection_with_pairs_shows_count() {
    let engine = make_engine_with_pairs();
    let screen = engine.current_screen();

    match &screen.components[0] {
        Component::Text { content, .. } => {
            assert!(
                content.contains("2"),
                "Expected count '2' in header, got: {}",
                content
            );
        }
        other => panic!("Expected Text header component, got {:?}", other),
    }
}

#[test]
fn duplicate_detection_merge_completes() {
    let mut engine = make_engine_with_pairs();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn duplicate_detection_dismiss_stays_on_screen() {
    let mut engine = make_engine_with_pairs();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "dismiss".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "duplicate_detection");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

#[test]
fn duplicate_detection_unknown_action_returns_update_screen() {
    let mut engine = make_engine_with_pairs();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "duplicate_detection");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}
