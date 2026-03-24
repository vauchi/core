// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

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

#[test]
fn groups_list_screen_id() {
    let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "groups_list");
}

#[test]
fn groups_list_title() {
    let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Groups");
}

#[test]
fn groups_list_shows_groups_with_member_counts() {
    let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
    let screen = engine.current_screen();

    // ActionList is the second component (after mode toggle)
    let action_list = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::ActionList { id, .. } if id == "groups"))
        .expect("should have groups ActionList");
    match action_list {
        Component::ActionList { id, items } => {
            assert_eq!(id, "groups");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, "g1");
            assert_eq!(items[0].label, "Family");
            assert_eq!(items[0].detail.as_deref(), Some("3 members"));
            assert_eq!(items[1].id, "g2");
            assert_eq!(items[1].label, "Work");
            assert_eq!(items[1].detail.as_deref(), Some("5 members"));
        }
        other => panic!("Expected ActionList, got {other:?}"),
    }
}

#[test]
fn groups_list_new_group_shows_alert() {
    let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "new_group".into(),
    });
    assert!(matches!(result, ActionResult::ShowAlert { .. }));
}

#[test]
fn groups_list_unknown_action_returns_update_screen() {
    let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
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
