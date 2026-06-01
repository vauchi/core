// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ActivityLogEngine`.
//!
//! Single-screen read-only log (`activity_log`): an `ActionList` of
//! entries (or an empty-state panel) with no `ScreenAction`s. The
//! list rows are `ListItemSelected` pass-throughs, so the screen
//! exposes no `ActionPressed` affordance.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ActivityLogEngine, WorkflowEngine};

// @internal
#[test]
fn activity_log_screen_is_reachable() {
    let engine = ActivityLogEngine::new(Vec::new());
    assert_eq!(engine.current_screen().screen_id, "activity_log");
    assert_reachability(&engine, &[]);
}
