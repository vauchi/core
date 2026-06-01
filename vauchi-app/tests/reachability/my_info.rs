// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `MyInfoEngine`.
//!
//! Single-screen own-card view (`my_info`): `ActionList`s of own
//! entries and group tabs, all `ListItemSelected` pass-throughs. The
//! preview-mode `exit-preview` action lives behind the same
//! `screen_id` (BFS dedup collapses it) and is covered by the
//! engine's inline tests. The default view exposes no `ScreenAction`.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{MyInfoEngine, MyInfoProgress, WorkflowEngine};

// @internal
#[test]
fn my_info_screen_is_reachable() {
    let engine = MyInfoEngine::new(MyInfoProgress {
        completed_steps: 1,
        total_steps: 3,
    });
    assert_eq!(engine.current_screen().screen_id, "my_info");
    assert_reachability(&engine, &["add_field", "preview-as-picker", "toggle_view"]);
}
