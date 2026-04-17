// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for sealed-box encryption (ephemeral X25519 + XChaCha20-Poly1305).

use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};

use vauchi_core::recovery::sealed_box;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn generate_keypair() -> (StaticSecret, PublicKey) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// @scenario: contact_recovery :: Seal and unseal a payload with the correct recipient key
#[test]
fn test_seal_unseal_roundtrip() {
    let (secret, public) = generate_keypair();
    let plaintext = b"guardian token payload for social recovery";

    let sealed = sealed_box::seal(plaintext, &public);
    let opened = sealed_box::open(&sealed, &secret).expect("open must succeed");

    assert_eq!(
        opened, plaintext,
        "opened plaintext must equal original plaintext"
    );
}

// @scenario: contact_recovery :: Wrong recipient key cannot decrypt sealed payload
#[test]
fn test_wrong_key_cannot_decrypt() {
    let (_secret_a, public_a) = generate_keypair();
    let (secret_b, _public_b) = generate_keypair();

    let plaintext = b"secret guardian token";
    let sealed = sealed_box::seal(plaintext, &public_a);

    let result = sealed_box::open(&sealed, &secret_b);
    assert!(
        result.is_err(),
        "opening with a different secret key must fail"
    );
}

// @scenario: contact_recovery :: Tampered sealed-box ciphertext fails authentication
#[test]
fn test_tampered_ciphertext_fails() {
    let (secret, public) = generate_keypair();
    let plaintext = b"guardian token to tamper";

    let mut sealed = sealed_box::seal(plaintext, &public);
    // Flip the last byte (inside the authentication tag)
    let last = sealed.last_mut().expect("sealed must be non-empty");
    *last ^= 0xFF;

    let result = sealed_box::open(&sealed, &secret);
    assert!(
        result.is_err(),
        "opening a tampered ciphertext must fail with authentication error"
    );
}

// @scenario: contact_recovery :: Sealed-box output size is ephemeral-pk plus nonce plus ciphertext plus tag
#[test]
fn test_sealed_box_output_size() {
    let (_secret, public) = generate_keypair();
    let plaintext = vec![0u8; 100];

    let sealed = sealed_box::seal(&plaintext, &public);

    // ephemeral_pk (32) + nonce (24) + ciphertext (100) + tag (16) = 172
    assert_eq!(
        sealed.len(),
        172,
        "sealed output must be exactly 32+24+100+16 = 172 bytes"
    );
}
