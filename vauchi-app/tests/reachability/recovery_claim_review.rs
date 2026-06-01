// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `RecoveryClaimReviewEngine`.
//!
//! Every `ReviewStep` (`Review`, `VerifyOutOfBand`, `ConfirmAccept`,
//! `ShowVoucherQr`, `Done`) renders with the same
//! `screen_id == "recovery_claim_review"`, so BFS `screen_id` dedup
//! collapses them - the single-screen variant, same as `recovery.rs`.
//! This test pins the initial `Review` step for the
//! `(Vouching, High)` claim, whose affordances are `vouch` / `reject`.
//!
//! Affordances on the other steps and other `(mode, confidence)`
//! claims - `accept`, `remind`, `verify_other`, `accept_anyway`,
//! `confirm_accept`, `cancel`, `done` - share the one screen_id and
//! are covered by `core/vauchi-core/tests/it/recovery_engine_tests.rs`
//! and the engine's inline tests. Declaring them here would make them
//! orphan handlers.

use vauchi_app::ui::WorkflowEngine;
use vauchi_app::ui::recovery_claim_review::{
    ClaimContext, Confidence, RecoveryClaimReviewEngine, ReviewMode,
};
use vauchi_app::ui::testing::assert_reachability;

/// Action ids the initial `Review` screen emits for a
/// `(Vouching, High)` claim, consumed by
/// `RecoveryClaimReviewEngine::handle_action` -
/// `core/vauchi-app/src/ui/recovery_claim_review.rs`.
const HANDLED: &[&str] = &["vouch", "reject"];

fn engine() -> RecoveryClaimReviewEngine {
    // Guardian reviewing a high-confidence claim (quorum already met):
    // the review screen offers vouch / reject.
    RecoveryClaimReviewEngine::new(
        ReviewMode::Vouching,
        ClaimContext {
            contact_name: "Alice".into(),
            old_pk_fingerprint: "ab12cd34ef56".into(),
            mutual_voucher_count: 3,
            threshold: 2,
            confidence: Confidence::High,
        },
    )
}

// @internal
#[test]
fn recovery_claim_review_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "recovery_claim_review");
    assert_reachability(&engine, HANDLED);
}
