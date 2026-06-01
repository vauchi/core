// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `RecoveryHelpEngine`.
//!
//! Multi-step guardian-help flow whose steps (`Info`, `PasteClaim`,
//! `ConfirmVoucher`, `ShowVoucher`) all render `screen_id ==
//! "recovery_help"`, so BFS `screen_id` dedup collapses them - the
//! single-screen shape, same as `recovery.rs`. Pins the initial
//! `Info` step, whose affordance is `vouch`. Later steps'
//! affordances (`verify_claim`, `cancel`, ...) share the screen_id
//! and are covered by the engine's inline tests.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{RecoveryHelpEngine, WorkflowEngine};

// @internal
#[test]
fn recovery_help_screen_is_reachable() {
    let engine = RecoveryHelpEngine::new();
    assert_eq!(engine.current_screen().screen_id, "recovery_help");
    assert_reachability(&engine, &["vouch"]);
}
