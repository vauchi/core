// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `SyncStatusEngine`.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{SyncStatusEngine, WorkflowEngine};

/// Full handler set for `SyncStatusEngine` —
/// `core/vauchi-app/src/ui/sync_status.rs:140`.
const HANDLED: &[&str] = &["sync_now", "test_connection"];

fn factory() -> SyncStatusEngine {
    SyncStatusEngine::new("https://relay.example".into(), 0, 0)
}

#[test]
fn sync_status_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "sync_status");
    assert_reachability_across_screens(factory, HANDLED);
}

#[test]
fn sync_status_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
