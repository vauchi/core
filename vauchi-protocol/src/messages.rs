// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Protocol message types shared between Vauchi relay and clients.
//!
//! This module defines the wire format for all messages exchanged over the
//! relay WebSocket connection. Types are serde-only — no crypto, no I/O.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from encoding/decoding protocol messages.
#[derive(Error, Debug)]
pub enum ProtocolMessageError {
    #[error("Frame too short")]
    FrameTooShort,

    #[error("Unsupported protocol version: {version}")]
    UnsupportedVersion { version: u8 },

    #[error("{0}")]
    Serialization(#[from] serde_json::Error),
}

// =========================================================================
// Constants
// =========================================================================

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Size of the length-prefix header in bytes (u32 big-endian).
pub const FRAME_HEADER_SIZE: usize = 4;

// =========================================================================
// Envelope
// =========================================================================

/// Top-level wire format wrapper for all relay protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub version: u8,
    pub message_id: String,
    pub timestamp: u64,
    pub payload: MessagePayload,
}

/// Tagged union of all protocol message types, discriminated by `"type"` in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessagePayload {
    EncryptedUpdate(EncryptedUpdate),
    Acknowledgment(Acknowledgment),
    Handshake(Handshake),
    RecoveryProofStore(RecoveryProofStore),
    RecoveryProofQuery(RecoveryProofQuery),
    RecoveryProofResponse(RecoveryProofResponse),
    HandshakeAck(HandshakeAck),
    PurgeRequest(PurgeRequest),
    PurgeResponse(PurgeResponse),
    AccountRevoked(AccountRevoked),
    ForwardingHints(ForwardingHints),
    DeviceLinkRelay(DeviceLinkRelay),
    #[serde(other)]
    Unknown,
}

// =========================================================================
// Core message types
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedUpdate {
    pub recipient_id: String,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acknowledgment {
    pub message_id: String,
    pub status: AckStatus,
}

/// Delivery status reported in an [`Acknowledgment`] message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AckStatus {
    /// Message received by relay and stored for delivery.
    Stored,
    /// Message delivered to recipient (recipient came online).
    Delivered,
    /// Recipient acknowledged receipt (end-to-end confirmation).
    ReceivedByRecipient,
    /// Delivery failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    pub client_id: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub routing_token: Option<String>,
    #[serde(default)]
    pub suppress_presence: bool,
    #[serde(default)]
    pub identity_public_key: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub timestamp: Option<u64>,
}

/// Server response to a [`Handshake`], confirming connection and advertising capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAck {
    pub protocol_version: u8,
    pub server_version: String,
    pub features: Vec<String>,
}

// =========================================================================
// Recovery proof messages
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProofStore {
    pub key_hash: String,
    pub proof_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProofQuery {
    pub key_hashes: Vec<String>,
}

/// Server response to a [`RecoveryProofQuery`], returning matching stored proofs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProofResponse {
    pub proofs: Vec<RecoveryProofEntry>,
}

/// A single recovery proof keyed by its hash, returned inside [`RecoveryProofResponse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProofEntry {
    pub key_hash: String,
    pub proof_data: Vec<u8>,
}

// =========================================================================
// Device link relay messages
// =========================================================================

/// Relay message for device linking protocol.
///
/// Routes encrypted blobs between existing and new devices using the
/// identity key (from QR) as a routing address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceLinkRelay {
    pub target_identity: String,
    pub sender_token: String,
    pub encrypted_payload: Vec<u8>,
}

// =========================================================================
// Purge messages
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeRequest {
    #[serde(default)]
    pub include_recovery_proofs: bool,
    #[serde(default)]
    pub recovery_key_hash: Option<String>,
    // v2: Signed purge fields (optional for backward compat)
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub purge_token: Option<String>,
    #[serde(default)]
    pub timestamp: Option<u64>,
}

/// Server response to a [`PurgeRequest`], reporting how many items were deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeResponse {
    pub blobs_deleted: usize,
    #[serde(default)]
    pub recovery_proofs_deleted: usize,
}

// =========================================================================
// Account revocation
// =========================================================================

/// Signed notification that a sender's account has been revoked, invalidating their contact card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRevoked {
    pub sender_id: String,
    pub recipient_id: String,
    pub timestamp: u64,
    pub signature: Vec<u8>,
}

// =========================================================================
// Forwarding hints (federation)
// =========================================================================

/// Forwarding hints with optional relay signature (Tracker #117).
///
/// When signed, the relay includes its Ed25519 public key and a signature
/// over the canonical hint data, allowing clients to verify the hints
/// originate from the authenticated relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingHints {
    pub hints: Vec<ForwardingHintInfo>,
    /// Relay's Ed25519 signing public key (32 bytes, hex-encoded).
    /// Present when the relay signs its forwarding hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_signing_key: Option<String>,
    /// Ed25519 signature over the canonical hint data (hex-encoded).
    /// Signed data: sorted hints concatenated as `blob_id || relay_url || expires_at_secs_be`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// A single forwarding hint directing a client to a federated relay holding a blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingHintInfo {
    pub blob_id: String,
    pub relay_url: String,
    pub expires_at_secs: u64,
}

impl ForwardingHints {
    /// Computes the canonical byte representation of the hints for signing.
    ///
    /// Hints are sorted by `blob_id` to ensure deterministic ordering.
    /// Each hint contributes: `blob_id_bytes || relay_url_bytes || expires_at_secs_be_bytes`.
    pub fn canonical_data(&self) -> Vec<u8> {
        let mut sorted_hints: Vec<&ForwardingHintInfo> = self.hints.iter().collect();
        sorted_hints.sort_by(|a, b| a.blob_id.cmp(&b.blob_id));

        let mut data = Vec::new();
        for hint in &sorted_hints {
            data.extend_from_slice(hint.blob_id.as_bytes());
            data.extend_from_slice(hint.relay_url.as_bytes());
            data.extend_from_slice(&hint.expires_at_secs.to_be_bytes());
        }
        data
    }
}

// =========================================================================
// Framing helpers
// =========================================================================

/// Decodes a message from binary data (with length prefix).
pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, ProtocolMessageError> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err(ProtocolMessageError::FrameTooShort);
    }

    let json = &data[FRAME_HEADER_SIZE..];
    let envelope: MessageEnvelope = serde_json::from_slice(json)?;

    if envelope.version != PROTOCOL_VERSION {
        return Err(ProtocolMessageError::UnsupportedVersion {
            version: envelope.version,
        });
    }

    Ok(envelope)
}

/// Encodes a message to binary data (with length prefix).
pub fn encode_message(envelope: &MessageEnvelope) -> Result<Vec<u8>, ProtocolMessageError> {
    let json = serde_json::to_vec(envelope)?;
    let len = json.len() as u32;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);

    Ok(frame)
}

// =========================================================================
// Tests
// =========================================================================

// INLINE_TEST_REQUIRED: Serde roundtrip tests for protocol message types colocated with type definitions
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purge_request_roundtrip() {
        let req = PurgeRequest {
            include_recovery_proofs: true,
            recovery_key_hash: Some("abc123".to_string()),
            public_key: Some("pk123".to_string()),
            signature: Some("sig456".to_string()),
            purge_token: Some("tok789".to_string()),
            timestamp: Some(1234567890),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: PurgeRequest = serde_json::from_str(&json).unwrap();
        assert!(decoded.include_recovery_proofs);
        assert_eq!(decoded.recovery_key_hash, Some("abc123".to_string()));
        assert_eq!(decoded.public_key, Some("pk123".to_string()));
        assert_eq!(decoded.signature, Some("sig456".to_string()));
        assert_eq!(decoded.purge_token, Some("tok789".to_string()));
        assert_eq!(decoded.timestamp, Some(1234567890));
    }

    #[test]
    fn test_purge_request_defaults() {
        let json = "{}";
        let decoded: PurgeRequest = serde_json::from_str(json).unwrap();
        assert!(!decoded.include_recovery_proofs);
        assert_eq!(decoded.recovery_key_hash, None);
        assert_eq!(decoded.public_key, None);
        assert_eq!(decoded.signature, None);
        assert_eq!(decoded.purge_token, None);
        assert_eq!(decoded.timestamp, None);
    }

    #[test]
    fn test_envelope_roundtrip() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-123".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::Acknowledgment(Acknowledgment {
                message_id: "orig-456".to_string(),
                status: AckStatus::Stored,
            }),
        };

        let encoded = encode_message(&envelope).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.message_id, "test-123");
        assert_eq!(decoded.timestamp, 1234567890);

        if let MessagePayload::Acknowledgment(ack) = decoded.payload {
            assert_eq!(ack.message_id, "orig-456");
            assert_eq!(ack.status, AckStatus::Stored);
        } else {
            panic!("Expected Acknowledgment payload");
        }
    }

    #[test]
    fn test_decode_frame_too_short() {
        let result = decode_message(&[0, 1, 2]);
        assert!(result.is_err(), "expected error");
        assert_eq!(result.unwrap_err().to_string(), "Frame too short");
    }

    #[test]
    fn test_decode_rejects_unsupported_protocol_version() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION + 1,
            message_id: "test-bad-version".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::Acknowledgment(Acknowledgment {
                message_id: "orig".to_string(),
                status: AckStatus::Stored,
            }),
        };
        let encoded = encode_message(&envelope).unwrap();
        let result = decode_message(&encoded);
        assert!(
            result.is_err(),
            "Should reject unsupported protocol version"
        );
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("Unsupported protocol version"),
            "Error should mention version: {err_msg}"
        );
    }

    #[test]
    fn test_decode_rejects_version_zero() {
        let envelope = MessageEnvelope {
            version: 0,
            message_id: "test-zero-version".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::Acknowledgment(Acknowledgment {
                message_id: "orig".to_string(),
                status: AckStatus::Stored,
            }),
        };
        let encoded = encode_message(&envelope).unwrap();
        let result = decode_message(&encoded);
        assert!(result.is_err(), "Should reject version 0");
    }

    #[test]
    fn test_decode_rejects_version_255() {
        let envelope = MessageEnvelope {
            version: 255,
            message_id: "test-max-version".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::Acknowledgment(Acknowledgment {
                message_id: "orig".to_string(),
                status: AckStatus::Stored,
            }),
        };
        let encoded = encode_message(&envelope).unwrap();
        let result = decode_message(&encoded);
        assert!(result.is_err(), "Should reject version 255");
    }

    #[test]
    fn test_encrypted_update_roundtrip() {
        let update = EncryptedUpdate {
            recipient_id: "deadbeef".repeat(8),
            ciphertext: vec![1, 2, 3, 4],
        };
        let json = serde_json::to_string(&update).unwrap();
        let decoded: EncryptedUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.recipient_id, "deadbeef".repeat(8));
        assert_eq!(decoded.ciphertext, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_handshake_optional_fields() {
        let json = r#"{"client_id":"abc"}"#;
        let h: Handshake = serde_json::from_str(json).unwrap();
        assert_eq!(h.client_id, "abc");
        assert_eq!(h.device_id, None);
        assert_eq!(h.routing_token, None);
        assert!(!h.suppress_presence);
    }

    #[test]
    fn test_unknown_payload_variant() {
        let json = r#"{"version":1,"message_id":"m1","timestamp":0,"payload":{"type":"FutureFeature","data":"x"}}"#;
        let envelope: MessageEnvelope = serde_json::from_str(json).unwrap();
        assert!(matches!(envelope.payload, MessagePayload::Unknown));
    }

    #[test]
    fn test_purge_response_roundtrip() {
        let resp = PurgeResponse {
            blobs_deleted: 5,
            recovery_proofs_deleted: 1,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: PurgeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.blobs_deleted, 5);
        assert_eq!(decoded.recovery_proofs_deleted, 1);
    }

    #[test]
    fn test_forwarding_hints_roundtrip() {
        let hints = ForwardingHints {
            hints: vec![ForwardingHintInfo {
                blob_id: "blob1".to_string(),
                relay_url: "wss://peer.example.com".to_string(),
                expires_at_secs: 9999999999,
            }],
            relay_signing_key: None,
            signature: None,
        };
        let json = serde_json::to_string(&hints).unwrap();
        let decoded: ForwardingHints = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.hints.len(), 1);
        assert_eq!(decoded.hints[0].relay_url, "wss://peer.example.com");
        // Unsigned hints should not have signature fields in JSON
        assert!(!json.contains("relay_signing_key"));
        assert!(!json.contains("signature"));
    }

    #[test]
    fn test_forwarding_hints_signed_roundtrip() {
        let hints = ForwardingHints {
            hints: vec![ForwardingHintInfo {
                blob_id: "blob1".to_string(),
                relay_url: "wss://peer.example.com".to_string(),
                expires_at_secs: 9999999999,
            }],
            relay_signing_key: Some("ab".repeat(32)),
            signature: Some("cd".repeat(64)),
        };
        let json = serde_json::to_string(&hints).unwrap();
        let decoded: ForwardingHints = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.relay_signing_key, Some("ab".repeat(32)));
        assert_eq!(decoded.signature, Some("cd".repeat(64)));
    }

    #[test]
    fn test_forwarding_hints_canonical_data_deterministic() {
        let hints1 = ForwardingHints {
            hints: vec![
                ForwardingHintInfo {
                    blob_id: "blob-b".to_string(),
                    relay_url: "wss://relay-2.test".to_string(),
                    expires_at_secs: 2000,
                },
                ForwardingHintInfo {
                    blob_id: "blob-a".to_string(),
                    relay_url: "wss://relay-1.test".to_string(),
                    expires_at_secs: 1000,
                },
            ],
            relay_signing_key: None,
            signature: None,
        };
        // Same hints in different order
        let hints2 = ForwardingHints {
            hints: vec![
                ForwardingHintInfo {
                    blob_id: "blob-a".to_string(),
                    relay_url: "wss://relay-1.test".to_string(),
                    expires_at_secs: 1000,
                },
                ForwardingHintInfo {
                    blob_id: "blob-b".to_string(),
                    relay_url: "wss://relay-2.test".to_string(),
                    expires_at_secs: 2000,
                },
            ],
            relay_signing_key: None,
            signature: None,
        };
        assert_eq!(
            hints1.canonical_data(),
            hints2.canonical_data(),
            "canonical_data must be order-independent"
        );
    }

    #[test]
    fn test_forwarding_hints_backward_compatible_deserialization() {
        // Old format without signature fields
        let json =
            r#"{"hints":[{"blob_id":"b1","relay_url":"wss://r.test","expires_at_secs":100}]}"#;
        let decoded: ForwardingHints = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.hints.len(), 1);
        assert!(decoded.relay_signing_key.is_none());
        assert!(decoded.signature.is_none());
    }

    #[test]
    fn test_device_link_relay_roundtrip() {
        let msg = DeviceLinkRelay {
            target_identity: "abc123".to_string(),
            sender_token: "def456".to_string(),
            encrypted_payload: vec![1, 2, 3, 4],
        };
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "link-test-1".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::DeviceLinkRelay(msg),
        };
        let encoded = encode_message(&envelope).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.message_id, "link-test-1");
        assert_eq!(decoded.timestamp, 1234567890);

        match decoded.payload {
            MessagePayload::DeviceLinkRelay(m) => {
                assert_eq!(m.target_identity, "abc123");
                assert_eq!(m.sender_token, "def456");
                assert_eq!(m.encrypted_payload, vec![1, 2, 3, 4]);
            }
            _ => panic!("Expected DeviceLinkRelay payload"),
        }
    }
}
