//! Simplified Message Types for Relay Communication
//!
//! These types are used by mobile clients, CLI, and relay server for
//! simpler message passing where the full RatchetHeader isn't needed
//! in the wire format (it's embedded in the ciphertext instead).
//!
//! This module provides a common definition to avoid duplication across
//! vauchi-mobile, vauchi-cli, and vauchi-relay.

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
    /// Device-to-device sync message (for inter-device synchronization).
    DeviceSyncMessage(SimpleDeviceSyncMessage),
    /// Acknowledgment for device sync messages.
    DeviceSyncAck(SimpleDeviceSyncAck),
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
    /// Optional device ID for inter-device sync (hex-encoded, 64 chars = 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
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
            .expect("system time before UNIX epoch")
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

/// Device-to-device sync message for synchronizing data between devices of the same identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleDeviceSyncMessage {
    /// User's public identity ID (for routing).
    pub identity_id: String,
    /// Target device ID (hex-encoded, 64 chars = 32 bytes).
    pub target_device_id: String,
    /// Sender device ID (hex-encoded, 64 chars = 32 bytes).
    pub sender_device_id: String,
    /// ECDH-encrypted payload containing SyncItems.
    pub encrypted_payload: Vec<u8>,
    /// Version number for ordering and deduplication.
    pub version: u64,
}

/// Acknowledgment for device sync messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleDeviceSyncAck {
    /// The message_id being acknowledged.
    pub message_id: String,
    /// Version that was synced to.
    pub synced_version: u64,
}

/// Create a device sync message envelope.
pub fn create_device_sync_message(
    identity_id: &str,
    target_device_id: &str,
    sender_device_id: &str,
    encrypted_payload: Vec<u8>,
    version: u64,
) -> SimpleEnvelope {
    create_simple_envelope(SimplePayload::DeviceSyncMessage(SimpleDeviceSyncMessage {
        identity_id: identity_id.to_string(),
        target_device_id: target_device_id.to_string(),
        sender_device_id: sender_device_id.to_string(),
        encrypted_payload,
        version,
    }))
}

/// Create a device sync acknowledgment envelope.
pub fn create_device_sync_ack(message_id: &str, synced_version: u64) -> SimpleEnvelope {
    create_simple_envelope(SimplePayload::DeviceSyncAck(SimpleDeviceSyncAck {
        message_id: message_id.to_string(),
        synced_version,
    }))
}
