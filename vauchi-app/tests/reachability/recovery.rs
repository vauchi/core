// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `RecoveryEngine`.
//!
//! Single-screen variant. All four `RecoveryStep` branches
//! (`Status`, `ShowClaimQr`, `CollectVouchers`, `Complete`) render
//! with the same `screen_id == "recovery_status"` — BFS dedup
//! would collapse them. This test pins the initial `Status`
//! step's affordance set; later steps' affordances
//! (`wait_for_voucher`, `cancel`, `submit_proof`, `done`) are
//! covered by `recovery_status.rs` inline tests.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{RecoveryEngine, WorkflowEngine};

/// Action ids consumed when `step == RecoveryStep::Status` —
/// `core/vauchi-app/src/ui/recovery_status.rs:336-344`.
const STATUS_STEP_HANDLED: &[&str] = &["start_recovery", "check_status"];

#[test]
fn recovery_initial_status_screen_is_reachable() {
    // Quorum threshold 3, no trusted contacts — minimal realistic
    // starting state (the same state users see before adding any
    // guardians).
    let engine = RecoveryEngine::new(Vec::new(), 3);
    assert_eq!(engine.current_screen().screen_id, "recovery_status");
    assert_reachability(&engine, STATUS_STEP_HANDLED);
}
