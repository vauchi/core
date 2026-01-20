//! Tests for network::message
//! Extracted from message.rs

use vauchi_core::network::*;

#[test]
fn test_message_envelope_serialize_roundtrip() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "test-id-123".to_string(),
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

#[test]
fn test_encrypted_update_serialize() {
    let update = EncryptedUpdate {
        recipient_id: "recipient-123".to_string(),
        sender_id: "sender-456".to_string(),
        ratchet_header: RatchetHeader {
            dh_public: [1u8; 32],
            dh_generation: 5,
            message_index: 10,
            previous_chain_length: 3,
        },
        ciphertext: vec![0xDE, 0xAD, 0xBE, 0xEF],
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

#[test]
fn test_acknowledgment_serialize() {
    let ack = Acknowledgment {
        message_id: "msg-123".to_string(),
        status: AckStatus::Delivered,
        error: None,
    };

    let json = serde_json::to_string(&ack).unwrap();
    let restored: Acknowledgment = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.message_id, ack.message_id);
    assert_eq!(restored.status, AckStatus::Delivered);
}

#[test]
fn test_handshake_signature_bytes() {
    let handshake = Handshake {
        identity_public_key: [2u8; 32],
        nonce: [3u8; 32],
        signature: [4u8; 64],
    };

    let json = serde_json::to_string(&handshake).unwrap();
    let restored: Handshake = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.identity_public_key, handshake.identity_public_key);
    assert_eq!(restored.nonce, handshake.nonce);
    assert_eq!(restored.signature, handshake.signature);
}

#[test]
fn test_ack_status_values() {
    assert_ne!(AckStatus::Delivered, AckStatus::Failed);
    assert_ne!(AckStatus::ReceivedByRecipient, AckStatus::Failed);
}

#[test]
fn test_presence_status_values() {
    assert_ne!(PresenceStatus::Online, PresenceStatus::Offline);
    assert_ne!(PresenceStatus::Away, PresenceStatus::Online);
}

// ============================================================
// Phase 2: Device Sync Message Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Test DeviceSyncMessage serialization for inter-device communication
#[test]
fn test_device_sync_message_serialize() {
    let msg = DeviceSyncMessage {
        target_device_id: [0x41u8; 32],
        sender_device_id: [0x42u8; 32],
        ciphertext: vec![0xDE, 0xAD, 0xBE, 0xEF],
        nonce: [0x55u8; 12],
        sync_version: 42,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let restored: DeviceSyncMessage = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.target_device_id, msg.target_device_id);
    assert_eq!(restored.sender_device_id, msg.sender_device_id);
    assert_eq!(restored.ciphertext, msg.ciphertext);
    assert_eq!(restored.nonce, msg.nonce);
    assert_eq!(restored.sync_version, msg.sync_version);
}

/// Test DeviceSyncMessage in MessagePayload envelope
#[test]
fn test_device_sync_message_in_envelope() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "device-sync-123".to_string(),
        timestamp: 1234567890,
        payload: MessagePayload::DeviceSync(DeviceSyncMessage {
            target_device_id: [0x41u8; 32],
            sender_device_id: [0x42u8; 32],
            ciphertext: vec![1, 2, 3, 4],
            nonce: [0x55u8; 12],
            sync_version: 1,
        }),
    };

    let json = serde_json::to_string(&envelope).unwrap();
    let restored: MessageEnvelope = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.message_id, envelope.message_id);
    if let MessagePayload::DeviceSync(msg) = restored.payload {
        assert_eq!(msg.target_device_id, [0x41u8; 32]);
        assert_eq!(msg.sync_version, 1);
    } else {
        panic!("Expected DeviceSync payload");
    }
}
