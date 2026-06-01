// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DuplicateDetectionEngine`.
//!
//! Single-screen engine (`duplicate_detection`). With at least one
//! candidate pair the screen renders an `ActionList` of pairs plus a
//! `merge` / `dismiss` action pair. The list rows are
//! `ListItemSelected` pass-throughs; the only `ActionPressed` ids are
//! `merge` / `dismiss`, both consumed by
//! `DuplicateDetectionEngine::handle_action`
//! (`core/vauchi-app/src/ui/duplicate_detection.rs`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{DuplicateDetectionEngine, DuplicatePair, WorkflowEngine};

const HANDLED: &[&str] = &["merge", "dismiss"];

fn engine() -> DuplicateDetectionEngine {
    // One candidate pair so the merge/dismiss affordances render (the
    // empty-pairs screen is a no-duplicates info panel).
    DuplicateDetectionEngine::new(vec![DuplicatePair {
        id1: "c1".into(),
        name1: "Alice".into(),
        is_imported_1: false,
        id2: "c2".into(),
        name2: "Alice".into(),
        is_imported_2: true,
        similarity: 0.95,
    }])
}

// @internal
#[test]
fn duplicate_detection_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "duplicate_detection");
    assert_reachability(&engine, HANDLED);
}
