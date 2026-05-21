// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire-shape regression tests for Phase 1B of the
//! `2026-05-21-wire-identifier-newtypes` problem record.
//!
//! Captures the exact JSON output (base64 string for each 32-byte
//! public-key field) of every wire struct that Phase 1B swaps from
//! `[u8; 32]` to `IdentityKey` / `DhPublicKey`. The tests pass with
//! the pre-swap `[u8; 32]` shape and must continue to pass after the
//! field swap — proving the per-field `#[serde(with = "...")]`
//! base64 adapter produces byte-identical JSON to the existing
//! `bytes_array_32` module.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use vauchi_core::exchange::EncryptedExchangeMessage;
use vauchi_core::network::{
    DeletionStage, Handshake, IdentityDeletionNotice, MessageEnvelope, MessagePayload,
    PROTOCOL_VERSION, PurgeRequest, RatchetHeader,
};
use vauchi_core::sync::device_sync::ContactSyncData;

/// All-bytes-equal sentinels that round-trip through base64 cleanly
/// and are trivial to read in failing diffs.
const ID_BYTES: [u8; 32] = [0x11; 32];
const DH_BYTES: [u8; 32] = [0x22; 32];
const TOKEN_BYTES: [u8; 32] = [0x33; 32];

fn b64(bytes: &[u8; 32]) -> String {
    BASE64.encode(bytes)
}

// @internal
#[test]
fn ratchet_header_dh_public_is_base64_string() {
    let header = RatchetHeader {
        dh_public: DH_BYTES,
        dh_generation: 5,
        message_index: 10,
        previous_chain_length: 3,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&header).unwrap()).unwrap();
    assert_eq!(json["dh_public"], serde_json::Value::String(b64(&DH_BYTES)));
}

// @internal
#[test]
fn handshake_identity_public_key_is_base64_string() {
    let handshake = Handshake {
        identity_public_key: ID_BYTES,
        nonce: [0x44; 32],
        signature: [0x55; 64],
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&handshake).unwrap()).unwrap();
    assert_eq!(
        json["identity_public_key"],
        serde_json::Value::String(b64(&ID_BYTES))
    );
    // nonce stays [u8; 32] (random nonce family, deferred). Confirm
    // wire shape unchanged across the identity_public_key swap.
    assert_eq!(json["nonce"], serde_json::Value::String(b64(&[0x44; 32])));
}

// @internal
#[test]
fn identity_deletion_notice_public_key_is_base64_string() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "snapshot-001".to_string(),
        timestamp: 0,
        payload: MessagePayload::IdentityDeletionNotice(IdentityDeletionNotice {
            stage: DeletionStage::Confirmed,
            public_key: ID_BYTES,
            timestamp: 0,
            signature: [0xAA; 64],
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    let pk = &json["payload"]["IdentityDeletionNotice"]["public_key"];
    assert_eq!(pk, &serde_json::Value::String(b64(&ID_BYTES)));
}

// @internal
#[test]
fn purge_request_public_key_and_token_are_base64_strings() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "snapshot-002".to_string(),
        timestamp: 0,
        payload: MessagePayload::PurgeRequest(PurgeRequest {
            public_key: ID_BYTES,
            signature: vec![0xBB; 64],
            purge_token: TOKEN_BYTES,
            timestamp: 0,
        }),
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    let pr = &json["payload"]["PurgeRequest"];
    assert_eq!(pr["public_key"], serde_json::Value::String(b64(&ID_BYTES)));
    // purge_token stays [u8; 32] (Phase 3 target), but the wire shape
    // must remain identical alongside the public_key swap.
    assert_eq!(
        pr["purge_token"],
        serde_json::Value::String(b64(&TOKEN_BYTES))
    );
}

// @internal
#[test]
fn contact_sync_data_public_key_is_base64_string() {
    let data = ContactSyncData {
        id: "contact-001".to_string(),
        public_key: ID_BYTES,
        display_name: "Snapshot".to_string(),
        card_json: "{}".to_string(),
        shared_key: [0xCC; 32],
        exchange_timestamp: 0,
        fingerprint_verified: false,
        visibility_rules_json: "{}".to_string(),
        recovery_trusted: false,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
    assert_eq!(
        json["public_key"],
        serde_json::Value::String(b64(&ID_BYTES))
    );
    // shared_key stays [u8; 32] (symmetric, deferred); wire shape
    // must remain identical alongside the public_key swap.
    assert_eq!(
        json["shared_key"],
        serde_json::Value::String(b64(&[0xCC; 32]))
    );
}

// @internal
#[test]
fn encrypted_exchange_message_pubkeys_are_base64_strings() {
    let msg = EncryptedExchangeMessage {
        sender_exchange_key: DH_BYTES,
        ephemeral_public_key: [0x66; 32],
        ciphertext: vec![0xEE; 16],
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
    assert_eq!(
        json["sender_exchange_key"],
        serde_json::Value::String(b64(&DH_BYTES))
    );
    assert_eq!(
        json["ephemeral_public_key"],
        serde_json::Value::String(b64(&[0x66; 32]))
    );
}
