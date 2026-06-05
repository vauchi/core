// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `GroupsEngine` (module `groups_list`).
//!
//! Single-screen engine (`groups_list`). The base screen renders a
//! single `ScreenAction` - `new_group`. Rename/delete are per-group
//! affordances on `GroupDetail` (the list-level versions operated on
//! `groups.first()`, a wrong-group bug, and were removed in
//! `2026-06-05-screen-ux-declutter`); "Merge Groups" was an unimplemented
//! stub. The `mode_toggle` `ToggleList` and the `groups` `ActionList`
//! rows are pass-throughs (`ItemToggled` / `ListItemSelected`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{GroupInfo, GroupsEngine, GroupsMode, WorkflowEngine};

/// Action ids the base `groups_list` screen emits and
/// `GroupsEngine::handle_action` consumes -
/// `core/vauchi-app/src/ui/groups_list.rs`.
const HANDLED: &[&str] = &["new_group"];

fn engine() -> GroupsEngine {
    // Two groups so every action renders enabled (merge needs >= 2);
    // the walker emits the affordances regardless of enabled state.
    GroupsEngine::new(
        vec![
            GroupInfo {
                id: "g1".into(),
                name: "Work".into(),
                member_count: 3,
                visible_field_count: 5,
            },
            GroupInfo {
                id: "g2".into(),
                name: "Friends".into(),
                member_count: 2,
                visible_field_count: 4,
            },
        ],
        GroupsMode::Members,
    )
}

// @internal
#[test]
fn groups_list_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "groups_list");
    assert_reachability(&engine, HANDLED);
}
