// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for RecoveryVoucher v2: guardian tokens, domain separation,
//! serialization backward compatibility, and proof validation.
//!
//! Traces to: features/contact_recovery.feature
//! - @recovery @trust: guardian token in vouchers
//! - @recovery @security: voucher v2 validation

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::{RecoveryClaim, RecoveryError, RecoveryProof, RecoveryVoucher};

// =============================================================================
// =============================================================================

// @scenario: contact_recovery :: Serialize and deserialize v2 recovery voucher with guardian token
#[test]
fn test_voucher_v2_with_guardian_token_roundtrip() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    // designator = old identity; guardian = voucher signer
    let token = GuardianToken::create(&old_keypair, voucher_keypair.public_key(), 0);

    let voucher =
        RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token.clone()), 0);
    let bytes = voucher.to_bytes();
    let restored = RecoveryVoucher::from_bytes(&bytes).expect("v2 voucher must deserialize");

    assert!(
        restored.verify(),
        "restored v2 voucher signature must verify"
    );
    assert!(
        restored.guardian_token().is_some(),
        "restored v2 voucher must preserve the guardian token"
    );
    let restored_token = restored.guardian_token().unwrap();
    assert!(
        restored_token.verify(),
        "restored guardian token must verify"
    );
    assert_eq!(
        restored_token.designator_pk(),
        token.designator_pk(),
        "designator_pk must survive roundtrip"
    );
    assert_eq!(
        restored_token.guardian_pk(),
        token.guardian_pk(),
        "guardian_pk must survive roundtrip"
    );
}

// @scenario: contact_recovery :: Serialize and deserialize v1 recovery voucher without guardian token
#[test]
fn test_voucher_v1_without_token_roundtrip() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, None, 0);
    let bytes = voucher.to_bytes();
    let restored = RecoveryVoucher::from_bytes(&bytes).expect("v1 voucher must deserialize");

    assert!(
        restored.verify(),
        "restored v1 voucher signature must verify"
    );
    assert!(
        restored.guardian_token().is_none(),
        "v1 voucher must not have a guardian token after roundtrip"
    );
}

// @scenario: contact_recovery :: Deserializing v2 voucher preserves guardian token fields exactly
#[test]
fn test_voucher_v2_from_bytes_preserves_token() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    let token = GuardianToken::create(&old_keypair, voucher_keypair.public_key(), 0);
    let original_designator_pk = *token.designator_pk();
    let original_guardian_pk = *token.guardian_pk();

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token), 0);
    let bytes = voucher.to_bytes();
    let restored = RecoveryVoucher::from_bytes(&bytes).expect("v2 voucher must deserialize");

    assert!(
        restored.guardian_token().is_some(),
        "from_bytes must preserve the guardian token"
    );
    let rt = restored.guardian_token().unwrap();
    assert_eq!(
        rt.designator_pk(),
        &original_designator_pk,
        "designator_pk must be preserved exactly"
    );
    assert_eq!(
        rt.guardian_pk(),
        &original_guardian_pk,
        "guardian_pk must be preserved exactly"
    );
}

// =============================================================================
// =============================================================================

// @scenario: contact_recovery :: A guardian token travels under the voucher signature
#[test]
fn test_voucher_v2_version_byte() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    // v2: with guardian token
    let token = GuardianToken::create(&old_keypair, voucher_keypair.public_key(), 0);
    let v2 = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token), 0);
    let v2_bytes = v2.to_bytes();
    assert_eq!(
        v2_bytes[0], 3,
        "a guardian voucher is format 3: its token is under the signature (private#930)"
    );

    // v1: without guardian token
    let v1 = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, None, 0);
    let v1_bytes = v1.to_bytes();
    assert_eq!(
        v1_bytes[0], 1,
        "first byte of v1 voucher (without token) must be 1"
    );
}

// =============================================================================
// create_from_claim with token
// =============================================================================

// @scenario: contact_recovery :: Create recovery voucher from claim with guardian token
#[test]
fn test_create_from_claim_with_token() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    let claim = RecoveryClaim::new(&old_pk, &new_pk, 0);
    let token = GuardianToken::create(&old_keypair, voucher_keypair.public_key(), 0);
    let original_guardian_pk = *token.guardian_pk();

    let voucher =
        RecoveryVoucher::create_from_claim(&claim, &voucher_keypair, Some(token), 0).unwrap();

    assert!(
        voucher.verify(),
        "voucher created from claim with token must verify"
    );
    assert!(
        voucher.guardian_token().is_some(),
        "voucher created with token must retain the token"
    );
    assert_eq!(
        voucher.guardian_token().unwrap().guardian_pk(),
        &original_guardian_pk,
        "guardian_pk in token must match voucher signer"
    );
}

// =============================================================================
// RecoveryProof::add_voucher guardian token validation
// =============================================================================

// @scenario: contact_recovery :: Add voucher with valid guardian token to recovery proof
#[test]
fn test_proof_add_voucher_with_valid_token() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    // Token: designator = old identity, guardian = voucher signer
    let token = GuardianToken::create(&old_keypair, voucher_keypair.public_key(), 0);

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token), 0);

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 1, 0);
    let result = proof.add_voucher(voucher);

    assert!(
        result.is_ok(),
        "add_voucher with valid guardian token must succeed, got: {:?}",
        result
    );
    assert_eq!(
        proof.voucher_count(),
        1,
        "proof must contain exactly 1 voucher after add"
    );
}

// @scenario: contact_recovery :: Reject recovery proof voucher with invalid guardian token signature
#[test]
fn test_proof_rejects_voucher_with_invalid_token_signature() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();
    let impostor_keypair = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    let mut token = GuardianToken::create(&old_keypair, voucher_keypair.public_key(), 0);
    token.set_guardian_pk_for_testing(impostor_keypair.public_key().as_bytes());

    assert!(
        !token.verify(),
        "tampered token must fail verification before being used in test"
    );

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token), 0);

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 1, 0);
    let result = proof.add_voucher(voucher);

    assert!(
        matches!(result, Err(RecoveryError::InvalidSignature)),
        "add_voucher with tampered token must return InvalidSignature, got: {:?}",
        result
    );
}

// @scenario: contact_recovery :: Reject recovery proof voucher whose token designator does not match proof identity
#[test]
fn test_proof_rejects_voucher_with_wrong_designator_pk() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();
    let wrong_designator = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    // Token signed by wrong_designator (not the recovering identity old_keypair)
    let token = GuardianToken::create(&wrong_designator, voucher_keypair.public_key(), 0);

    // Token itself is valid (self-consistent), but designator doesn't match proof.old_pk
    assert!(
        token.verify(),
        "self-consistent token must verify on its own"
    );

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token), 0);

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 1, 0);
    let result = proof.add_voucher(voucher);

    assert!(
        matches!(result, Err(RecoveryError::MismatchedKeys)),
        "add_voucher with wrong designator must return MismatchedKeys, got: {:?}",
        result
    );
}

// @scenario: contact_recovery :: Reject recovery proof voucher where token guardian key does not match voucher signer
#[test]
fn test_proof_rejects_voucher_with_wrong_guardian_pk() {
    let old_keypair = SigningKeyPair::generate();
    let new_keypair = SigningKeyPair::generate();
    let voucher_keypair = SigningKeyPair::generate();
    let different_guardian = SigningKeyPair::generate();

    let old_pk = *old_keypair.public_key().as_bytes();
    let new_pk = *new_keypair.public_key().as_bytes();

    // Token names different_guardian as guardian, but the voucher is signed by voucher_keypair
    let token = GuardianToken::create(&old_keypair, different_guardian.public_key(), 0);

    // Token is self-consistent but guardian_pk != voucher_pk
    assert!(
        token.verify(),
        "self-consistent token must verify on its own"
    );

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, Some(token), 0);

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 1, 0);
    let result = proof.add_voucher(voucher);

    assert!(
        matches!(result, Err(RecoveryError::MismatchedKeys)),
        "add_voucher where token.guardian_pk != voucher_pk must return MismatchedKeys, got: {:?}",
        result
    );
}

// @scenario: contact_recovery :: Accept v1 backward-compatible voucher without guardian token
#[test]
fn test_proof_accepts_voucher_without_token() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let voucher_keypair = SigningKeyPair::generate();

    // v1 voucher with no token — backward compat: no token required
    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &voucher_keypair, None, 0);

    let mut proof = RecoveryProof::new(&old_pk, &new_pk, 1, 0);
    let result = proof.add_voucher(voucher);

    assert!(
        result.is_ok(),
        "add_voucher with no guardian token (v1 compat) must succeed, got: {:?}",
        result
    );
    assert_eq!(
        proof.voucher_count(),
        1,
        "proof must contain exactly 1 voucher after successful add"
    );
}

// =============================================================================
// The recovery window: a proof's vouchers span at most 96 h (#919)
// =============================================================================

fn proof_with_vouchers_at(timestamps: &[u64]) -> RecoveryProof {
    let old = SigningKeyPair::generate();
    let new = SigningKeyPair::generate();
    let old_pk = *old.public_key().as_bytes();
    let new_pk = *new.public_key().as_bytes();
    let mut proof = RecoveryProof::new(&old_pk, &new_pk, timestamps.len() as u32, 0);
    for &timestamp in timestamps {
        let helper = SigningKeyPair::generate();
        let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &helper, None, timestamp);
        proof.add_voucher(voucher).unwrap();
    }
    proof
}

// @internal
#[test]
fn a_proof_whose_vouchers_span_exactly_the_window_validates() {
    let start = 1_800_000_000;
    let proof = proof_with_vouchers_at(&[
        start,
        start + 3600,
        start + RecoveryProof::VOUCHER_WINDOW_SECS,
    ]);

    assert!(matches!(proof.validate(), Ok(())));
}

// @internal
#[test]
fn a_proof_whose_vouchers_span_more_than_the_window_is_refused() {
    let start = 1_800_000_000;
    let proof = proof_with_vouchers_at(&[
        start + RecoveryProof::VOUCHER_WINDOW_SECS + 1,
        start + 3600,
        start,
    ]);

    assert!(matches!(
        proof.validate(),
        Err(RecoveryError::VouchersOutsideWindow)
    ));
}

// @internal
#[test]
fn the_voucher_window_is_96_hours() {
    assert_eq!(RecoveryProof::VOUCHER_WINDOW_SECS, 96 * 60 * 60);
}

// =============================================================================
// The token is under the voucher signature (private#930)
// =============================================================================

struct GuardianVouching {
    old: SigningKeyPair,
    new: SigningKeyPair,
    helper: SigningKeyPair,
}

impl GuardianVouching {
    fn new() -> Self {
        Self {
            old: SigningKeyPair::generate(),
            new: SigningKeyPair::generate(),
            helper: SigningKeyPair::generate(),
        }
    }

    fn token_at(&self, created_at: u64) -> GuardianToken {
        GuardianToken::create(&self.old, self.helper.public_key(), created_at)
    }

    fn voucher_with(&self, token: GuardianToken) -> RecoveryVoucher {
        RecoveryVoucher::create(
            self.old.public_key().as_bytes(),
            self.new.public_key().as_bytes(),
            &self.helper,
            Some(token),
            1_700_000_000,
        )
    }

    fn proof(&self) -> RecoveryProof {
        RecoveryProof::new(
            self.old.public_key().as_bytes(),
            self.new.public_key().as_bytes(),
            1,
            1_700_000_000,
        )
    }
}

/// The voucher header: format, old_pk, new_pk, helper_pk, timestamp,
/// signature, before any token.
const VOUCHER_HEADER_LEN: usize = 1 + 32 + 32 + 32 + 8 + 64;

// @scenario: contact_recovery :: A guardian token travels under the voucher signature
#[test]
fn an_unsigned_guardian_token_is_refused() {
    let world = GuardianVouching::new();
    let signed = world.voucher_with(world.token_at(1_700_000_000)).to_bytes();
    let other_token = world.token_at(1_700_000_001).to_bytes();
    let mut swapped_token = signed[..VOUCHER_HEADER_LEN + 4].to_vec();
    swapped_token.extend_from_slice(&other_token);
    let mut stripped_to_contact = signed[..VOUCHER_HEADER_LEN].to_vec();
    stripped_to_contact[0] = 1;
    let mut old_format_two = signed.clone();
    old_format_two[0] = 2;
    let mut guardian_format_without_token = signed[..VOUCHER_HEADER_LEN].to_vec();
    guardian_format_without_token[0] = 3;

    assert!(RecoveryVoucher::from_bytes(&signed).unwrap().verify());
    for (name, bytes) in [
        ("another token under the same signature", swapped_token),
        (
            "the token stripped and format set to contact",
            stripped_to_contact,
        ),
    ] {
        let voucher = RecoveryVoucher::from_bytes(&bytes).unwrap();
        assert!(
            !voucher.verify(),
            "{name}: the signature no longer covers the bytes"
        );
        assert!(matches!(
            world.proof().add_voucher(voucher),
            Err(RecoveryError::InvalidSignature)
        ));
    }
    for (name, bytes) in [
        ("format 2, whose token was not signed", old_format_two),
        ("format 3 without a token", guardian_format_without_token),
    ] {
        assert!(
            matches!(
                RecoveryVoucher::from_bytes(&bytes),
                Err(RecoveryError::InvalidFormat)
            ),
            "{name} must not parse"
        );
    }
}

// @scenario: contact_recovery :: A guardian token travels under the voucher signature
#[test]
fn a_proof_carrying_a_voucher_with_a_forged_token_fails_validation() {
    let world = GuardianVouching::new();
    let mallory = SigningKeyPair::generate();
    let forged_token = GuardianToken::create_with_claimed_pk(
        &mallory,
        world.old.public_key(),
        world.helper.public_key(),
        1_700_000_000,
    );
    let voucher = world.voucher_with(forged_token);
    assert!(
        voucher.verify(),
        "the helper did sign this voucher, forged token and all"
    );
    let wire = WireProof {
        version: 2,
        old_pk: *world.old.public_key().as_bytes(),
        new_pk: *world.new.public_key().as_bytes(),
        threshold: 1,
        vouchers: vec![voucher.clone()],
        created_at: 1_700_000_000,
        expires_at: 1_700_000_000 + 192 * 3600,
    };
    let proof = RecoveryProof::from_bytes(&postcard::to_allocvec(&wire).unwrap()).unwrap();

    assert!(matches!(
        world.proof().add_voucher(voucher),
        Err(RecoveryError::InvalidSignature)
    ));
    assert!(
        matches!(proof.validate(), Err(RecoveryError::InvalidSignature)),
        "validate re-verifies the token, got {:?}",
        proof.validate()
    );
}

/// The proof's postcard shape, to carry a voucher `add_voucher` refuses.
#[derive(serde::Serialize)]
struct WireProof {
    version: u8,
    old_pk: [u8; 32],
    new_pk: [u8; 32],
    threshold: u32,
    vouchers: Vec<RecoveryVoucher>,
    created_at: u64,
    expires_at: u64,
}
