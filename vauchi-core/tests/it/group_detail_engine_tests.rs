// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn sample_members() -> Vec<ContactItem> {
    vec![
        ContactItem {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: None,
            avatar_initials: "AL".into(),
            status: None,
            searchable_fields: vec![],
            actions: vec![],
            a11y: None,
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "BO".into(),
            status: None,
            searchable_fields: vec![],
            actions: vec![],
            a11y: None,
        },
    ]
}

// @internal
#[test]
fn group_detail_screen_id() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "group_detail");
}

// @internal
#[test]
fn group_detail_title_is_group_name() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Family");
}

// @internal
#[test]
fn group_detail_shows_member_count() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();

    let detail = find_info_detail(&screen, "group_info", "Members");
    assert_eq!(detail, "2");
}

// @internal
// @internal
#[test]
fn group_detail_rename_opens_form_dialog() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "rename".into(),
    });
    match result {
        ActionResult::ShowFormDialog {
            dialog_type,
            context_id,
        } => {
            assert_eq!(dialog_type, "rename_group");
            assert_eq!(context_id, Some("g1".into()));
        }
        other => panic!("Expected ShowFormDialog, got {other:?}"),
    }
}

// @internal — ADR-022: destructive actions use InlineConfirm, not ShowAlert
// @internal
#[test]
fn group_detail_delete_shows_inline_confirm() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("Expected UpdateScreen with InlineConfirm, got {result:?}");
    };
    let has_inline_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { destructive, .. } if *destructive));
    assert!(
        has_inline_confirm,
        "delete_group should show a destructive InlineConfirm"
    );
}

// @internal
// @internal
#[test]
fn group_detail_confirm_delete_completes() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    // First trigger the delete to enter pending state
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    // Then confirm
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_group".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "confirm_delete_group should return Complete, got {result:?}"
    );
}

// @internal
// @internal
#[test]
fn group_detail_cancel_delete_removes_inline_confirm() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    // Trigger delete
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_group".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("Expected UpdateScreen, got {result:?}");
    };
    let has_inline_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { .. }));
    assert!(!has_inline_confirm, "cancel should remove InlineConfirm");
}

// @internal
#[test]
fn group_detail_unknown_action_returns_update_screen() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "group_detail");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @internal
#[test]
fn group_detail_has_preview_as_member_actions() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();

    // Each member should have a corresponding "preview-as-member:<id>" action.
    let action_ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert!(
        action_ids.contains(&"preview-as-member:c1"),
        "Expected action 'preview-as-member:c1', got: {action_ids:?}"
    );
    assert!(
        action_ids.contains(&"preview-as-member:c2"),
        "Expected action 'preview-as-member:c2', got: {action_ids:?}"
    );
}

// @internal
#[test]
fn group_detail_preview_as_member_label_contains_name() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();

    let alice_action = screen
        .actions
        .iter()
        .find(|a| a.id == "preview-as-member:c1")
        .expect("preview-as-member:c1 action missing");
    assert!(
        alice_action.label.contains("Alice"),
        "Label should contain member name 'Alice', got: '{}'",
        alice_action.label
    );
}

// @internal
#[test]
fn group_detail_preview_as_member_action_returns_preview_as() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "preview-as-member:c1".into(),
    });
    match result {
        ActionResult::PreviewAs { contact_id } => {
            assert_eq!(contact_id, "c1");
        }
        other => panic!("Expected PreviewAs {{ contact_id: \"c1\" }}, got {other:?}"),
    }
}

// @internal
#[test]
fn group_detail_no_preview_as_actions_when_no_members() {
    let engine = GroupDetailEngine::new("g1".into(), "Empty Group".into(), vec![]);
    let screen = engine.current_screen();

    let preview_actions: Vec<&str> = screen
        .actions
        .iter()
        .filter(|a| a.id.starts_with("preview-as-member:"))
        .map(|a| a.id.as_str())
        .collect();
    assert!(
        preview_actions.is_empty(),
        "Expected no preview-as actions for empty group, got: {preview_actions:?}"
    );
}

// --- helpers ---

fn find_info_detail(screen: &ScreenModel, panel_id: &str, item_title: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == panel_id => items
                .iter()
                .find(|item| item.title == item_title)
                .map(|item| item.detail.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("InfoItem '{item_title}' not found in panel '{panel_id}'"))
}
