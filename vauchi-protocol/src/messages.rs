// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Protocol message types shared between Vauchi relay and clients.
//!
//! This module defines the wire format for all messages exchanged over the
//! relay WebSocket connection. Types are serde-only — no crypto, no I/O.

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub version: u8,
    pub message_id: String,
    pub timestamp: u64,
    pub payload: MessagePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessagePayload {
    EncryptedUpdate(EncryptedUpdate),
    Acknowledgment(Acknowledgment),
    Handshake(Handshake),
    RecoveryProofStore(RecoveryProofStore),
    RecoveryProofQuery(RecoveryProofQuery),
    RecoveryProofResponse(RecoveryProofResponse),
    DeviceSyncMessage(DeviceSyncMessage),
    DeviceSyncAck(DeviceSyncAck),
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
    /// Protocol versions the client supports (e.g. [1] or [1, 2]).
    /// Old clients omit this field; the server treats that as [1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_versions: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAck {
    pub protocol_version: u8,
    pub server_version: String,
    pub features: Vec<String>,
    /// Protocol versions the server supports, letting clients know about upgrades.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_versions: Option<Vec<u8>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProofResponse {
    pub proofs: Vec<RecoveryProofEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProofEntry {
    pub key_hash: String,
    pub proof_data: Vec<u8>,
}

// =========================================================================
// Device sync messages
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncMessage {
    pub identity_id: String,
    pub target_device_id: String,
    pub sender_device_id: String,
    pub encrypted_payload: Vec<u8>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncAck {
    pub message_id: String,
    pub synced_version: u64,
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
    pub include_device_sync: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeResponse {
    pub blobs_deleted: usize,
    pub device_sync_deleted: usize,
    #[serde(default)]
    pub recovery_proofs_deleted: usize,
}

// =========================================================================
// Account revocation
// =========================================================================

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
// Version negotiation
// =========================================================================

/// Negotiates the highest protocol version supported by both client and server.
///
/// - `client_versions`: versions the client declared in `Handshake.supported_versions`.
///   `None` means an old client that predates version negotiation (treated as `[1]`).
/// - `server_versions`: versions the server supports (e.g. `[1]` or `[1, 2]`).
///
/// Returns `Some(version)` on success, `None` if no overlap exists.
pub fn negotiate_version(client_versions: Option<&[u8]>, server_versions: &[u8]) -> Option<u8> {
    let client = client_versions.unwrap_or(&[PROTOCOL_VERSION]);
    client
        .iter()
        .filter(|v| server_versions.contains(v))
        .max()
        .copied()
}

// =========================================================================
// Framing helpers
// =========================================================================

/// Decodes a message from binary data (with length prefix).
pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, String> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err("Frame too short".to_string());
    }

    let json = &data[FRAME_HEADER_SIZE..];
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

/// Encodes a message to binary data (with length prefix).
pub fn encode_message(envelope: &MessageEnvelope) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(envelope).map_err(|e| e.to_string())?;
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
            include_device_sync: true,
            include_recovery_proofs: true,
            recovery_key_hash: Some("abc123".to_string()),
            public_key: Some("pk123".to_string()),
            signature: Some("sig456".to_string()),
            purge_token: Some("tok789".to_string()),
            timestamp: Some(1234567890),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: PurgeRequest = serde_json::from_str(&json).unwrap();
        assert!(decoded.include_device_sync);
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
        assert!(!decoded.include_device_sync);
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
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Frame too short");
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
    fn test_device_sync_message_roundtrip() {
        let msg = DeviceSyncMessage {
            identity_id: "id1".to_string(),
            target_device_id: "dev1".to_string(),
            sender_device_id: "dev2".to_string(),
            encrypted_payload: vec![10, 20, 30],
            version: 42,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: DeviceSyncMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.version, 42);
        assert_eq!(decoded.encrypted_payload, vec![10, 20, 30]);
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
            device_sync_deleted: 3,
            recovery_proofs_deleted: 1,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: PurgeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.blobs_deleted, 5);
        assert_eq!(decoded.device_sync_deleted, 3);
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

    // =====================================================================
    // Version negotiation tests (T1-4)
    // =====================================================================

    #[test]
    fn test_negotiate_version_both_support_v1_returns_v1() {
        let result = negotiate_version(Some(&[1]), &[1]);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_negotiate_version_client_supports_v1_v2_server_v1_returns_v1() {
        let result = negotiate_version(Some(&[1, 2]), &[1]);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_negotiate_version_both_support_v1_v2_returns_v2() {
        let result = negotiate_version(Some(&[1, 2]), &[1, 2]);
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_negotiate_version_no_overlap_returns_none() {
        let result = negotiate_version(Some(&[2, 3]), &[1]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_negotiate_version_client_none_defaults_to_v1() {
        // Old clients that don't send supported_versions
        let result = negotiate_version(None, &[1, 2]);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_negotiate_version_client_empty_returns_none() {
        let result = negotiate_version(Some(&[]), &[1]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_negotiate_version_picks_highest_common() {
        let result = negotiate_version(Some(&[1, 3, 5]), &[2, 3, 4, 5]);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_handshake_supported_versions_serde_roundtrip() {
        let hs = Handshake {
            client_id: "abc".to_string(),
            device_id: None,
            routing_token: None,
            suppress_presence: false,
            identity_public_key: None,
            nonce: None,
            signature: None,
            timestamp: None,
            supported_versions: Some(vec![1, 2]),
        };
        let json = serde_json::to_string(&hs).unwrap();
        let decoded: Handshake = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.supported_versions, Some(vec![1, 2]));
    }

    #[test]
    fn test_handshake_supported_versions_backward_compat() {
        // Old clients without supported_versions field
        let json = r#"{"client_id":"abc"}"#;
        let h: Handshake = serde_json::from_str(json).unwrap();
        assert_eq!(h.supported_versions, None);
    }

    #[test]
    fn test_handshake_ack_supported_versions_serde_roundtrip() {
        let ack = HandshakeAck {
            protocol_version: 1,
            server_version: "1.1.0".to_string(),
            features: vec!["routing_token".to_string()],
            supported_versions: Some(vec![1, 2]),
        };
        let json = serde_json::to_string(&ack).unwrap();
        let decoded: HandshakeAck = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.supported_versions, Some(vec![1, 2]));
    }

    #[test]
    fn test_handshake_ack_backward_compat_without_supported_versions() {
        let json = r#"{"protocol_version":1,"server_version":"1.0.0","features":[]}"#;
        let ack: HandshakeAck = serde_json::from_str(json).unwrap();
        assert_eq!(ack.protocol_version, 1);
        assert_eq!(ack.supported_versions, None);
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
