// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DeviceLinkJoinEngine`.
//!
//! M5 B3 Slice 3. The join engine renders the fresh-device flow:
//! EnterName → PostingRequest → AwaitingResponse → Completing →
//! Complete | Failed. Only the EnterName screen exposes user
//! actions (`join`, `cancel`); the remaining steps are driven by
//! `DeviceLinkJoinUpdate` from the responder machine in
//! `AppEngine::device_link_responder`. CC-22 checks the action
//! affordances on the action-reachable screens.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    DEVICE_LINK_CANCEL_ACTION_ID, DEVICE_LINK_JOIN_ACTION_ID, DeviceLinkJoinEngine, WorkflowEngine,
};

/// Action ids emitted by the EnterName screen of `DeviceLinkJoinEngine`
/// (`core/vauchi-app/src/ui/device_link_join.rs`).
const HANDLED: &[&str] = &[DEVICE_LINK_JOIN_ACTION_ID, DEVICE_LINK_CANCEL_ACTION_ID];

fn factory() -> DeviceLinkJoinEngine {
    // Non-empty default name so the `join` action advances past the
    // EnterName gate during BFS.
    DeviceLinkJoinEngine::new("My Phone".to_string())
}

// @internal
#[test]
fn device_link_join_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "device_link_join");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn device_link_join_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
