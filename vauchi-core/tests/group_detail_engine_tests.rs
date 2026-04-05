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
        },
        ContactItem {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "BO".into(),
            status: None,
            searchable_fields: vec![],
        },
    ]
}

#[test]
fn group_detail_screen_id() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "group_detail");
}

#[test]
fn group_detail_title_is_group_name() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Family");
}

#[test]
fn group_detail_shows_member_count() {
    let engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let screen = engine.current_screen();

    let detail = find_info_detail(&screen, "group_info", "Members");
    assert_eq!(detail, "2");
}

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

// @internal
#[test]
fn group_detail_delete_shows_confirmation() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Delete Group?");
            assert!(
                message.contains("Family"),
                "Expected group name in message, got: {message}"
            );
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

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
