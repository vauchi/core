//! Wire protocol for relay communication.
//!
//! This module defines the message format used for communication with
//! the WebBook relay server over WebSocket.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u8 = 1;
pub const FRAME_HEADER_SIZE: usize = 4;

/// Envelope wrapping all relay messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub version: u8,
    pub message_id: String,
    pub timestamp: u64,
    pub payload: MessagePayload,
}

/// Payload types for relay messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessagePayload {
    EncryptedUpdate(EncryptedUpdate),
    Acknowledgment(Acknowledgment),
    Handshake(Handshake),
    #[serde(other)]
    Unknown,
}

/// Encrypted update message containing ciphertext for a recipient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedUpdate {
    pub recipient_id: String,
    pub sender_id: String,
    pub ciphertext: Vec<u8>,
}

/// Acknowledgment of message receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acknowledgment {
    pub message_id: String,
    pub status: AckStatus,
}

/// Status of message acknowledgment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AckStatus {
    Delivered,
    ReceivedByRecipient,
    #[allow(dead_code)]
    Failed,
}

/// Client handshake message for relay registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    pub client_id: String,
}

/// Legacy plaintext exchange message format.
///
/// This format is kept for backward compatibility. New exchanges use
/// `EncryptedExchangeMessage` from webbook-core instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeMessage {
    pub msg_type: String,
    pub identity_public_key: String,
    pub ephemeral_public_key: String,
    pub display_name: String,
    #[serde(default)]
    pub is_response: bool,
}

impl ExchangeMessage {
    /// Check if data is a legacy plaintext exchange message.
    pub fn is_exchange(data: &[u8]) -> bool {
        if let Ok(msg) = serde_json::from_slice::<ExchangeMessage>(data) {
            msg.msg_type == "exchange"
        } else {
            false
        }
    }

    /// Parse from bytes (for backward compatibility with legacy plaintext format).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

/// Create a new message envelope with the given payload.
pub fn create_envelope(payload: MessagePayload) -> MessageEnvelope {
    MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: uuid::Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        payload,
    }
}

/// Encode a message envelope to bytes with length prefix.
pub fn encode_message(envelope: &MessageEnvelope) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(envelope).map_err(|e| e.to_string())?;
    let len = json.len() as u32;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);

    Ok(frame)
}

/// Decode a message envelope from bytes with length prefix.
pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, String> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err("Frame too short".to_string());
    }

    let json = &data[FRAME_HEADER_SIZE..];
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

/// Create an acknowledgment envelope for a message.
pub fn create_ack(message_id: &str, status: AckStatus) -> MessageEnvelope {
    create_envelope(MessagePayload::Acknowledgment(Acknowledgment {
        message_id: message_id.to_string(),
        status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let handshake = Handshake {
            client_id: "test-client".to_string(),
        };
        let envelope = create_envelope(MessagePayload::Handshake(handshake));

        let encoded = encode_message(&envelope).unwrap();
        let decoded = decode_message(&encoded).unwrap();

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.message_id, envelope.message_id);

        match decoded.payload {
            MessagePayload::Handshake(h) => assert_eq!(h.client_id, "test-client"),
            _ => panic!("Wrong payload type"),
        }
    }

    #[test]
    fn test_exchange_message_detection() {
        let exchange = ExchangeMessage {
            msg_type: "exchange".to_string(),
            identity_public_key: "abc".to_string(),
            ephemeral_public_key: "def".to_string(),
            display_name: "Alice".to_string(),
            is_response: false,
        };
        let data = serde_json::to_vec(&exchange).unwrap();

        assert!(ExchangeMessage::is_exchange(&data));
        assert!(!ExchangeMessage::is_exchange(b"not json"));
        assert!(!ExchangeMessage::is_exchange(b"{\"msg_type\":\"other\"}"));
    }
}
