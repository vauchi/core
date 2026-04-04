// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::identity::Identity;
use vauchi_core::sync::delta::{ReciprocityConfirmPayload, VersionedPayload};

// @internal
#[test]
fn reciprocity_confirm_encode_decode_roundtrip() {
    let identity = Identity::create("Alice");
    let token = [0xAB; 32];
    let recipient_pk = [0xCD; 32];

    let payload = ReciprocityConfirmPayload::new(token, &identity, &recipient_pk);

    // Inner encode (no version prefix)
    let inner = payload.encode();
    assert_eq!(inner.len(), 96, "inner payload: 32 + 64 = 96 bytes");

    // Full wire format (with version prefix)
    let encoded = VersionedPayload::encode_reciprocity(&payload);
    assert_eq!(encoded[0], 0x03, "version byte must be 0x03");
    assert_eq!(encoded.len(), 97, "wire format: 1 + 32 + 64 = 97 bytes");

    let decoded = VersionedPayload::decode(&encoded).unwrap();
    match decoded {
        VersionedPayload::ReciprocityConfirm(confirm) => {
            assert_eq!(confirm.token(), &token);
            assert!(
                confirm.verify(identity.signing_public_key(), &recipient_pk),
                "signature must verify with correct sender and recipient keys"
            );
        }
        _ => panic!("expected ReciprocityConfirm variant"),
    }
}

// @internal
#[test]
fn reciprocity_confirm_rejects_wrong_signature() {
    let identity_a = Identity::create("Alice");
    let identity_b = Identity::create("Bob");
    let token = [0xAB; 32];
    let recipient_pk = [0xCD; 32];

    let payload = ReciprocityConfirmPayload::new(token, &identity_a, &recipient_pk);
    let encoded = VersionedPayload::encode_reciprocity(&payload);

    let decoded = VersionedPayload::decode(&encoded).unwrap();
    match decoded {
        VersionedPayload::ReciprocityConfirm(confirm) => {
            assert!(
                !confirm.verify(identity_b.signing_public_key(), &recipient_pk),
                "signature must NOT verify with wrong sender key"
            );
        }
        _ => panic!("expected ReciprocityConfirm variant"),
    }
}

// @internal
#[test]
fn reciprocity_confirm_rejects_wrong_recipient() {
    let identity = Identity::create("Alice");
    let token = [0xAB; 32];
    let recipient_pk = [0xCD; 32];
    let wrong_recipient = [0xEF; 32];

    let payload = ReciprocityConfirmPayload::new(token, &identity, &recipient_pk);
    let encoded = VersionedPayload::encode_reciprocity(&payload);

    let decoded = VersionedPayload::decode(&encoded).unwrap();
    match decoded {
        VersionedPayload::ReciprocityConfirm(confirm) => {
            assert!(
                !confirm.verify(identity.signing_public_key(), &wrong_recipient),
                "signature must NOT verify with wrong recipient key"
            );
        }
        _ => panic!("expected ReciprocityConfirm variant"),
    }
}

// @internal
#[test]
fn unknown_version_byte_returns_error() {
    let data = [0xFF, 0x00, 0x01];
    let result = VersionedPayload::decode(&data);
    assert!(result.is_err(), "unknown version byte should error");
}
