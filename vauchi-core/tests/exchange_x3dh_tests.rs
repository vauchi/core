// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange::x3dh
//! Extracted from x3dh.rs

use vauchi_core::exchange::*;

#[test]
fn test_keypair_generation() {
    let kp = X3DHKeyPair::generate();
    assert_eq!(kp.public_key().len(), 32);
}

#[test]
fn test_keypair_from_bytes_roundtrip() {
    let kp1 = X3DHKeyPair::generate();
    let bytes = kp1.secret_bytes();
    let kp2 = X3DHKeyPair::from_bytes(bytes);

    assert_eq!(kp1.public_key(), kp2.public_key());
}

// === HKDF Derivation Tests (Item 68) ===
// Verifies that X3DH applies HKDF to the raw DH output rather than
// using it directly. Raw DH output has non-uniform distribution;
// HKDF produces a proper pseudorandom key.

#[test]
fn test_x3dh_respond_key_differs_from_raw_dh() {
    // Generate keypairs
    let responder_keys = X3DHKeyPair::generate();
    let initiator_keys = X3DHKeyPair::generate();

    // Initiate exchange to get ephemeral public key
    let (initiator_key, ephemeral_public) =
        X3DH::initiate(&initiator_keys, responder_keys.public_key()).unwrap();

    // Respond to get the derived key
    let responder_key = X3DH::respond(
        &responder_keys,
        initiator_keys.public_key(),
        &ephemeral_public,
    )
    .unwrap();

    // Compute the raw DH output (what respond would get without HKDF)
    let raw_dh = responder_keys.diffie_hellman(&ephemeral_public);

    // The derived key MUST NOT equal the raw DH output — HKDF transforms it
    assert_ne!(
        responder_key.as_bytes(),
        &raw_dh,
        "X3DH respond key must be HKDF-derived, not raw DH output"
    );

    // Both sides must still agree on the same key
    assert_eq!(
        initiator_key.as_bytes(),
        responder_key.as_bytes(),
        "Initiator and responder must derive the same key"
    );
}

// === Identity Binding Tests (Item #29 — Full X3DH) ===
// Verifies that the shared secret is cryptographically tied to both
// parties' long-term X25519 keys via DH1 (our_static × their_static).

/// Wrong identity key must produce a different shared secret.
/// With full X3DH, DH1 binds the secret to both parties' identity keys.
#[test]
fn test_x3dh_identity_binding_wrong_key_fails() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();
    let carol_keys = X3DHKeyPair::generate();

    // Alice initiates with bob's public key
    let (alice_secret, ephemeral) = X3DH::initiate(&alice_keys, bob_keys.public_key()).unwrap();

    // Bob responds with CORRECT identity (alice's key) → should match
    let bob_secret_correct = X3DH::respond(&bob_keys, alice_keys.public_key(), &ephemeral).unwrap();
    assert_eq!(
        alice_secret.as_bytes(),
        bob_secret_correct.as_bytes(),
        "Correct identity key must produce matching secret"
    );

    // Bob responds with WRONG identity (carol's key) → must NOT match
    let bob_secret_wrong = X3DH::respond(&bob_keys, carol_keys.public_key(), &ephemeral).unwrap();
    assert_ne!(
        alice_secret.as_bytes(),
        bob_secret_wrong.as_bytes(),
        "Wrong identity key must produce different secret"
    );
}

/// Zero identity key must not produce a matching secret.
#[test]
fn test_x3dh_zero_identity_does_not_match() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let (alice_secret, ephemeral) = X3DH::initiate(&alice_keys, bob_keys.public_key()).unwrap();

    // Bob responds with zero identity key
    let bob_secret_zero = X3DH::respond(&bob_keys, &[0u8; 32], &ephemeral).unwrap();
    assert_ne!(
        alice_secret.as_bytes(),
        bob_secret_zero.as_bytes(),
        "Zero identity key must not produce matching secret"
    );
}

/// Full bidirectional agreement: initiate with alice_keys + bob's public,
/// respond with bob_keys + alice's public → same secret.
#[test]
fn test_x3dh_full_bidirectional_agreement() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let (alice_secret, ephemeral) = X3DH::initiate(&alice_keys, bob_keys.public_key()).unwrap();
    let bob_secret = X3DH::respond(&bob_keys, alice_keys.public_key(), &ephemeral).unwrap();

    assert_eq!(
        alice_secret.as_bytes(),
        bob_secret.as_bytes(),
        "Full X3DH bidirectional agreement must produce same secret"
    );
}

// === Ephemeral Key Uniqueness Tests (Tracker #43) ===
// Verifies that each X3DH initiation generates a fresh ephemeral key.
// The `EphemeralSecret` type in x25519_dalek is consumed on use (affine type),
// enforcing single-use at the type level. These tests additionally verify
// that the RNG produces unique ephemeral public keys across calls.

/// Tracker #43: Each X3DH initiation must produce a unique ephemeral public key.
///
/// Two initiations with the same static keys and same peer must produce
/// different ephemeral public keys, proving the ephemeral secret is freshly
/// generated each time (not derived from static keys).
#[test]
fn test_x3dh_ephemeral_key_uniqueness() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let mut ephemeral_pubs: Vec<Vec<u8>> = Vec::with_capacity(20);
    for _ in 0..20 {
        let (_secret, ephemeral_public) =
            X3DH::initiate(&alice_keys, bob_keys.public_key()).unwrap();
        ephemeral_pubs.push(ephemeral_public.to_vec());
    }

    // All ephemeral public keys must be unique
    for i in 0..ephemeral_pubs.len() {
        for j in (i + 1)..ephemeral_pubs.len() {
            assert_ne!(
                ephemeral_pubs[i], ephemeral_pubs[j],
                "Ephemeral keys at indices {} and {} are identical — RNG failure",
                i, j
            );
        }
    }
}

/// Tracker #43: Ephemeral keys produce different shared secrets per session.
///
/// Even with the same identity keys and peer, each X3DH session must derive
/// a unique shared secret (forward secrecy property). Compromising one
/// session's key must not reveal others.
#[test]
fn test_x3dh_forward_secrecy_different_sessions() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let (secret1, eph1) = X3DH::initiate(&alice_keys, bob_keys.public_key()).unwrap();
    let (secret2, eph2) = X3DH::initiate(&alice_keys, bob_keys.public_key()).unwrap();

    // Different ephemeral keys
    assert_ne!(
        eph1.as_slice(),
        eph2.as_slice(),
        "Ephemeral public keys must differ between sessions"
    );

    // Different shared secrets (forward secrecy)
    assert_ne!(
        secret1.as_bytes(),
        secret2.as_bytes(),
        "Shared secrets must differ between sessions (forward secrecy)"
    );

    // But both sessions must still allow agreement with Bob
    let bob_secret1 = X3DH::respond(&bob_keys, alice_keys.public_key(), &eph1).unwrap();
    let bob_secret2 = X3DH::respond(&bob_keys, alice_keys.public_key(), &eph2).unwrap();

    assert_eq!(
        secret1.as_bytes(),
        bob_secret1.as_bytes(),
        "Session 1 must produce agreement"
    );
    assert_eq!(
        secret2.as_bytes(),
        bob_secret2.as_bytes(),
        "Session 2 must produce agreement"
    );
}
