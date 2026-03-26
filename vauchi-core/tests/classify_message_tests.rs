// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for classify_message API
//! Trace: ADR-021 Tier 1 — classify_message

use vauchi_core::network::MessageType;
use vauchi_core::network::simple_message::*;

#[test]
fn test_classify_message_returns_encrypted_update_for_encrypted_update_payload() {
    let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(SimpleEncryptedUpdate {
        recipient_id: "recipient-1".to_string(),
        sender_id: "sender-1".to_string(),
        ciphertext: vec![0xDE, 0xAD],
    }));
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::EncryptedUpdate);
}

#[test]
fn test_classify_message_returns_acknowledgment_for_ack_payload() {
    let envelope = create_simple_envelope(SimplePayload::Acknowledgment(SimpleAcknowledgment {
        message_id: "msg-123".to_string(),
        status: SimpleAckStatus::Stored,
    }));
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::Acknowledgment);
}

#[test]
fn test_classify_message_returns_handshake_for_handshake_payload() {
    let envelope = create_simple_envelope(SimplePayload::Handshake(SimpleHandshake {
        client_id: "client-1".to_string(),
        device_id: None,
        identity_public_key: None,
        nonce: None,
        signature: None,
        timestamp: None,
    }));
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::Handshake);
}

// DeviceSync classify test removed (SP-33): wire type removed.

#[test]
fn test_classify_message_returns_identity_revoked_for_revoked_payload() {
    let envelope = create_simple_envelope(SimplePayload::IdentityRevoked(SimpleIdentityRevoked {
        sender_id: "sender-1".to_string(),
        recipient_id: "recipient-1".to_string(),
        timestamp: 1700000000,
        signature: vec![0xAB; 64],
    }));
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::IdentityRevoked);
}

#[test]
fn test_classify_message_returns_unknown_for_empty_input() {
    let result = vauchi_core::network::classify_message(&[]);

    assert_eq!(result, MessageType::Unknown);
}

#[test]
fn test_classify_message_returns_unknown_for_garbage_bytes() {
    let result = vauchi_core::network::classify_message(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB]);

    assert_eq!(result, MessageType::Unknown);
}

#[test]
fn test_classify_message_returns_unknown_for_short_frame() {
    // Only 3 bytes - not enough for even the frame header
    let result = vauchi_core::network::classify_message(&[0x00, 0x00, 0x01]);

    assert_eq!(result, MessageType::Unknown);
}

#[test]
fn test_classify_message_returns_unknown_for_truncated_json() {
    // Valid frame header claiming 100 bytes but only 10 bytes of data
    let mut bytes = vec![0x00, 0x00, 0x00, 0x64]; // 100 in big-endian
    bytes.extend_from_slice(b"{\"version\"");
    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::Unknown);
}

// DeviceSyncAck classify test removed (SP-33): wire type removed.

mod proptest_classify {
    use proptest::prelude::*;
    use vauchi_core::network::MessageType;

    proptest! {
        #[test]
        fn classify_message_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            // Must never panic, always returns a valid MessageType
            let result = vauchi_core::network::classify_message(&data);
            // Must always return a valid variant without panicking
            prop_assert!(matches!(
                result,
                MessageType::EncryptedUpdate
                    | MessageType::Acknowledgment
                    | MessageType::Handshake
                    | MessageType::IdentityRevoked
                    | MessageType::ValidationRecord
                    | MessageType::ValidationRevocation
                    | MessageType::Unknown
            ));
        }
    }
}
