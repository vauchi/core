// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `FingerprintVerifyEngine`.
//!
//! Single-screen engine (`fingerprint_verify`) whose action set
//! depends on the `is_verified` construction flag: the unverified
//! screen offers `confirm_match` / `back`, the verified screen offers
//! `unverify` / `back`. Both branches are pinned (one test each), so
//! every `handle_action` arm
//! (`core/vauchi-app/src/ui/fingerprint_verify.rs`) is reachable from
//! some rendered affordance.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{FingerprintVerifyEngine, WorkflowEngine};

// @internal
#[test]
fn unverified_screen_offers_confirm_match() {
    // Not yet verified: the screen renders `confirm_match` / `back`.
    let engine = FingerprintVerifyEngine::new("contact-1", "AA:BB:CC", "DD:EE:FF", false);
    assert_eq!(engine.current_screen().screen_id, "fingerprint_verify");
    assert_reachability(&engine, &["confirm_match", "back"]);
}

// @internal
#[test]
fn verified_screen_offers_unverify() {
    // Already verified: the screen renders `unverify` / `back`.
    let engine = FingerprintVerifyEngine::new("contact-1", "AA:BB:CC", "DD:EE:FF", true);
    assert_eq!(engine.current_screen().screen_id, "fingerprint_verify");
    assert_reachability(&engine, &["unverify", "back"]);
}
