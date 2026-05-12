// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Failing tests for `GuardianToken` — TDD RED phase.
//!
//! These tests define the contract for a signed guardian designation token.
//! The token proves that a designator has explicitly named a contact as a
//! recovery guardian, using Ed25519 with domain separation.

use vauchi_core::crypto::{PublicKey, Signature, SigningKeyPair};
use vauchi_core::recovery::guardian::GuardianToken;

// @scenario: contact_recovery :: Create a guardian token with correct designator and guardian keys
#[test]
fn test_create_guardian_token() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();

    let token = GuardianToken::create(&designator, guardian.public_key(), 1_700_000_000);

    assert_eq!(token.designator_pk(), designator.public_key().as_bytes());
    assert_eq!(token.guardian_pk(), guardian.public_key().as_bytes());
    // Caller-controlled now is stamped verbatim — was `> 0` previously,
    // when SystemTime::now() guaranteed non-zero.
    assert_eq!(token.created_at(), 1_700_000_000);
}

// @scenario: contact_recovery :: Verify a freshly created guardian token
#[test]
fn test_verify_guardian_token() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();

    let token = GuardianToken::create(&designator, guardian.public_key(), 0);

    assert!(
        token.verify(),
        "a freshly created token must verify successfully"
    );
}

// @scenario: contact_recovery :: Reject guardian token with tampered guardian key
#[test]
fn test_tampered_guardian_token_fails_verification() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();
    let impostor = SigningKeyPair::generate();

    let mut token = GuardianToken::create(&designator, guardian.public_key(), 0);
    token.set_guardian_pk_for_testing(impostor.public_key().as_bytes());

    assert!(
        !token.verify(),
        "a token with a tampered guardian_pk must not verify"
    );
}

// @scenario: contact_recovery :: Serialize and deserialize a guardian token preserving all fields
#[test]
fn test_guardian_token_serialization_roundtrip() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();

    let token = GuardianToken::create(&designator, guardian.public_key(), 0);
    let bytes = token.to_bytes();
    let restored = GuardianToken::from_bytes(&bytes).expect("deserialization must succeed");

    assert_eq!(restored.designator_pk(), token.designator_pk());
    assert_eq!(restored.guardian_pk(), token.guardian_pk());
    assert_eq!(restored.created_at(), token.created_at());
    assert!(restored.verify(), "restored token must still verify");
}

// @scenario: contact_recovery :: Reject forged guardian token signed by wrong designator key
#[test]
fn test_wrong_designator_cannot_forge_token() {
    let real_designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();
    let fake_signer = SigningKeyPair::generate();

    // Attacker uses fake_signer's key to sign but claims real_designator's public key.
    let forged = GuardianToken::create_with_claimed_pk(
        &fake_signer,
        real_designator.public_key(),
        guardian.public_key(),
        0,
    );

    assert!(
        !forged.verify(),
        "a token signed by the wrong key must not verify against the claimed designator_pk"
    );
}

// @scenario: contact_recovery :: Enforce domain separation in guardian token signature
#[test]
fn test_domain_separation() {
    let designator = SigningKeyPair::generate();
    let guardian = SigningKeyPair::generate();

    let token = GuardianToken::create(&designator, guardian.public_key(), 0);

    // Token must verify through the proper API.
    assert!(token.verify());

    // But raw signature over just the concatenated keys (without domain prefix)
    // must NOT verify — confirming the domain tag is part of the signed data.
    let raw_data: Vec<u8> = token
        .designator_pk()
        .iter()
        .chain(token.guardian_pk().iter())
        .copied()
        .collect();

    let pk = PublicKey::from_bytes(*token.designator_pk());
    let sig = Signature::from_bytes(*token.signature_bytes());

    assert!(
        !pk.verify(&raw_data, &sig),
        "raw concatenation without domain prefix must not verify (domain separation enforced)"
    );
}
