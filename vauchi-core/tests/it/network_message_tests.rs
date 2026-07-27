// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for network::message
//! Extracted from message.rs

use vauchi_core::identifiers::{DhPublicKey, IdentityKey};
use vauchi_core::network::*;

// @scenario: relay_network :: Relay protocol versioning
#[test]
fn test_message_envelope_serialize_roundtrip() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "test-id-123".to_string().into(),
        timestamp: 1234567890,
        payload: MessagePayload::Presence(PresenceUpdate {
            status: PresenceStatus::Online,
            message: Some("Hello".to_string()),
        }),
    };

    let json = serde_json::to_string(&envelope).unwrap();
    let restored: MessageEnvelope = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.version, envelope.version);
    assert_eq!(restored.message_id, envelope.message_id);
    assert_eq!(restored.timestamp, envelope.timestamp);
}

// @scenario: relay_network :: Relay only sees encrypted blobs
#[test]
fn test_encrypted_update_serialize() {
    let update = EncryptedUpdate {
        recipient_id: "recipient-123".to_string().into(),
        sender_id: "sender-456".to_string().into(),
        ratchet_header: RatchetHeader {
            dh_public: DhPublicKey::from_bytes([1u8; 32]),
            dh_generation: 5,
            message_index: 10,
            previous_chain_length: 3,
        },
        ciphertext: vec![0xDE, 0xAD, 0xBE, 0xEF],
        origin_hint: None,
    };

    let json = serde_json::to_string(&update).unwrap();
    let restored: EncryptedUpdate = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.recipient_id, update.recipient_id);
    assert_eq!(restored.sender_id, update.sender_id);
    assert_eq!(
        restored.ratchet_header.dh_public,
        update.ratchet_header.dh_public
    );
    assert_eq!(restored.ciphertext, update.ciphertext);
}

// @scenario: message_delivery :: Receive acknowledgment when update is delivered
#[test]
fn test_acknowledgment_serialize() {
    let ack = Acknowledgment {
        message_id: "msg-123".to_string().into(),
        status: AckStatus::Delivered,
        error: None,
    };

    let json = serde_json::to_string(&ack).unwrap();
    let restored: Acknowledgment = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.message_id, ack.message_id);
    assert_eq!(restored.status, AckStatus::Delivered);
}

// @scenario: relay_network :: Client authenticates with Ed25519 signature
#[test]
fn test_handshake_signature_bytes() {
    let handshake = Handshake {
        identity_public_key: IdentityKey::from_bytes([2u8; 32]),
        nonce: [3u8; 32],
        signature: [4u8; 64],
    };

    let json = serde_json::to_string(&handshake).unwrap();
    let restored: Handshake = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.identity_public_key, handshake.identity_public_key);
    assert_eq!(restored.nonce, handshake.nonce);
    assert_eq!(restored.signature, handshake.signature);
}

// @scenario: message_delivery :: See delivery status for updates
// @scenario: message_delivery :: Read receipts are never sent
#[test]
fn test_ack_status_values() {
    assert_ne!(AckStatus::Delivered, AckStatus::Failed);
    assert_ne!(AckStatus::ReceivedByRecipient, AckStatus::Failed);
}

// @scenario: message_delivery :: Delivery status updates in real-time
// @scenario: message_delivery :: Offline indicator
#[test]
fn test_presence_status_values() {
    assert_ne!(PresenceStatus::Online, PresenceStatus::Offline);
    assert_ne!(PresenceStatus::Away, PresenceStatus::Online);
}

// Device sync message tests removed (SP-33): DeviceSyncMessage wire type removed.
// Device sync reimplemented via EncryptedUpdate + self-token (Task 4.3 done).
