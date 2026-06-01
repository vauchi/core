// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `SupportEngine`.
//!
//! Single-screen donation panel (`support`) with two outbound-link
//! actions, both consumed by `SupportEngine::handle_action`
//! (`core/vauchi-app/src/ui/support.rs`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{SupportEngine, WorkflowEngine};

// @internal
#[test]
fn support_screen_is_reachable() {
    let engine = SupportEngine::new();
    assert_eq!(engine.current_screen().screen_id, "support");
    assert_reachability(&engine, &["open_github_sponsors", "open_liberapay"]);
}
