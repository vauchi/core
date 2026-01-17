//! Simplified Message Types for Relay Communication
//!
//! These types are used by mobile clients, CLI, and relay server for
//! simpler message passing where the full RatchetHeader isn't needed
//! in the wire format (it's embedded in the ciphertext instead).
//!
//! This module provides a common definition to avoid duplication across
//! webbook-mobile, webbook-cli, and webbook-relay.

use serde::{Deserialize, Serialize};

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
pub enum SimplePayload {
    /// Encrypted update (ciphertext contains full message including any headers).
    EncryptedUpdate(SimpleEncryptedUpdate),
    /// Acknowledgment.
    Acknowledgment(SimpleAcknowledgment),
    /// Client handshake for relay registration.
    Handshake(SimpleHandshake),
    /// Unknown message type (for forward compatibility).
    #[serde(other)]
    Unknown,
}

/// Simple encrypted update - ciphertext is opaque to relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEncryptedUpdate {
    pub recipient_id: String,
    pub sender_id: String,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimpleAckStatus {
    Delivered,
    ReceivedByRecipient,
    #[allow(dead_code)]
    Failed,
}

/// Simple handshake for relay registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleHandshake {
    /// Client's public ID (hex-encoded identity key).
    pub client_id: String,
}

/// Legacy exchange message format (plaintext, for backward compatibility).
///
/// New implementations should use EncryptedExchangeMessage from webbook_core::exchange.
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
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// Create a new simple envelope with fresh ID and timestamp.
pub fn create_simple_envelope(payload: SimplePayload) -> SimpleEnvelope {
    SimpleEnvelope {
        version: SIMPLE_PROTOCOL_VERSION,
        message_id: uuid::Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        payload,
    }
}

/// Create an acknowledgment envelope.
pub fn create_simple_ack(message_id: &str, status: SimpleAckStatus) -> SimpleEnvelope {
    create_simple_envelope(SimplePayload::Acknowledgment(SimpleAcknowledgment {
        message_id: message_id.to_string(),
        status,
    }))
}

/// Encode a simple envelope to bytes with length prefix.
pub fn encode_simple_message(envelope: &SimpleEnvelope) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(envelope).map_err(|e| e.to_string())?;
    let len = json.len() as u32;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);

    Ok(frame)
}

/// Decode a simple envelope from bytes with length prefix.
pub fn decode_simple_message(data: &[u8]) -> Result<SimpleEnvelope, String> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err("Frame too short".to_string());
    }

    let json = &data[FRAME_HEADER_SIZE..];
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let handshake = SimpleHandshake {
            client_id: "test-client".to_string(),
        };
        let envelope = create_simple_envelope(SimplePayload::Handshake(handshake));

        let encoded = encode_simple_message(&envelope).unwrap();
        let decoded = decode_simple_message(&encoded).unwrap();

        assert_eq!(decoded.version, SIMPLE_PROTOCOL_VERSION);
        assert_eq!(decoded.message_id, envelope.message_id);

        match decoded.payload {
            SimplePayload::Handshake(h) => assert_eq!(h.client_id, "test-client"),
            _ => panic!("Wrong payload type"),
        }
    }

    #[test]
    fn test_legacy_exchange_message() {
        let msg = LegacyExchangeMessage::new("abc123", "def456", "Alice");
        assert_eq!(msg.msg_type, "exchange");
        assert!(!msg.is_response);

        let bytes = msg.to_bytes();
        assert!(LegacyExchangeMessage::is_exchange(&bytes));

        let parsed = LegacyExchangeMessage::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.display_name, "Alice");
    }

    #[test]
    fn test_legacy_exchange_response() {
        let msg = LegacyExchangeMessage::new_response("abc123", "def456", "Bob");
        assert!(msg.is_response);
    }

    #[test]
    fn test_simple_ack() {
        let ack = create_simple_ack("msg-123", SimpleAckStatus::Delivered);

        match ack.payload {
            SimplePayload::Acknowledgment(a) => {
                assert_eq!(a.message_id, "msg-123");
            }
            _ => panic!("Wrong payload type"),
        }
    }

    #[test]
    fn test_encrypted_update() {
        let update = SimpleEncryptedUpdate {
            recipient_id: "recipient-abc".to_string(),
            sender_id: "sender-xyz".to_string(),
            ciphertext: vec![1, 2, 3, 4, 5],
        };

        let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(update));
        let encoded = encode_simple_message(&envelope).unwrap();
        let decoded = decode_simple_message(&encoded).unwrap();

        match decoded.payload {
            SimplePayload::EncryptedUpdate(u) => {
                assert_eq!(u.recipient_id, "recipient-abc");
                assert_eq!(u.ciphertext, vec![1, 2, 3, 4, 5]);
            }
            _ => panic!("Wrong payload type"),
        }
    }
}
