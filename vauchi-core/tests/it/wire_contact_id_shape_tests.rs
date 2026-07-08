// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire-shape regression tests for Phase 2 of the
//! `2026-05-21-wire-identifier-newtypes` problem record.
//!
//! Captures the exact JSON output (raw string) of every wire struct
//! whose `sender_id` / `recipient_id` field will swap from bare
//! `String` to `ContactId(String)` in Phase 2. The tests pass with
//! the pre-swap `String` shape and must continue to pass after the
//! field swap — proving the `#[serde(transparent)]` newtype produces
//! byte-identical JSON to the bare string.
//!
//! Sites covered: EncryptedUpdate.{sender_id,recipient_id},
//! IdentityRevoked.{sender_id,recipient_id},
//! EmergencyAlert.sender_id.

use vauchi_core::identifiers::DhPublicKey;
use vauchi_core::network::{
    EmergencyAlert, EncryptedUpdate, IdentityRevoked, MessageEnvelope, MessagePayload,
    PROTOCOL_VERSION, RatchetHeader,
};

/// Hex-encoded sentinel IDs that are visually distinct in failing
/// diffs and represent the shape contacts use on the wire (hex
/// fingerprint of a 32-byte signing key).
const SENDER: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RECIPIENT: &str = "2222222222222222222222222222222222222222222222222222222222222222";

// @internal
#[test]
fn encrypted_update_sender_and_recipient_are_raw_strings() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "snapshot-eu-001".to_string().into(),
        timestamp: 0,
        payload: MessagePayload::EncryptedUpdate(EncryptedUpdate {
            recipient_id: RECIPIENT.to_string().into(),
            sender_id: SENDER.to_string().into(),
            ratchet_header: RatchetHeader {
                dh_public: DhPublicKey::from_bytes([0x33; 32]),
                dh_generation: 0,
                message_index: 0,
                previous_chain_length: 0,
            },
            ciphertext: vec![0xCC; 8],
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    let payload = &json["payload"]["EncryptedUpdate"];
    assert_eq!(
        payload["recipient_id"],
        serde_json::Value::String(RECIPIENT.to_string())
    );
    assert_eq!(
        payload["sender_id"],
        serde_json::Value::String(SENDER.to_string())
    );
}

// @internal
#[test]
fn identity_revoked_sender_and_recipient_are_raw_strings() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "snapshot-ir-001".to_string().into(),
        timestamp: 0,
        payload: MessagePayload::IdentityRevoked(IdentityRevoked {
            sender_id: SENDER.to_string().into(),
            recipient_id: RECIPIENT.to_string().into(),
            timestamp: 42,
            signature: [0xDD; 64],
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    let payload = &json["payload"]["IdentityRevoked"];
    assert_eq!(
        payload["sender_id"],
        serde_json::Value::String(SENDER.to_string())
    );
    assert_eq!(
        payload["recipient_id"],
        serde_json::Value::String(RECIPIENT.to_string())
    );
}

// @internal
#[test]
fn emergency_alert_sender_is_raw_string() {
    let alert = EmergencyAlert {
        sender_id: SENDER.to_string().into(),
        message: "test".to_string(),
        timestamp: 0,
        location: None,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&alert).unwrap()).unwrap();
    assert_eq!(
        json["sender_id"],
        serde_json::Value::String(SENDER.to_string())
    );
}
