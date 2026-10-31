// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery boundary regression tests.
//!
//! Targets surviving mutants in `recovery/mod.rs` that the comprehensive
//! suite did not pin: the rate limiter (previously untested) and the proof
//! version gate. Each test asserts an exact boundary so that flipping a
//! comparison or stubbing a return value fails.
//!
//! Based on: features/contact_recovery.feature

use vauchi_core::identifiers::IdentityKey;
use vauchi_core::identity::Identity;
use vauchi_core::recovery::{
    RECOVERY_PROOF_VERSION, RecoveryError, RecoveryProof, RecoveryRateLimiter, RecoveryVoucher,
    SEALED_RECOVERY_PROOF_VERSION,
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

// =============================================================================
// The form the relay stores
// =============================================================================

const NOW: u64 = 1_700_000_000;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A complete proof from Alice's old key to her new one, vouched by three
/// helpers whose identity keys are the social graph the relay must not see.
fn proof_with_three_helpers() -> (RecoveryProof, Identity, Vec<IdentityKey>) {
    let old = Identity::create("Alice", 0);
    let new = Identity::create("Alice again", 0);
    let mut proof = RecoveryProof::new(old.signing_public_key(), new.signing_public_key(), 3, NOW);
    let mut helper_keys = Vec::new();
    for name in ["Bob", "Carol", "Dave"] {
        let helper = Identity::create(name, 0);
        let voucher = RecoveryVoucher::create(
            old.signing_public_key(),
            new.signing_public_key(),
            helper.signing_keypair(),
            None,
            NOW + 1,
        );
        proof.add_voucher(voucher).unwrap();
        helper_keys.push(IdentityKey::from_bytes(*helper.signing_public_key()));
    }
    (proof, old, helper_keys)
}

/// The relay binds a stored proof to its uploader by reading this prefix and
/// nothing else (spec Social Recovery B3, vauchi/private#927, #936).
// @scenario: contact_recovery :: Relay refuses a recovery proof upload the new key did not sign
#[test]
fn a_sealed_proof_starts_with_its_version_then_its_new_key() {
    let (proof, _old, _helpers) = proof_with_three_helpers();

    let sealed = proof.seal_for_relay().unwrap();

    assert_eq!(sealed[0], SEALED_RECOVERY_PROOF_VERSION);
    assert_eq!(&sealed[1..33], proof.new_pk().as_bytes());
    assert!(RecoveryProof::is_sealed(&sealed));
    assert!(!RecoveryProof::is_sealed(&proof.to_bytes().unwrap()));
}

/// SC-17: the stored proof shows the relay neither the old key nor any
/// helper's identity key; only the holder of the old key opens it.
// @scenario: contact_recovery :: Relay stores recovery proof privately
#[test]
fn a_stored_recovery_proof_contains_no_helper_identity_key() {
    let (proof, old, helpers) = proof_with_three_helpers();
    let old_pk = IdentityKey::from_bytes(*old.signing_public_key());

    let sealed = proof.seal_for_relay().unwrap();

    assert!(
        !contains(&sealed, old.signing_public_key()),
        "the old key must not appear in the stored bytes"
    );
    for helper in &helpers {
        assert!(
            !contains(&sealed, helper.as_bytes()),
            "a helper's identity key must not appear in the stored bytes"
        );
    }
    let opened = RecoveryProof::open_sealed(&sealed, &old_pk).unwrap();
    assert_eq!(opened.old_pk(), proof.old_pk());
    assert_eq!(opened.new_pk(), proof.new_pk());
    assert_eq!(opened.voucher_count(), 3);
    assert_eq!(
        opened.vouchers()[1].voucher_pk(),
        proof.vouchers()[1].voucher_pk()
    );

    let stranger = IdentityKey::from_bytes(*Identity::create("Mallory", 0).signing_public_key());
    assert!(matches!(
        RecoveryProof::open_sealed(&sealed, &stranger),
        Err(RecoveryError::DecryptionFailed)
    ));
    let mut tampered = sealed.clone();
    tampered[40] ^= 0x01;
    assert!(matches!(
        RecoveryProof::open_sealed(&tampered, &old_pk),
        Err(RecoveryError::DecryptionFailed)
    ));
    let mut header_swapped = sealed.clone();
    header_swapped[5] ^= 0x01;
    assert!(
        matches!(
            RecoveryProof::open_sealed(&header_swapped, &old_pk),
            Err(RecoveryError::DecryptionFailed)
        ),
        "the header is bound to the body, so a changed new key fails the tag"
    );
}

/// A sealed proof from a schema the reader does not know is refused before
/// any decryption, as the plain proof's version gate does.
// @internal
#[test]
fn open_sealed_refuses_a_short_blob_and_an_unknown_version() {
    let (proof, old, _helpers) = proof_with_three_helpers();
    let old_pk = IdentityKey::from_bytes(*old.signing_public_key());
    let sealed = proof.seal_for_relay().unwrap();

    assert!(matches!(
        RecoveryProof::open_sealed(&sealed[..33], &old_pk),
        Err(RecoveryError::InvalidFormat)
    ));
    let mut future = sealed.clone();
    future[0] = SEALED_RECOVERY_PROOF_VERSION + 1;
    assert!(matches!(
        RecoveryProof::open_sealed(&future, &old_pk),
        Err(RecoveryError::InvalidProof(_))
    ));
}
