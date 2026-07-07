// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

// @scenario: contact_recovery :: Version byte is 2 for voucher with token and 1 without
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
        v2_bytes[0], 2,
        "first byte of v2 voucher (with token) must be 2"
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
