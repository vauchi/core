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
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingHints {
    pub hints: Vec<ForwardingHintInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingHintInfo {
    pub blob_id: String,
    pub relay_url: String,
    pub expires_at_secs: u64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purge_request_roundtrip() {
        let req = PurgeRequest {
            include_device_sync: true,
            include_recovery_proofs: true,
            recovery_key_hash: Some("abc123".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: PurgeRequest = serde_json::from_str(&json).unwrap();
        assert!(decoded.include_device_sync);
        assert!(decoded.include_recovery_proofs);
        assert_eq!(decoded.recovery_key_hash, Some("abc123".to_string()));
    }

    #[test]
    fn test_purge_request_defaults() {
        let json = "{}";
        let decoded: PurgeRequest = serde_json::from_str(json).unwrap();
        assert!(!decoded.include_device_sync);
        assert!(!decoded.include_recovery_proofs);
        assert_eq!(decoded.recovery_key_hash, None);
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
        };
        let json = serde_json::to_string(&hints).unwrap();
        let decoded: ForwardingHints = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.hints.len(), 1);
        assert_eq!(decoded.hints[0].relay_url, "wss://peer.example.com");
    }
}
