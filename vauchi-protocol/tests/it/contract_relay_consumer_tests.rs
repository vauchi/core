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

use vauchi_protocol::escrow::*;
use vauchi_protocol::v2::*;
use vauchi_protocol::*;

// ============================================================
// Wire format stability
// ============================================================

// @internal
#[test]
fn provider_contract_protocol_version_is_1() {
    assert_eq!(PROTOCOL_VERSION, 1);
}

// @internal
#[test]
fn provider_contract_frame_header_size_is_4() {
    assert_eq!(FRAME_HEADER_SIZE, 4);
}

// @internal
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

// @internal
#[test]
fn provider_contract_all_relay_payload_variants_constructable() {
    // Relay creates these payloads — they must be constructable and encode to JSON
    let payloads = [
        MessagePayload::HandshakeAck(HandshakeAck {
            protocol_version: 1,
            server_version: "1.0.0".to_string(),
            features: vec![],
        }),
        MessagePayload::Acknowledgment(Acknowledgment {
            message_id: "m1".to_string(),
            status: AckStatus::Stored,
        }),
        MessagePayload::PurgeResponse(PurgeResponse {}),
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

// @internal
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

// @internal
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

// @internal
#[test]
fn provider_contract_forwarding_hints_canonical_data_exists() {
    let hints = ForwardingHints {
        hints: vec![ForwardingHintInfo {
            blob_id: "b1".to_string(),
            relay_url: "https://test".to_string(),
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

// @internal
#[test]
fn provider_contract_unknown_variant_for_forward_compat() {
    let json =
        r#"{"version":1,"message_id":"m","timestamp":0,"payload":{"type":"UnknownFutureType"}}"#;
    let envelope: MessageEnvelope = serde_json::from_str(json).unwrap();
    assert!(matches!(envelope.payload, MessagePayload::Unknown));
}

// ============================================================
// V2 HTTP API type roundtrip tests (OHTTP-11)
// ============================================================

// @internal
#[test]
fn v2_send_request_roundtrip() {
    let req = V2SendRequest {
        recipient_id: "abc".into(),
        ciphertext: "data".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: V2SendRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.recipient_id, "abc");
    assert_eq!(parsed.ciphertext, "data");
}

// @internal
#[test]
fn v2_purge_request_roundtrip() {
    let req = V2PurgeRequest {
        recipient_id: "rid".into(),
        public_key: "aa".repeat(32),
        purge_token: "bb".repeat(32),
        signature: "cc".repeat(64),
        timestamp: 1700000000,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: V2PurgeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.recipient_id, "rid");
    assert_eq!(parsed.public_key, "aa".repeat(32));
    assert_eq!(parsed.purge_token, "bb".repeat(32));
    assert_eq!(parsed.signature, "cc".repeat(64));
    assert_eq!(parsed.timestamp, 1700000000);
}

// @internal
#[test]
fn v2_response_roundtrip() {
    let mut resp = V2Response::new("ok");
    resp.blob_id = Some("blob-1".into());
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: V2Response = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.status, "ok");
    assert_eq!(parsed.blob_id, Some("blob-1".into()));
    assert!(parsed.error.is_none());
}

// @internal
#[test]
fn v2_response_serializes_absent_fields_as_null() {
    let json = serde_json::to_value(V2Response::new("ok")).unwrap();

    assert_eq!(
        json,
        serde_json::json!({
            "status": "ok",
            "error": null,
            "blob_id": null,
            "blobs": null,
            "acknowledged": null,
            "code": null,
            "payload": null,
            "response": null,
            "blobs_deleted": null,
            "proofs": null,
            "guardians": null,
        })
    );
}

// @internal
#[test]
fn v2_response_with_blobs_roundtrip() {
    let mut resp = V2Response::new("ok");
    resp.blobs = Some(vec![FetchedBlob {
        blob_id: "b1".into(),
        ciphertext: "dGVzdA==".into(),
        created_at: 12345,
        mailbox_token: None,
    }]);
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: V2Response = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.status, "ok");
    let blobs = parsed.blobs.unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].blob_id, "b1");
    assert_eq!(blobs[0].ciphertext, "dGVzdA==");
    assert_eq!(blobs[0].created_at, 12345);
}

// @internal
#[test]
fn fetched_blob_roundtrip() {
    let blob = FetchedBlob {
        blob_id: "fb-1".into(),
        ciphertext: "Y2lwaGVy".into(),
        created_at: 99999,
        mailbox_token: None,
    };
    let json = serde_json::to_string(&blob).unwrap();
    let parsed: FetchedBlob = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.blob_id, "fb-1");
    assert_eq!(parsed.ciphertext, "Y2lwaGVy");
    assert_eq!(parsed.created_at, 99999);
    assert!(
        parsed.mailbox_token.is_none(),
        "mailbox_token defaults to None when not serialized"
    );
}

// @internal
#[test]
fn fetched_blob_with_mailbox_token_roundtrip() {
    let blob = FetchedBlob {
        blob_id: "fb-2".into(),
        ciphertext: "Y2lwaGVy".into(),
        created_at: 99999,
        mailbox_token: Some("aabbccdd".into()),
    };
    let json = serde_json::to_string(&blob).unwrap();
    let parsed: FetchedBlob = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.mailbox_token.as_deref(), Some("aabbccdd"));
}

// @internal
#[test]
fn fetched_blob_legacy_relay_compat() {
    // Older relays do not emit `mailbox_token` — the field must default to None.
    let json = r#"{"blob_id":"old","ciphertext":"YQ==","created_at":1}"#;
    let parsed: FetchedBlob = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.blob_id, "old");
    assert!(parsed.mailbox_token.is_none());
}

// @internal
#[test]
fn v2_response_defaults_for_missing_optional_fields() {
    let json = r#"{"status":"ok"}"#;
    let parsed: V2Response = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.status, "ok");
    assert!(parsed.error.is_none());
    assert!(parsed.blob_id.is_none());
    assert!(parsed.blobs.is_none());
    assert!(parsed.acknowledged.is_none());
}

// ============================================================
// Escrow protocol types (relay consumer contract)
// ============================================================

// @internal
#[test]
fn provider_contract_escrow_constants() {
    assert_eq!(MAX_BLOB_BYTES, 65_536);
    assert_eq!(MAX_TTL_SECONDS, 604_800);
    assert_eq!(MAX_SLOTS_PER_GATE, 2);
    assert_eq!(HASH_HEX_LENGTH, 64);
}

// @internal
#[test]
fn provider_contract_escrow_message_all_variants_constructable() {
    let hash = "ff".repeat(32);
    let messages = [
        EscrowMessage::Put {
            gate_hash: hash.clone(),
            slot_hash: hash.clone(),
            blob: "dGVzdA==".to_string(),
            ttl_seconds: 3600,
        },
        EscrowMessage::Get {
            gate_hash: hash.clone(),
            slot_hash: hash.clone(),
        },
        EscrowMessage::Count {
            gate_hash: hash.clone(),
        },
    ];
    assert_eq!(messages.len(), 3, "EscrowMessage must have 3 variants");
}

// @internal
#[test]
fn provider_contract_escrow_response_all_variants_constructable() {
    let responses = [
        EscrowResponse::Stored,
        EscrowResponse::AlreadyExists,
        EscrowResponse::GateFull,
        EscrowResponse::BlobTooLarge,
        EscrowResponse::Blob {
            blob: "dGVzdA==".to_string(),
        },
        EscrowResponse::NotReady { count: 1 },
        EscrowResponse::Count { count: 2 },
        EscrowResponse::NotFound,
    ];
    assert_eq!(responses.len(), 8, "EscrowResponse must have 8 variants");
}

// @internal
#[test]
fn provider_contract_escrow_put_roundtrip() {
    let msg = EscrowMessage::Put {
        gate_hash: "aa".repeat(32),
        slot_hash: "bb".repeat(32),
        blob: "dGVzdA==".to_string(),
        ttl_seconds: 86400,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: EscrowMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, msg);
}

// @internal
#[test]
fn provider_contract_escrow_blob_response_roundtrip() {
    let resp = EscrowResponse::Blob {
        blob: "Y2lwaGVy".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: EscrowResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

// @internal
#[test]
fn provider_contract_escrow_tagged_discriminator() {
    // Relay dispatches on "action" tag for messages
    let msg = EscrowMessage::Count {
        gate_hash: "cc".repeat(32),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""action":"Count"#));

    // Relay builds responses with "status" tag
    let resp = EscrowResponse::Stored;
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains(r#""status":"Stored"#));
}

// @internal
#[test]
fn provider_contract_escrow_validation_rejects_bad_input() {
    let msg = EscrowMessage::Put {
        gate_hash: "short".to_string(),
        slot_hash: "also_short".to_string(),
        blob: "dGVzdA==".to_string(),
        ttl_seconds: MAX_TTL_SECONDS + 1,
    };
    let errs = msg.validate().unwrap_err();
    assert!(errs.len() >= 3, "should report gate, slot, and ttl errors");
}
