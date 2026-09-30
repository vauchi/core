// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `GroupsEngine` through the public `vauchi_app::ui::*` re-exports. Rows,
//! Add group and the empty state are pinned in vauchi-app's
//! `groups_screen_tests` (#446).

use vauchi_app::ui::*;

fn sample_groups() -> Vec<GroupInfo> {
    vec![GroupInfo {
        id: "g1".into(),
        name: "Family".into(),
        member_count: 3,
        entries_seen: 2,
    }]
}

// @internal
#[test]
fn groups_list_screen_id_and_title() {
    let screen = GroupsEngine::new(sample_groups()).current_screen();
    assert_eq!(screen.screen_id, "groups_list");
    assert_eq!(screen.title, "Groups");
}

// @internal
#[test]
fn groups_list_stale_toolbar_actions_are_inert() {
    // "new_group" was the toolbar action before #446 moved Add group into
    // the body; a stale shell emitting it must not open a dialog.
    let mut engine = GroupsEngine::new(sample_groups());
    for stale in ["new_group", "rename_group", "delete_group", "unknown"] {
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: stale.into(),
        });
        match result {
            ActionResult::UpdateScreen(screen) => assert_eq!(screen.screen_id, "groups_list"),
            other => panic!("stale `{stale}` must re-render, got {other:?}"),
        }
    }
}
