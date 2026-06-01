// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `HelpEngine`.
//!
//! Single-screen help index (`help`): a `help_search` `TextInput`
//! (`TextChanged` pass-through) over an `ActionList` of topics
//! (`ListItemSelected` pass-through). No `ScreenAction`s.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{HelpEngine, WorkflowEngine};

// @internal
#[test]
fn help_screen_is_reachable() {
    let engine = HelpEngine::new(Vec::new());
    assert_eq!(engine.current_screen().screen_id, "help");
    assert_reachability(&engine, &[]);
}
