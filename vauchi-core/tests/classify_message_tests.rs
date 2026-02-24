// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for classify_message API
//! Trace: ADR-021 Tier 1 — classify_message

use vauchi_core::network::simple_message::*;
use vauchi_core::network::MessageType;

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

#[test]
fn test_classify_message_returns_device_sync_for_device_sync_payload() {
    let envelope = create_device_sync_message(
        "identity-1",
        "target-device",
        "sender-device",
        vec![0x01, 0x02],
        1,
    );
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::DeviceSync);
}

#[test]
fn test_classify_message_returns_account_revoked_for_revoked_payload() {
    let envelope = create_simple_envelope(SimplePayload::AccountRevoked(SimpleAccountRevoked {
        sender_id: "sender-1".to_string(),
        recipient_id: "recipient-1".to_string(),
        timestamp: 1700000000,
        signature: vec![0xAB; 64],
    }));
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::AccountRevoked);
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

#[test]
fn test_classify_message_returns_device_sync_ack_for_sync_ack_payload() {
    let envelope = create_device_sync_ack("msg-456", 42);
    let bytes = encode_simple_message(&envelope).unwrap();

    let result = vauchi_core::network::classify_message(&bytes);

    assert_eq!(result, MessageType::DeviceSyncAck);
}

mod proptest_classify {
    use proptest::prelude::*;
    use vauchi_core::network::MessageType;

    proptest! {
        #[test]
        fn classify_message_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            // Must never panic, always returns a valid MessageType
            let result = vauchi_core::network::classify_message(&data);
            // The result must be one of the enum variants (this is guaranteed by the type system,
            // but we explicitly check it's a valid value)
            let _ = match result {
                MessageType::EncryptedUpdate => "encrypted_update",
                MessageType::Acknowledgment => "acknowledgment",
                MessageType::Handshake => "handshake",
                MessageType::DeviceSync => "device_sync",
                MessageType::DeviceSyncAck => "device_sync_ack",
                MessageType::AccountRevoked => "account_revoked",
                MessageType::ValidationRecord => "validation_record",
                MessageType::ValidationRevocation => "validation_revocation",
                MessageType::Unknown => "unknown",
            };
        }
    }
}
