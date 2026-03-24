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

#[test]
fn group_detail_rename_shows_alert() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "rename".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Coming Soon");
            assert_eq!(
                message,
                "Group renaming will be available in a future update."
            );
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn group_detail_delete_shows_alert() {
    let mut engine = GroupDetailEngine::new("g1".into(), "Family".into(), sample_members());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_group".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Coming Soon");
            assert_eq!(
                message,
                "Group deletion will be available in a future update."
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
