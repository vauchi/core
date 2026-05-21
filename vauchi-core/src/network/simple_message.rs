// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Simplified Message Types for Relay Communication
//!
//! These types are used by mobile clients, CLI, and relay server for
//! simpler message passing where the full RatchetHeader isn't needed
//! in the wire format (it's embedded in the ciphertext instead).
//!
//! This module provides a common definition to avoid duplication across
//! vauchi-platform, vauchi-cli, and vauchi-relay.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identifiers::ContactId;

/// Errors from encoding/decoding simple relay messages.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SimpleMessageError {
    #[error("Frame too short")]
    FrameTooShort,

    #[error("Unsupported protocol version: {version}")]
    UnsupportedVersion { version: u8 },

    #[error("{0}")]
    Serialization(#[from] serde_json::Error),
}

/// Protocol version for simple messages.
pub const SIMPLE_PROTOCOL_VERSION: u8 = 1;

/// Frame header size (4 bytes length prefix).
pub const FRAME_HEADER_SIZE: usize = 4;

/// Simple message envelope for relay communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEnvelope {
    pub version: u8,
    pub message_id: String,
    pub timestamp: u64,
    pub payload: SimplePayload,
}

/// Payload types for simple relay messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum SimplePayload {
    /// Encrypted update (ciphertext contains full message including any headers).
    EncryptedUpdate(SimpleEncryptedUpdate),
    /// Acknowledgment.
    Acknowledgment(SimpleAcknowledgment),
    /// Client handshake for relay registration.
    Handshake(SimpleHandshake),
    /// Identity revocation signal (signed, not encrypted).
    IdentityRevoked(SimpleIdentityRevoked),
    /// Unknown message type (for forward compatibility).
    #[serde(other)]
    Unknown,
}

/// Identity revocation for the simple protocol.
///
/// Wire-compatible with `vauchi_protocol::IdentityRevoked` so the relay
/// can route it to the recipient without changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleIdentityRevoked {
    pub sender_id: ContactId,
    pub recipient_id: ContactId,
    pub timestamp: u64,
    pub signature: Vec<u8>,
}

/// Simple encrypted update - ciphertext is opaque to relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEncryptedUpdate {
    pub recipient_id: ContactId,
    pub sender_id: ContactId,
    /// Opaque ciphertext - may contain RatchetMessage, ExchangeMessage, etc.
    pub ciphertext: Vec<u8>,
}

/// Simple acknowledgment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleAcknowledgment {
    pub message_id: String,
    pub status: SimpleAckStatus,
}

/// Acknowledgment status.
///
/// Must stay aligned with the relay's `protocol::AckStatus` enum
/// to ensure wire compatibility for JSON deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SimpleAckStatus {
    /// Relay has persisted the message (store-and-forward).
    Stored,
    /// Message delivered to the recipient's connection.
    Delivered,
    /// Recipient has acknowledged receipt.
    ReceivedByRecipient,
    /// Delivery failed.
    Failed,
}

/// Simple handshake for relay registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleHandshake {
    /// Client's public ID (hex-encoded identity key).
    pub client_id: String,
    /// Optional device ID for inter-device sync (hex-encoded, 64 chars = 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Ed25519 public key proving ownership of client_id (hex, 64 chars = 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_public_key: Option<String>,
    /// Random nonce for replay prevention (hex, 64 chars = 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Ed25519 signature over `nonce || timestamp.to_be_bytes()` (hex, 128 chars = 64 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Unix timestamp in seconds, must be within ±60s of relay clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// Legacy exchange message format (plaintext, for backward compatibility).
///
/// New implementations should use EncryptedExchangeMessage from vauchi_core::exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyExchangeMessage {
    pub msg_type: String,
    /// Hex-encoded identity public key.
    pub identity_public_key: String,
    /// Hex-encoded ephemeral public key for X3DH.
    pub ephemeral_public_key: String,
    pub display_name: String,
    #[serde(default)]
    pub is_response: bool,
}

impl LegacyExchangeMessage {
    /// Create a new exchange request message.
    pub fn new(identity_key: &str, ephemeral_key: &str, display_name: &str) -> Self {
        Self {
            msg_type: "exchange".to_string(),
            identity_public_key: identity_key.to_string(),
            ephemeral_public_key: ephemeral_key.to_string(),
            display_name: display_name.to_string(),
            is_response: false,
        }
    }

    /// Create a response to an exchange request.
    pub fn new_response(identity_key: &str, ephemeral_key: &str, display_name: &str) -> Self {
        Self {
            msg_type: "exchange".to_string(),
            identity_public_key: identity_key.to_string(),
            ephemeral_public_key: ephemeral_key.to_string(),
            display_name: display_name.to_string(),
            is_response: true,
        }
    }

    /// Check if data is a legacy exchange message.
    pub fn is_exchange(data: &[u8]) -> bool {
        if let Ok(msg) = serde_json::from_slice::<LegacyExchangeMessage>(data) {
            msg.msg_type == "exchange"
        } else {
            false
        }
    }

    /// Parse from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SimpleMessageError> {
        Ok(serde_json::to_vec(self)?)
    }
}

/// Create a new simple envelope with fresh ID and timestamp.
pub fn create_simple_envelope(payload: SimplePayload, now: u64) -> SimpleEnvelope {
    SimpleEnvelope {
        version: SIMPLE_PROTOCOL_VERSION,
        message_id: uuid::Uuid::new_v4().to_string(),
        timestamp: now,
        payload,
    }
}

/// Create an acknowledgment envelope.
pub fn create_simple_ack(message_id: &str, status: SimpleAckStatus, now: u64) -> SimpleEnvelope {
    create_simple_envelope(
        SimplePayload::Acknowledgment(SimpleAcknowledgment {
            message_id: message_id.to_string(),
            status,
        }),
        now,
    )
}

/// Encode a simple envelope to bytes with length prefix.
pub fn encode_simple_message(envelope: &SimpleEnvelope) -> Result<Vec<u8>, SimpleMessageError> {
    let json = serde_json::to_vec(envelope)?;
    let len = json.len() as u32;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);

    Ok(frame)
}

/// Creates a signed handshake that the relay can verify with Ed25519.
///
/// Signs `nonce || timestamp.to_be_bytes()` using the identity's signing key.
/// The relay verifies the signature, checks the timestamp window (±60s),
/// and prevents nonce replay.
pub fn create_signed_handshake(
    identity: &crate::Identity,
    device_id: Option<String>,
    now: u64,
) -> SimpleHandshake {
    let client_id = identity.public_id();

    // Generate random 32-byte nonce
    let nonce_bytes: [u8; 32] = crate::crypto::random_bytes();

    let timestamp = now;

    // Sign: nonce || timestamp.to_be_bytes()
    let mut signed_data = Vec::with_capacity(40);
    signed_data.extend_from_slice(&nonce_bytes);
    signed_data.extend_from_slice(&timestamp.to_be_bytes());

    let signature = identity.signing_keypair().sign(&signed_data);
    let public_key = identity.signing_keypair().public_key();

    SimpleHandshake {
        client_id,
        device_id,
        identity_public_key: Some(hex_encode(public_key.as_bytes())),
        nonce: Some(hex_encode(&nonce_bytes)),
        signature: Some(hex_encode(signature.as_bytes())),
        timestamp: Some(timestamp),
    }
}

/// Hex-encodes a byte slice to a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decode a simple envelope from bytes with length prefix.
pub fn decode_simple_message(data: &[u8]) -> Result<SimpleEnvelope, SimpleMessageError> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err(SimpleMessageError::FrameTooShort);
    }

    let json = &data[FRAME_HEADER_SIZE..];
    let envelope: SimpleEnvelope = serde_json::from_slice(json)?;

    if envelope.version != SIMPLE_PROTOCOL_VERSION {
        return Err(SimpleMessageError::UnsupportedVersion {
            version: envelope.version,
        });
    }

    Ok(envelope)
}

// ===========================================================================
// Tests
// Trace: codebase-review-tracker item #50
// ===========================================================================

// INLINE_TEST_REQUIRED: tests use private hex_encode and create_signed_handshake internals
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_handshake_has_auth_fields() {
        let identity = crate::Identity::create("Test User", 0);
        let handshake = create_signed_handshake(&identity, None, 0);

        assert!(!handshake.client_id.is_empty());
        assert!(
            handshake.identity_public_key.is_some(),
            "expected Some value"
        );
        assert!(handshake.nonce.is_some(), "expected Some value");
        assert!(handshake.signature.is_some(), "expected Some value");
        assert!(handshake.timestamp.is_some(), "expected Some value");

        // Public key should be 64 hex chars (32 bytes)
        assert_eq!(handshake.identity_public_key.as_ref().unwrap().len(), 64);
        // Nonce should be 64 hex chars (32 bytes)
        assert_eq!(handshake.nonce.as_ref().unwrap().len(), 64);
        // Signature should be 128 hex chars (64 bytes)
        assert_eq!(handshake.signature.as_ref().unwrap().len(), 128);
    }

    #[test]
    fn test_signed_handshake_signature_verifiable() {
        let identity = crate::Identity::create("Test User", 0);
        let handshake = create_signed_handshake(&identity, None, 0);

        // Reconstruct the signed data as the relay would
        let nonce_hex = handshake.nonce.as_ref().unwrap();
        let sig_hex = handshake.signature.as_ref().unwrap();
        let pk_hex = handshake.identity_public_key.as_ref().unwrap();
        let timestamp = handshake.timestamp.unwrap();

        // Decode hex values
        let nonce_bytes: Vec<u8> = (0..nonce_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&nonce_hex[i..i + 2], 16).unwrap())
            .collect();
        let sig_bytes: Vec<u8> = (0..sig_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&sig_hex[i..i + 2], 16).unwrap())
            .collect();
        let pk_bytes: Vec<u8> = (0..pk_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&pk_hex[i..i + 2], 16).unwrap())
            .collect();

        // Reconstruct signed data: nonce || timestamp.to_be_bytes()
        let mut signed_data = Vec::with_capacity(40);
        signed_data.extend_from_slice(&nonce_bytes);
        signed_data.extend_from_slice(&timestamp.to_be_bytes());

        // Verify with ed25519 (as the relay does)
        let pk_array: [u8; 32] = pk_bytes.try_into().expect("pk should be 32 bytes");
        let sig_array: [u8; 64] = sig_bytes.try_into().expect("sig should be 64 bytes");
        let public_key = crate::crypto::signing::PublicKey::from_bytes(pk_array);
        let signature = crate::crypto::signing::Signature::from_bytes(sig_array);
        assert!(
            public_key.verify(&signed_data, &signature),
            "expected success"
        );
    }

    #[test]
    fn test_signed_handshake_with_device_id() {
        let identity = crate::Identity::create("Test User", 0);
        let device_id = Some("abcd1234".to_string());
        let handshake = create_signed_handshake(&identity, device_id.clone(), 0);

        assert_eq!(handshake.device_id, device_id);
        assert!(handshake.signature.is_some(), "expected Some value");
    }

    #[test]
    fn test_signed_handshake_serialization_includes_auth() {
        let identity = crate::Identity::create("Test User", 0);
        let handshake = create_signed_handshake(&identity, None, 0);

        let envelope = create_simple_envelope(SimplePayload::Handshake(handshake), 0);
        let encoded = encode_simple_message(&envelope).unwrap();
        let decoded = decode_simple_message(&encoded).unwrap();

        if let SimplePayload::Handshake(h) = decoded.payload {
            assert!(h.identity_public_key.is_some(), "expected Some value");
            assert!(h.nonce.is_some(), "expected Some value");
            assert!(h.signature.is_some(), "expected Some value");
            assert!(h.timestamp.is_some(), "expected Some value");
        } else {
            panic!("Expected Handshake payload");
        }
    }

    #[test]
    fn test_identity_revoked_simple_roundtrip() {
        let revoked = SimpleIdentityRevoked {
            sender_id: "sender123".to_string().into(),
            recipient_id: "recipient456".to_string().into(),
            timestamp: 1700000000,
            signature: vec![0xAB; 64],
        };
        let envelope = create_simple_envelope(SimplePayload::IdentityRevoked(revoked), 0);
        let encoded = encode_simple_message(&envelope).unwrap();
        let decoded = decode_simple_message(&encoded).unwrap();
        match decoded.payload {
            SimplePayload::IdentityRevoked(r) => {
                assert_eq!(r.sender_id, "sender123");
                assert_eq!(r.recipient_id, "recipient456");
                assert_eq!(r.timestamp, 1700000000);
                assert_eq!(r.signature.len(), 64);
                assert!(r.signature.iter().all(|b| *b == 0xAB));
            }
            _ => panic!("Expected IdentityRevoked"),
        }
    }

    #[test]
    fn test_identity_revoked_wire_compatible_with_protocol() {
        // The type tag must match vauchi-protocol's MessagePayload::IdentityRevoked
        let revoked = SimpleIdentityRevoked {
            sender_id: "s".to_string().into(),
            recipient_id: "r".to_string().into(),
            timestamp: 1000,
            signature: vec![1, 2, 3],
        };
        let payload = SimplePayload::IdentityRevoked(revoked);
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""type":"IdentityRevoked""#));
        assert!(json.contains(r#""sender_id":"s""#));
        assert!(json.contains(r#""recipient_id":"r""#));
    }

    #[test]
    fn test_unsigned_handshake_backward_compatible() {
        // Old-style handshake without auth fields should still deserialize
        let handshake = SimpleHandshake {
            client_id: "abc123".to_string(),
            device_id: None,
            identity_public_key: None,
            nonce: None,
            signature: None,
            timestamp: None,
        };

        let envelope = create_simple_envelope(SimplePayload::Handshake(handshake), 0);
        let encoded = encode_simple_message(&envelope).unwrap();
        let decoded = decode_simple_message(&encoded).unwrap();

        if let SimplePayload::Handshake(h) = decoded.payload {
            assert_eq!(h.client_id, "abc123");
            assert!(h.identity_public_key.is_none());
            assert!(h.nonce.is_none());
            assert!(h.signature.is_none());
            assert!(h.timestamp.is_none());
        } else {
            panic!("Expected Handshake payload");
        }
    }
}
