// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Property-Based & Adversarial Tests
//!
//! Property-based tests (proptest) for crypto roundtrips, chunking, and
//! tamper detection. Adversarial tests for edge-case payloads and malformed
//! handshake packets.

use proptest::prelude::*;
use vauchi_core::ExchangeError;
use vauchi_core::crypto::encryption::{self, SymmetricKey};
use vauchi_core::crypto::kdf::HKDF;
use vauchi_core::exchange::{
    BLE_HANDSHAKE_INFO, BleCardPayload, BleChunker, BleHandshakeSession, BleReassembler,
    X3DHKeyPair,
};
use vauchi_core::identity::Identity;

// ============================================================
// Helpers
// ============================================================

fn make_test_key() -> SymmetricKey {
    let key_bytes = HKDF::derive_key(None, &[0x42; 32], BLE_HANDSHAKE_INFO);
    SymmetricKey::from_bytes(*key_bytes)
}

fn make_test_identity() -> Identity {
    Identity::create("PropTest", 0)
}

fn make_test_card(identity: &Identity, name: &str) -> BleCardPayload {
    let exchange_keys = X3DHKeyPair::generate();
    BleCardPayload::new(
        *identity.signing_public_key(),
        name.to_string(),
        *exchange_keys.public_key(),
        vec![("email".into(), "test@example.com".into())],
        None,
    )
}

// ============================================================
// Property-Based Tests
// ============================================================

// @scenario: ble_exchange :: Encrypt-decrypt roundtrip preserves arbitrary data
// @scenario: ble_exchange :: Any single byte flip in ciphertext fails decryption
// @scenario: ble_exchange :: Chunking and reassembly preserves data
proptest! {
// @internal
    #[test]
    fn prop_encrypt_decrypt_roundtrip(plaintext in prop::collection::vec(any::<u8>(), 0..10_000)) {
        let key = make_test_key();
        let ad = b"ble-proptest-ad";

        let ciphertext = encryption::encrypt_with_ad(&key, &plaintext, ad)
            .expect("encryption must succeed");
        let decrypted = encryption::decrypt_with_ad(&key, &ciphertext, ad)
            .expect("decryption must succeed");

        prop_assert_eq!(&decrypted, &plaintext, "roundtrip must preserve plaintext");
    }

// @internal
    #[test]
    fn prop_tampered_ciphertext_always_fails(
        plaintext in prop::collection::vec(any::<u8>(), 1..1000),
        flip_pos in 1usize..999,
    ) {
        let key = make_test_key();
        let ad = b"ble-proptest-ad";

        let ciphertext = encryption::encrypt_with_ad(&key, &plaintext, ad)
            .expect("encryption must succeed");

        // Flip one byte at a position within the ciphertext (skip algorithm tag at [0])
        let mut tampered = ciphertext.clone();
        let idx = 1 + (flip_pos % (tampered.len() - 1));
        tampered[idx] ^= 0xFF;

        let result = encryption::decrypt_with_ad(&key, &tampered, ad);
        prop_assert!(
            result.is_err(),
            "tampered ciphertext at byte {} must fail decryption, but got Ok({} bytes)",
            idx,
            result.unwrap().len()
        );
    }

// @internal
    #[test]
    fn prop_chunking_roundtrip(
        mtu in 20usize..512,
        data in prop::collection::vec(any::<u8>(), 1..4_000),
    ) {
        let chunker = BleChunker::new(&data, mtu);
        let total = chunker.total_chunks();

        let mut reassembler = BleReassembler::new(total).unwrap();
        for i in 0..total {
            let chunk = chunker.chunk(i).expect("chunk index must be valid");
            reassembler.insert_chunk(&chunk).expect("insert must succeed");
        }

        prop_assert!(reassembler.is_complete(), "all chunks must be received");
        let assembled = reassembler.assemble().expect("assemble must succeed");
        prop_assert_eq!(&assembled, &data, "chunking roundtrip must preserve data");
    }
}

// ============================================================
// Adversarial Tests
// ============================================================

// @scenario: ble_exchange :: Empty display name roundtrips correctly
// @internal
#[test]
fn test_adversarial_empty_display_name() {
    let identity_key = [1u8; 32];
    let exchange_key = [2u8; 32];

    let payload = BleCardPayload::new(
        identity_key,
        String::new(), // empty name
        exchange_key,
        vec![],
        None,
    );

    let bytes = payload.to_bytes().expect("serialization must succeed");
    let restored = BleCardPayload::from_bytes(&bytes).expect("deserialization must succeed");

    assert_eq!(restored.display_name, "", "empty name must roundtrip");
    assert!(restored.verify_crc16(), "CRC16 must verify for empty name");
}

// @scenario: ble_exchange :: Unicode display name roundtrips correctly
// @internal
#[test]
fn test_adversarial_unicode_display_name() {
    let identity_key = [3u8; 32];
    let exchange_key = [4u8; 32];

    // Emoji + CJK + Arabic + combining characters
    let name = "\u{1F600}\u{4E16}\u{754C}\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645}\u{0301}";

    let payload = BleCardPayload::new(identity_key, name.to_string(), exchange_key, vec![], None);

    let bytes = payload.to_bytes().expect("serialization must succeed");
    let restored = BleCardPayload::from_bytes(&bytes).expect("deserialization must succeed");

    assert_eq!(
        restored.display_name, name,
        "unicode name must roundtrip exactly"
    );
    assert!(
        restored.verify_crc16(),
        "CRC16 must verify for unicode name"
    );
}

// @scenario: ble_exchange :: Null bytes in fields roundtrip correctly
// @internal
#[test]
fn test_adversarial_null_bytes_in_fields() {
    let identity_key = [5u8; 32];
    let exchange_key = [6u8; 32];

    let fields = vec![
        (
            "key\0with\0nulls".to_string(),
            "value\0also\0null".to_string(),
        ),
        ("\0".to_string(), "\0\0\0".to_string()),
    ];

    let payload = BleCardPayload::new(
        identity_key,
        "NullTest".to_string(),
        exchange_key,
        fields.clone(),
        None,
    );

    let bytes = payload.to_bytes().expect("serialization must succeed");
    let restored = BleCardPayload::from_bytes(&bytes).expect("deserialization must succeed");

    assert_eq!(
        restored.fields, fields,
        "fields with null bytes must roundtrip"
    );
    assert!(
        restored.verify_crc16(),
        "CRC16 must verify for null-byte fields"
    );
}

// @scenario: ble_exchange :: Maximum size avatar roundtrips correctly
// @internal
#[test]
fn test_adversarial_max_size_avatar() {
    let identity_key = [7u8; 32];
    let exchange_key = [8u8; 32];

    // 16KB avatar (maximum for BLE exchange)
    let avatar = vec![0xAB; 16 * 1024];

    let payload = BleCardPayload::new(
        identity_key,
        "AvatarTest".to_string(),
        exchange_key,
        vec![],
        Some(avatar.clone()),
    );

    let bytes = payload.to_bytes().expect("serialization must succeed");
    let restored = BleCardPayload::from_bytes(&bytes).expect("deserialization must succeed");

    assert_eq!(
        restored.avatar.as_deref(),
        Some(avatar.as_slice()),
        "16KB avatar must roundtrip"
    );
    assert!(
        restored.verify_crc16(),
        "CRC16 must verify for max-size avatar"
    );
}

// @scenario: ble_exchange :: Truncated handshake packet is rejected
// @internal
#[test]
fn test_adversarial_truncated_handshake_packet() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Victim");

    // A valid KeyOffer is 89 bytes; feed only 50 bytes
    let truncated = vec![0u8; 50];

    let mut session = BleHandshakeSession::new_responder(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let result = session.process_key_offer(
        &truncated,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    assert!(result.is_err(), "truncated packet must fail");
    match result.unwrap_err() {
        ExchangeError::InvalidBleFormat => {} // expected
        other => panic!("expected InvalidBleFormat for 50-byte packet, got: {other}"),
    }
}
