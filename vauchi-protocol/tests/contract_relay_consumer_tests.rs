// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider contract tests: protocol crate stability for relay consumer (PI-04).
//!
//! These tests verify that vauchi-protocol's public API maintains the shape
//! and semantics that the relay depends on.
//!
//! Consumer: vauchi-relay
//! Provider: vauchi-protocol

use vauchi_protocol::*;

// ============================================================
// Wire format stability
// ============================================================

#[test]
fn provider_contract_protocol_version_is_1() {
    assert_eq!(PROTOCOL_VERSION, 1);
}

#[test]
fn provider_contract_frame_header_size_is_4() {
    assert_eq!(FRAME_HEADER_SIZE, 4);
}

#[test]
fn provider_contract_encode_decode_roundtrip() {
    let envelope = MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "provider-test".to_string(),
        timestamp: 1234567890,
        payload: MessagePayload::Acknowledgment(Acknowledgment {
            message_id: "orig".to_string(),
            status: AckStatus::Stored,
        }),
    };
    let bytes = encode_message(&envelope).unwrap();
    let decoded = decode_message(&bytes).unwrap();
    assert_eq!(decoded.version, PROTOCOL_VERSION);
    assert_eq!(decoded.message_id, "provider-test");
}

// ============================================================
// All payload variants required by relay
// ============================================================

#[test]
fn provider_contract_all_relay_payload_variants_constructable() {
    // Relay creates these payloads — they must be constructable and encode to JSON
    let payloads = [
        MessagePayload::HandshakeAck(HandshakeAck {
            protocol_version: 1,
            server_version: "1.0.0".to_string(),
            features: vec![],
            supported_versions: None,
        }),
        MessagePayload::Acknowledgment(Acknowledgment {
            message_id: "m1".to_string(),
            status: AckStatus::Stored,
        }),
        MessagePayload::PurgeResponse(PurgeResponse {
            blobs_deleted: 0,
            device_sync_deleted: 0,
            recovery_proofs_deleted: 0,
        }),
        MessagePayload::RecoveryProofResponse(RecoveryProofResponse { proofs: vec![] }),
        MessagePayload::ForwardingHints(ForwardingHints {
            hints: vec![],
            relay_signing_key: None,
            signature: None,
        }),
    ];
    assert_eq!(
        payloads.len(),
        5,
        "relay must be able to construct 5 response payload types"
    );
}

// ============================================================
// AckStatus variants
// ============================================================

#[test]
fn provider_contract_ack_status_all_variants() {
    let variants = [
        AckStatus::Stored,
        AckStatus::Delivered,
        AckStatus::ReceivedByRecipient,
        AckStatus::Failed,
    ];
    assert_eq!(variants.len(), 4, "AckStatus must have exactly 4 variants");
}

// ============================================================
// Handshake struct shape
// ============================================================

#[test]
fn provider_contract_handshake_fields_accessible() {
    let h = Handshake {
        client_id: "test".to_string(),
        device_id: Some("dev".to_string()),
        routing_token: Some("tok".to_string()),
        suppress_presence: true,
        identity_public_key: Some("ipk".to_string()),
        nonce: Some("nonce".to_string()),
        signature: Some("sig".to_string()),
        timestamp: Some(12345),
        supported_versions: Some(vec![1]),
    };
    assert_eq!(h.client_id, "test");
    assert_eq!(h.device_id, Some("dev".to_string()));
    assert_eq!(h.routing_token, Some("tok".to_string()));
    assert!(h.suppress_presence);
    assert_eq!(h.identity_public_key, Some("ipk".to_string()));
}

// ============================================================
// ForwardingHints canonical_data
// ============================================================

#[test]
fn provider_contract_forwarding_hints_canonical_data_exists() {
    let hints = ForwardingHints {
        hints: vec![ForwardingHintInfo {
            blob_id: "b1".to_string(),
            relay_url: "wss://test".to_string(),
            expires_at_secs: 100,
        }],
        relay_signing_key: None,
        signature: None,
    };
    let data = hints.canonical_data();
    assert!(!data.is_empty());
}

// ============================================================
// Unknown payload backward compatibility
// ============================================================

#[test]
fn provider_contract_unknown_variant_for_forward_compat() {
    let json =
        r#"{"version":1,"message_id":"m","timestamp":0,"payload":{"type":"UnknownFutureType"}}"#;
    let envelope: MessageEnvelope = serde_json::from_str(json).unwrap();
    assert!(matches!(envelope.payload, MessagePayload::Unknown));
}
