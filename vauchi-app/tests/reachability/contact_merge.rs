// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactMergeEngine`.
//!
//! Single-screen engine (`contact_merge`): a side-by-side preview of
//! two contacts with a `confirm` / `cancel` action pair. `confirm`
//! completes the merge; `cancel` refreshes. Both ids are consumed by
//! `ContactMergeEngine::handle_action`
//! (`core/vauchi-app/src/ui/contact_merge.rs`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ContactMergeEngine, MergePreview, WorkflowEngine};

const HANDLED: &[&str] = &["confirm", "cancel"];

fn engine() -> ContactMergeEngine {
    // Two same-named contacts (the realistic duplicate-merge case);
    // field lists are presentation-only and do not affect the
    // affordance set.
    ContactMergeEngine::new(MergePreview {
        primary_name: "Alice".into(),
        primary_fields: vec!["email: a@x.test".into()],
        secondary_name: "Alice".into(),
        secondary_fields: vec!["phone: 555".into()],
    })
}

// @internal
#[test]
fn contact_merge_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "contact_merge");
    assert_reachability(&engine, HANDLED);
}
