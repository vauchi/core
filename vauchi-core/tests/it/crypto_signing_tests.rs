// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto::signing
//! Extracted from signing.rs

use vauchi_core::crypto::*;

// @scenario: identity_management :: Create new identity on first launch
// @scenario: security :: Correct algorithms used
#[test]
fn test_keypair_generation() {
    let kp = SigningKeyPair::generate();
    assert_eq!(kp.public_key().as_bytes().len(), 32);
}

// @scenario: security :: Contact card signatures verified
#[test]
fn test_sign_verify() {
    let kp = SigningKeyPair::generate();
    let msg = b"test message";
    let sig = kp.sign(msg);
    assert!(kp.public_key().verify(msg, &sig));
}

// @scenario: security :: Weak Ed25519 identity keys are rejected
#[test]
fn test_crypto_hardening_weak_identity_key_rejected() {
    use ed25519_dalek::Verifier;

    let mut weak_key = [0u8; 32];
    weak_key[0] = 1;

    let mut forged_signature = [0u8; 64];
    forged_signature[0] = 1;

    let public_key = PublicKey::from_bytes(weak_key);
    let signature = Signature::from_bytes(forged_signature);
    let message = b"attacker-selected message";

    let dalek_key = ed25519_dalek::VerifyingKey::from_bytes(&weak_key).unwrap();
    let dalek_signature = ed25519_dalek::Signature::from_bytes(&forged_signature);
    assert!(dalek_key.is_weak(), "fixture must be a weak public key");
    assert!(
        dalek_key.verify(message, &dalek_signature).is_ok(),
        "fixture must demonstrate ordinary verification accepting the forgery"
    );
    assert!(
        dalek_key.verify_strict(message, &dalek_signature).is_err(),
        "strict verification must reject the weak-key forgery"
    );

    assert!(
        !public_key.verify(message, &signature),
        "weak identity key must not authenticate a forged signature"
    );
    assert!(
        !vauchi_core::crypto::signing::verify_signature(&weak_key, message, &signature),
        "standalone verification must reject the same weak-key forgery"
    );
}

// RFC 8032 section 7.1, test vector 1 (empty message).
// @scenario: security :: Ed25519 implementation matches the published standard
#[test]
fn test_crypto_hardening_ed25519_rfc8032_vector_1() {
    let seed = hex::decode("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
        .unwrap()
        .try_into()
        .unwrap();
    let expected_public =
        hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").unwrap();
    let expected_signature = hex::decode(concat!(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
        "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    ))
    .unwrap();

    let keypair = SigningKeyPair::from_seed(&seed);
    let signature = keypair.sign(b"");

    assert_eq!(keypair.public_key().as_bytes(), expected_public.as_slice());
    assert_eq!(signature.as_bytes(), expected_signature.as_slice());
    assert!(keypair.public_key().verify(b"", &signature));
}
