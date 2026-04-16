// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for group delete InlineConfirm behavior (SP-19 compliance).
//!
//! Both GroupDetailEngine and GroupsEngine use InlineConfirm → Complete
//! for irrevocable group deletion. AppEngine's handle_completion routes
//! Complete to vauchi.delete_group().

use vauchi_app::ui::{
    ActionResult, Component, ContactItem, GroupDetailEngine, GroupInfo, GroupsEngine, GroupsMode,
    UserAction, WorkflowEngine,
};

fn sample_members() -> Vec<ContactItem> {
    vec![ContactItem {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: None,
        avatar_initials: "A".into(),
        status: None,
        searchable_fields: vec![],
        a11y: None,
    }]
}

fn sample_groups() -> Vec<GroupInfo> {
    vec![
        GroupInfo {
            id: "g1".into(),
            name: "Family".into(),
            member_count: 3,
            visible_field_count: 2,
        },
        GroupInfo {
            id: "g2".into(),
            name: "Work".into(),
            member_count: 5,
            visible_field_count: 4,
        },
    ]
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

// @internal
#[test]
fn groups_engine_delete_tracks_group_id() {
    let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });

    // Verify pending_delete_group_id is set (first group)
    assert_eq!(engine.pending_delete_group_id(), Some("g1"));
}

// @internal
#[test]
fn groups_engine_confirm_delete_returns_complete() {
    let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_group".into(),
    });

    assert_eq!(result, ActionResult::Complete);
    // pending_delete_group_id is preserved for handle_completion to read
    assert!(engine.pending_delete_group_id().is_some());
}

// @internal
#[test]
fn groups_engine_cancel_delete_clears_group_id() {
    let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_group".into(),
    });

    assert_eq!(engine.pending_delete_group_id(), None);
}

// @internal
#[test]
fn groups_engine_as_any_downcasts() {
    let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
    let any = engine.as_any().expect("GroupsEngine must implement as_any");
    assert!(any.downcast_ref::<GroupsEngine>().is_some());
}
