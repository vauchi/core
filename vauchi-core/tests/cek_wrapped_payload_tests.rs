// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for CEK-wrapped payload encoding (version-tagged inner format).
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Card updates use per-contact content encryption key"
//!   - "Crypto-shredding renders card unreadable without key"

use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::sync::delta::{
    CekWrappedPayload, VersionedPayload, PAYLOAD_VERSION_CEK, PAYLOAD_VERSION_LEGACY,
};

// === Serialization Round-trips ===

#[test]
fn test_cek_wrapped_payload_roundtrip() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"test card delta JSON";
    let cek_ciphertext = cek.encrypt(plaintext).unwrap();

    let payload = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext: cek_ciphertext.clone(),
        signature: [0x42u8; 64],
        nonce: [0xABu8; 32],
    };

    let encoded = payload.encode();
    let decoded = CekWrappedPayload::decode(&encoded).unwrap();

    assert_eq!(decoded.cek, cek.to_bytes());
    assert_eq!(decoded.cek_ciphertext, cek_ciphertext);
    assert_eq!(decoded.signature, [0x42u8; 64]);
    assert_eq!(decoded.nonce, [0xABu8; 32]);
}

#[test]
fn test_cek_wrapped_payload_decrypt_roundtrip() {
    let cek = ContentEncryptionKey::generate();
    let plaintext = b"serialized card delta content";
    let cek_ciphertext = cek.encrypt(plaintext).unwrap();

    let payload = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: [0u8; 64],
        nonce: [0u8; 32],
    };

    // Recipient decrypts using CEK from payload
    let recipient_cek = ContentEncryptionKey::from_bytes(payload.cek);
    let decrypted = recipient_cek.decrypt(&payload.cek_ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

// === Version-Tagged Encoding ===

#[test]
fn test_versioned_payload_legacy_roundtrip() {
    let delta_bytes = b"legacy card delta JSON bytes";

    let encoded = VersionedPayload::encode_legacy(delta_bytes);
    assert_eq!(encoded[0], PAYLOAD_VERSION_LEGACY);

    let decoded = VersionedPayload::decode(&encoded).unwrap();
    match decoded {
        VersionedPayload::Legacy(data) => assert_eq!(data, delta_bytes),
        _ => panic!("Expected Legacy variant"),
    }
}

#[test]
fn test_versioned_payload_cek_roundtrip() {
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(b"card delta").unwrap();

    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext: cek_ciphertext.clone(),
        signature: [0x11u8; 64],
        nonce: [0x22u8; 32],
    };

    let encoded = VersionedPayload::encode_cek(&wrapped);
    assert_eq!(encoded[0], PAYLOAD_VERSION_CEK);

    let decoded = VersionedPayload::decode(&encoded).unwrap();
    match decoded {
        VersionedPayload::CekWrapped(p) => {
            assert_eq!(p.cek, cek.to_bytes());
            assert_eq!(p.cek_ciphertext, cek_ciphertext);
            assert_eq!(p.signature, [0x11u8; 64]);
            assert_eq!(p.nonce, [0x22u8; 32]);
        }
        _ => panic!("Expected CekWrapped variant"),
    }
}

#[test]
fn test_versioned_payload_unknown_version_returns_error() {
    let data = vec![0xFF, 0x01, 0x02, 0x03];
    let result = VersionedPayload::decode(&data);
    assert!(result.is_err());
}

#[test]
fn test_versioned_payload_empty_returns_error() {
    let result = VersionedPayload::decode(&[]);
    assert!(result.is_err());
}

// === Backward Compatibility ===

#[test]
fn test_legacy_payload_preserves_exact_bytes() {
    // Legacy CardDelta bytes must be preserved exactly after version byte stripping
    let original =
        br#"{"version":1,"timestamp":1700000000,"changes":[],"nonce":"AA==","signature":"AA=="}"#;

    let encoded = VersionedPayload::encode_legacy(original);
    let decoded = VersionedPayload::decode(&encoded).unwrap();

    match decoded {
        VersionedPayload::Legacy(data) => assert_eq!(data, original.to_vec()),
        _ => panic!("Expected Legacy variant"),
    }
}

#[test]
fn test_cek_wrapped_with_real_encryption() {
    // Full flow: create delta bytes → CEK encrypt → wrap → version tag → decode → CEK decrypt
    let delta_json = br#"{"version":1,"timestamp":1700000000,"changes":[{"DisplayNameChanged":{"new_name":"Alice Updated"}}]}"#;

    // Sender side
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(delta_json).unwrap();

    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: [0u8; 64],
        nonce: [0u8; 32],
    };

    let versioned_bytes = VersionedPayload::encode_cek(&wrapped);

    // Recipient side
    let decoded = VersionedPayload::decode(&versioned_bytes).unwrap();
    match decoded {
        VersionedPayload::CekWrapped(p) => {
            let recipient_cek = ContentEncryptionKey::from_bytes(p.cek);
            let decrypted = recipient_cek.decrypt(&p.cek_ciphertext).unwrap();
            assert_eq!(decrypted, delta_json);
        }
        _ => panic!("Expected CekWrapped variant"),
    }
}

#[test]
fn test_cek_rotation_in_wrapped_payload() {
    let delta = b"card delta data";

    // First update: CEK v1
    let cek_v1 = ContentEncryptionKey::generate();
    let ct_v1 = cek_v1.encrypt(delta).unwrap();
    let wrapped_v1 = CekWrappedPayload {
        cek: cek_v1.to_bytes(),
        cek_ciphertext: ct_v1,
        signature: [0u8; 64],
        nonce: [0u8; 32],
    };

    // Second update: CEK v2 (rotated)
    let cek_v2 = ContentEncryptionKey::generate();
    let ct_v2 = cek_v2.encrypt(delta).unwrap();
    let wrapped_v2 = CekWrappedPayload {
        cek: cek_v2.to_bytes(),
        cek_ciphertext: ct_v2,
        signature: [0u8; 64],
        nonce: [0u8; 32],
    };

    // CEK v1 and v2 are different keys
    assert_ne!(wrapped_v1.cek, wrapped_v2.cek);

    // v1 CEK cannot decrypt v2 ciphertext
    let old_cek = ContentEncryptionKey::from_bytes(wrapped_v1.cek);
    assert!(old_cek.decrypt(&wrapped_v2.cek_ciphertext).is_err());

    // v2 CEK correctly decrypts v2 ciphertext
    let new_cek = ContentEncryptionKey::from_bytes(wrapped_v2.cek);
    let decrypted = new_cek.decrypt(&wrapped_v2.cek_ciphertext).unwrap();
    assert_eq!(decrypted, delta);
}

// === Version Byte Constants ===

#[test]
fn test_version_constants() {
    assert_eq!(PAYLOAD_VERSION_LEGACY, 0x01);
    assert_eq!(PAYLOAD_VERSION_CEK, 0x02);
}
