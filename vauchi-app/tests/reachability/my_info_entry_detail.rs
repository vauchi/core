// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `MyInfoEntryDetailEngine`.
//!
//! Single-screen own-field detail (`my_info_entry_detail`) with
//! `edit` / `delete` actions; the `group_visibility` toggles are
//! `ItemToggled` pass-throughs. Both actions are consumed by
//! `MyInfoEntryDetailEngine::handle_action`
//! (`core/vauchi-app/src/ui/my_info_entry_detail.rs`).
//!
//! The footer `back` action was dropped in the Goal 3 back-chrome
//! sweep (`2026-06-05-core-driven-back-chrome`): this is a pushed
//! sub-screen, so `AppEngine::can_go_back` is true and the frontends
//! render a core-driven back affordance — the footer button duplicated
//! it.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{MyInfoEntryDetailEngine, WorkflowEngine};

// @internal
#[test]
fn my_info_entry_detail_screen_is_reachable() {
    let engine = MyInfoEntryDetailEngine::new(
        "f1".into(),
        "email".into(),
        "Email".into(),
        "alice@example.test".into(),
        None,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(engine.current_screen().screen_id, "my_info_entry_detail");
    assert_reachability(&engine, &["edit", "delete"]);
}
