// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire-shape regression tests for Phase 3 of the
//! `2026-05-21-wire-identifier-newtypes` problem record.
//!
//! Captures the exact JSON output (raw string) of every wire struct
//! whose `message_id` field will swap from bare `String` (the
//! current `pub type MessageId = String;` alias) to `MessageId`
//! newtype in Phase 3. The tests pass with the pre-swap `String`
//! shape and must continue to pass after the swap — proving the
//! `#[serde(transparent)]` newtype produces byte-identical JSON
//! to the bare string.
//!
//! Sites covered: MessageEnvelope.message_id, Acknowledgment.message_id.

use vauchi_core::identifiers::{ContactId, DhPublicKey, IdentityKey};
use vauchi_core::network::{
    AckStatus, Acknowledgment, EncryptedUpdate, Handshake, MessageEnvelope, MessagePayload,
    PROTOCOL_VERSION, RatchetHeader,
};

const ENVELOPE_ID: &str = "8b0cba8e-6ff2-4a4b-9c84-fb74a3f93f01";
const ACK_ID: &str = "9c1dcb9f-7ff3-5b5c-ad95-fc85b4f04f02";

// @internal
#[test]
fn message_envelope_message_id_is_raw_string() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: ENVELOPE_ID.to_string(),
        timestamp: 0,
        payload: MessagePayload::Handshake(Handshake {
            identity_public_key: IdentityKey::from_bytes([0x11; 32]),
            nonce: [0x22; 32],
            signature: [0x33; 64],
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    assert_eq!(
        json["message_id"],
        serde_json::Value::String(ENVELOPE_ID.to_string())
    );
}

// @internal
#[test]
fn acknowledgment_message_id_is_raw_string() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: ENVELOPE_ID.to_string(),
        timestamp: 0,
        payload: MessagePayload::Acknowledgment(Acknowledgment {
            message_id: ACK_ID.to_string(),
            status: AckStatus::Delivered,
            error: None,
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    let ack = &json["payload"]["Acknowledgment"];
    assert_eq!(
        ack["message_id"],
        serde_json::Value::String(ACK_ID.to_string())
    );
    // Outer envelope keeps its own message_id (different from ack target).
    assert_eq!(
        json["message_id"],
        serde_json::Value::String(ENVELOPE_ID.to_string())
    );
}

// @internal
#[test]
fn encrypted_update_envelope_message_id_is_raw_string() {
    // EncryptedUpdate has no message_id of its own (the envelope carries it).
    // This test pins the envelope-level wire shape across the Phase 3 swap
    // when the inner payload is the most common variant.
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: ENVELOPE_ID.to_string(),
        timestamp: 0,
        payload: MessagePayload::EncryptedUpdate(EncryptedUpdate {
            recipient_id: ContactId::from(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ),
            sender_id: ContactId::from(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            ),
            ratchet_header: RatchetHeader {
                dh_public: DhPublicKey::from_bytes([0x44; 32]),
                dh_generation: 0,
                message_index: 0,
                previous_chain_length: 0,
            },
            ciphertext: vec![0xAA; 8],
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    assert_eq!(
        json["message_id"],
        serde_json::Value::String(ENVELOPE_ID.to_string())
    );
}
