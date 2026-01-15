//! Wire Protocol
//!
//! Message types that match the relay server's protocol.

use serde::{Deserialize, Serialize};

/// Protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Frame header size (4 bytes for length prefix).
pub const FRAME_HEADER_SIZE: usize = 4;

/// Message envelope wrapping all protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub version: u8,
    pub message_id: String,
    pub timestamp: u64,
    pub payload: MessagePayload,
}

/// Payload types for protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessagePayload {
    EncryptedUpdate(EncryptedUpdate),
    Acknowledgment(Acknowledgment),
    Handshake(Handshake),
    #[serde(other)]
    Unknown,
}

/// Encrypted update sent between users via relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedUpdate {
    pub recipient_id: String,
    pub sender_id: String,
    pub ciphertext: Vec<u8>,
}

/// Acknowledgment for a delivered message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acknowledgment {
    pub message_id: String,
    pub status: AckStatus,
}

/// Acknowledgment status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AckStatus {
    Delivered,
    ReceivedByRecipient,
    Failed,
}

/// Handshake message to identify client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    pub client_id: String,
}

/// Exchange message sent when completing contact exchange.
/// This is sent as the ciphertext in an EncryptedUpdate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeMessage {
    /// Message type identifier.
    pub msg_type: String,
    /// Sender's identity public key (hex encoded).
    pub identity_public_key: String,
    /// Sender's ephemeral X3DH public key (hex encoded).
    pub ephemeral_public_key: String,
    /// Sender's display name.
    pub display_name: String,
}

impl ExchangeMessage {
    /// Creates a new exchange message.
    pub fn new(identity_key: &[u8; 32], ephemeral_key: &[u8; 32], name: &str) -> Self {
        ExchangeMessage {
            msg_type: "exchange".to_string(),
            identity_public_key: hex::encode(identity_key),
            ephemeral_public_key: hex::encode(ephemeral_key),
            display_name: name.to_string(),
        }
    }

    /// Checks if this is an exchange message.
    pub fn is_exchange(data: &[u8]) -> bool {
        if let Ok(msg) = serde_json::from_slice::<ExchangeMessage>(data) {
            msg.msg_type == "exchange"
        } else {
            false
        }
    }

    /// Parses exchange message from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }

    /// Serializes to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// Creates a message envelope.
pub fn create_envelope(payload: MessagePayload) -> MessageEnvelope {
    use std::time::{SystemTime, UNIX_EPOCH};

    MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: uuid::Uuid::new_v4().to_string(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        payload,
    }
}

/// Encodes a message envelope to binary with length prefix.
pub fn encode_message(envelope: &MessageEnvelope) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(envelope).map_err(|e| e.to_string())?;
    let len = json.len() as u32;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);

    Ok(frame)
}

/// Decodes a message envelope from binary with length prefix.
pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, String> {
    if data.len() < FRAME_HEADER_SIZE {
        return Err("Frame too short".to_string());
    }

    let json = &data[FRAME_HEADER_SIZE..];
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

/// Creates an acknowledgment envelope.
pub fn create_ack(message_id: &str, status: AckStatus) -> MessageEnvelope {
    create_envelope(MessagePayload::Acknowledgment(Acknowledgment {
        message_id: message_id.to_string(),
        status,
    }))
}
