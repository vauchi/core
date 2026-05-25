// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery boundary regression tests.
//!
//! Targets surviving mutants in `recovery/mod.rs` that the comprehensive
//! suite did not pin: the rate limiter (previously untested) and the proof
//! version gate. Each test asserts an exact boundary so that flipping a
//! comparison or stubbing a return value fails.
//!
//! Based on: features/contact_recovery.feature

use vauchi_core::recovery::{
    RECOVERY_PROOF_VERSION, RecoveryError, RecoveryProof, RecoveryRateLimiter,
};

// =============================================================================
// RecoveryRateLimiter::check_rate_limit — exact boundaries
// =============================================================================

/// A claim below the per-window cap, inside an unexpired window, is allowed.
// @internal
#[test]
fn rate_limit_allows_claim_below_cap_within_window() {
    let limiter = RecoveryRateLimiter::new(5);
    // 0 seconds elapsed → window not expired; 4 < 5 → allowed.
    assert!(
        limiter.check_rate_limit(4, 1_000, 1_000),
        "4 claims with a cap of 5 inside the window must be allowed"
    );
}

/// A claim exactly at the cap, inside an unexpired window, is denied.
///
/// Pins the `claim_count < max` boundary: `<=` or `==` mutants would let the
/// cap-th claim through.
// @internal
#[test]
fn rate_limit_denies_claim_at_cap_within_window() {
    let limiter = RecoveryRateLimiter::new(5);
    // 0 seconds elapsed → window not expired; 5 < 5 is false → denied.
    assert!(
        !limiter.check_rate_limit(5, 1_000, 1_000),
        "the 5th claim with a cap of 5 inside the window must be denied"
    );
}

/// At exactly the 1-hour boundary the window has expired, so a claim is
/// allowed even when the count is already at the cap.
///
/// Pins the `elapsed >= 3600` boundary: a `<` mutant would treat the window
/// as still open and deny the claim.
// @internal
#[test]
fn rate_limit_resets_at_one_hour_boundary() {
    let limiter = RecoveryRateLimiter::new(5);
    // elapsed == 3600 → window expired → allowed regardless of count.
    assert!(
        limiter.check_rate_limit(5, 0, 3600),
        "a window exactly 3600s old must be treated as expired and reset"
    );
    // One second short of expiry, still within the window → count still caps.
    assert!(
        !limiter.check_rate_limit(5, 0, 3599),
        "a window 3599s old must still be open and deny the capped claim"
    );
}

// =============================================================================
// RecoveryProof::validate_version — version gate
// =============================================================================

/// A proof at the current schema version validates.
///
/// Pins the strict `>` in `version > RECOVERY_PROOF_VERSION`: a `>=` mutant
/// would reject a proof at the current version.
// @internal
#[test]
fn validate_version_accepts_current_version() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let proof = RecoveryProof::new(&old_pk, &new_pk, 3, 1_700_000_000);

    assert_eq!(proof.version(), RECOVERY_PROOF_VERSION);
    assert!(
        proof.validate_version().is_ok(),
        "a proof at the current schema version must validate"
    );
}

/// A proof serialized by a newer schema version is rejected.
///
/// `version` is the first field in the postcard encoding, so flipping the
/// first byte yields a proof one version ahead. Pins both the `Ok(())` stub
/// and the comparison-operator mutants on the version check.
// @internal
#[test]
fn validate_version_rejects_future_version() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let proof = RecoveryProof::new(&old_pk, &new_pk, 3, 1_700_000_000);

    let mut bytes = proof.to_bytes().expect("proof must serialize");
    bytes[0] = RECOVERY_PROOF_VERSION + 1;
    let future = RecoveryProof::from_bytes(&bytes).expect("bumped proof must deserialize");

    assert_eq!(
        future.version(),
        RECOVERY_PROOF_VERSION + 1,
        "byte flip must produce a proof one schema version ahead"
    );
    assert!(
        matches!(
            future.validate_version(),
            Err(RecoveryError::InvalidProof(_))
        ),
        "a proof from a newer schema version must be rejected as InvalidProof"
    );
}
