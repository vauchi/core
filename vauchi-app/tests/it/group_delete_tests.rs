// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for group delete InlineConfirm behavior (SP-19 compliance).
//!
//! Both GroupDetailEngine and GroupsEngine use InlineConfirm → Complete
//! for irrevocable group deletion. AppEngine's handle_completion routes
//! Complete to vauchi.delete_group().

use vauchi_app::ui::{
    ActionResult, Component, GroupDetailEngine, Item, UserAction, WorkflowEngine,
};

fn sample_members() -> Vec<Item> {
    vec![Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: None,
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    }]
}

// --- GroupDetailEngine tests ---

// @internal
#[test]
fn group_detail_delete_shows_inline_confirm() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    assert!(matches!(result, ActionResult::UpdateScreen(_)));

    let screen = engine.current_screen();
    let has_confirm = screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "delete_group" && *destructive)
    });
    assert!(
        has_confirm,
        "GroupDetail must show destructive InlineConfirm"
    );
}

// @internal
#[test]
fn group_detail_confirm_delete_completes() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_group".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn group_detail_cancel_delete_removes_confirm() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_group".into(),
    });

    let screen = engine.current_screen();
    let has_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { .. }));
    assert!(!has_confirm, "InlineConfirm must be removed after cancel");
}

// --- GroupsEngine tests ---

// Group deletion is now a per-group action on GroupDetail (covered above).
// The former list-level GroupsEngine delete (pending_delete_group_id +
// confirm/cancel) was removed — it deleted `groups.first()`, the wrong
// group — in 2026-06-05-screen-ux-declutter.
