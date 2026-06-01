// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `SocialGraphEngine`.
//!
//! Single-screen read-only network view (`social_graph`): a summary
//! panel plus a `trust_filter` `ToggleList` (`ItemToggled`
//! pass-through) over a contact list (`ListItemSelected`
//! pass-through). No `ScreenAction`s.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{SocialGraphEngine, WorkflowEngine};

// @internal
#[test]
fn social_graph_screen_is_reachable() {
    let engine = SocialGraphEngine::new(Vec::new(), 0);
    assert_eq!(engine.current_screen().screen_id, "social_graph");
    assert_reachability(&engine, &[]);
}
