// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `GroupsEngine` (module `groups_list`).
//!
//! Single-screen engine (`groups_list`) with no `ActionPressed`
//! affordance: "Add group" is a body button (#446), so it and the
//! `groups` `ActionList` rows are `ListItemSelected` pass-throughs,
//! validated by `groups_screen_tests`. Rename/delete are per-group
//! affordances on `GroupDetail` (the list-level versions operated on
//! `groups.first()`, a wrong-group bug, and were removed in
//! `2026-06-05-screen-ux-declutter`); "Merge Groups" was an unimplemented
//! stub.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{GroupInfo, GroupsEngine, WorkflowEngine};

/// Action ids the base `groups_list` screen emits and
/// `GroupsEngine::handle_action` consumes -
/// `core/vauchi-app/src/ui/groups_list.rs`.
const HANDLED: &[&str] = &[];

fn engine() -> GroupsEngine {
    GroupsEngine::new(vec![
        GroupInfo {
            id: "g1".into(),
            name: "Work".into(),
            member_count: 3,
            entries_seen: 5,
        },
        GroupInfo {
            id: "g2".into(),
            name: "Friends".into(),
            member_count: 2,
            entries_seen: 4,
        },
    ])
}

// @internal
#[test]
fn groups_list_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "groups_list");
    assert_reachability(&engine, HANDLED);
}
