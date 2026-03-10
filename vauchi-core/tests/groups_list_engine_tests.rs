// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

fn sample_groups() -> Vec<ActionListItem> {
    vec![
        ActionListItem {
            id: "g1".into(),
            label: "Family".into(),
            icon: None,
            detail: Some("3 members".into()),
        },
        ActionListItem {
            id: "g2".into(),
            label: "Work".into(),
            icon: None,
            detail: Some("5 members".into()),
        },
    ]
}

#[test]
fn groups_list_screen_id() {
    let engine = GroupsEngine::new(sample_groups());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "groups_list");
}

#[test]
fn groups_list_title() {
    let engine = GroupsEngine::new(sample_groups());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Contact Groups");
}

#[test]
fn groups_list_shows_groups() {
    let engine = GroupsEngine::new(sample_groups());
    let screen = engine.current_screen();

    match &screen.components[0] {
        Component::ActionList { id, items } => {
            assert_eq!(id, "groups");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, "g1");
            assert_eq!(items[0].label, "Family");
            assert_eq!(items[0].detail.as_deref(), Some("3 members"));
            assert_eq!(items[1].id, "g2");
            assert_eq!(items[1].label, "Work");
        }
        other => panic!("Expected ActionList, got {other:?}"),
    }
}

#[test]
fn groups_list_create_group_shows_alert() {
    let mut engine = GroupsEngine::new(sample_groups());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_group".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Coming Soon");
            assert_eq!(
                message,
                "Group creation will be available in a future update."
            );
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn groups_list_unknown_action_returns_update_screen() {
    let mut engine = GroupsEngine::new(sample_groups());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "groups_list");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}
