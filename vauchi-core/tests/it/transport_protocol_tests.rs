// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for transport-agnostic ExchangeProtocol (X25519 + XChaCha20-Poly1305).

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::caps::TransportCaps;
use vauchi_core::exchange::transport::protocol::ExchangeProtocol;

/// Offer must be exactly 90 bytes:
/// identity_pub(32) + ephemeral_pub(32) + nonce(16) + timestamp(8) + caps(2)
// @internal
#[test]
fn create_offer_returns_90_bytes() {
    let protocol = ExchangeProtocol::new_random();
    let offer = protocol
        .create_offer(0u64)
        .expect("create_offer should succeed");
    assert_eq!(offer.len(), 90, "offer must be exactly 90 bytes");
}

/// Two parties performing mutual key agreement must derive identical shared keys.
// @internal
#[test]
fn mutual_key_agreement_produces_identical_shared_keys() {
    let alice = ExchangeProtocol::new_random();
    let bob = ExchangeProtocol::new_random();

    let alice_offer = alice.create_offer(0u64).expect("alice offer");
    let bob_offer = bob.create_offer(0u64).expect("bob offer");

    let alice_shared = alice.process_offer(&bob_offer).expect("alice process");
    let bob_shared = bob.process_offer(&alice_offer).expect("bob process");

    assert_eq!(
        alice_shared.as_bytes(),
        bob_shared.as_bytes(),
        "Alice and Bob must derive the same shared key"
    );
    // Sanity: key is not all zeros
    assert_ne!(alice_shared.as_bytes(), &[0u8; 32]);
}

/// Encrypt-then-decrypt roundtrip must recover the original plaintext.
// @internal
#[test]
fn encrypt_then_decrypt_roundtrip() {
    let alice = ExchangeProtocol::new_random();
    let bob = ExchangeProtocol::new_random();

    let alice_offer = alice.create_offer(0u64).expect("alice offer");
    let bob_offer = bob.create_offer(0u64).expect("bob offer");

    let shared = alice.process_offer(&bob_offer).expect("shared key");
    let _ = bob.process_offer(&alice_offer).expect("bob shared key");

    let plaintext = b"hello vauchi contact card data";
    let encrypted =
        ExchangeProtocol::encrypt_card(plaintext, &shared).expect("encryption should succeed");

    // Encrypted output must differ from plaintext and include 24-byte nonce prefix
    assert_ne!(&encrypted[..plaintext.len()], &plaintext[..]);
    assert!(
        encrypted.len() >= 24 + plaintext.len(),
        "encrypted must include nonce prefix"
    );

    let decrypted =
        ExchangeProtocol::decrypt_card(&encrypted, &shared).expect("decryption should succeed");
    assert_eq!(decrypted, plaintext, "roundtrip must recover original data");
}

/// Decryption with a wrong key must fail.
// @internal
#[test]
fn wrong_key_cannot_decrypt() {
    let alice = ExchangeProtocol::new_random();
    let bob = ExchangeProtocol::new_random();
    let carol = ExchangeProtocol::new_random();

    let alice_offer = alice.create_offer(0u64).expect("alice offer");
    let bob_offer = bob.create_offer(0u64).expect("bob offer");
    let carol_offer = carol.create_offer(0u64).expect("carol offer");

    let alice_bob_shared = alice.process_offer(&bob_offer).expect("alice-bob key");
    let _bob_alice_shared = bob.process_offer(&alice_offer).expect("bob-alice key");
    let alice_carol_shared = alice.process_offer(&carol_offer).expect("alice-carol key");

    // Keys must differ
    assert_ne!(
        alice_bob_shared.as_bytes(),
        alice_carol_shared.as_bytes(),
        "different peers must yield different shared keys"
    );

    let plaintext = b"secret card data";
    let encrypted = ExchangeProtocol::encrypt_card(plaintext, &alice_bob_shared)
        .expect("encryption should succeed");

    let result = ExchangeProtocol::decrypt_card(&encrypted, &alice_carol_shared);
    assert!(
        result.is_err(),
        "decryption with wrong key must fail, got: {:?}",
        result
    );
}

/// Tampering with the offer (flipping a byte in ephemeral pub) must produce
/// a different shared secret than the untampered offer.
// @internal
#[test]
fn tampered_offer_produces_different_shared_secret() {
    let alice = ExchangeProtocol::new_random();
    let bob = ExchangeProtocol::new_random();

    let bob_offer = bob.create_offer(0u64).expect("bob offer");

    let shared_original = alice.process_offer(&bob_offer).expect("original key");

    // Tamper with the ephemeral public key (bytes 32..64)
    let mut tampered_offer = bob_offer.clone();
    tampered_offer[32] ^= 0xFF;

    let shared_tampered = alice.process_offer(&tampered_offer).expect("tampered key");

    assert_ne!(
        shared_original.as_bytes(),
        shared_tampered.as_bytes(),
        "tampered offer must produce a different shared secret"
    );
}

/// An offer shorter than 90 bytes must be rejected.
// @internal
#[test]
fn short_offer_rejected() {
    let alice = ExchangeProtocol::new_random();

    let short_offer = vec![0u8; 89];
    let result = alice.process_offer(&short_offer);
    assert!(
        result.is_err(),
        "offer shorter than 90 bytes must be rejected"
    );

    // Empty offer
    let result = alice.process_offer(&[]);
    assert!(result.is_err(), "empty offer must be rejected");
}

/// Capabilities bitfield must appear at bytes 88..90 of the offer.
// @internal
#[test]
fn capabilities_embedded_in_offer_bytes_88_90() {
    let caps = TransportCaps::BLE | TransportCaps::WIFI_AWARE;
    let protocol = ExchangeProtocol::new_random().with_capabilities(caps);

    let offer = protocol.create_offer(0u64).expect("create_offer");

    let caps_bytes = [offer[88], offer[89]];
    let decoded_caps = TransportCaps::from_bytes(caps_bytes);
    assert_eq!(
        decoded_caps, caps,
        "capabilities at bytes 88..90 must match"
    );
}
