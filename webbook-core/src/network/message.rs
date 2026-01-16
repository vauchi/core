//! Network Message Types
//!
//! Wire protocol message types for relay communication.

use serde::{Deserialize, Serialize};

/// Unique message identifier for deduplication and acknowledgments.
pub type MessageId = String;

/// Wire protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Envelope wrapping all messages on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Protocol version for compatibility checking.
    pub version: u8,
    /// Unique message ID (UUID v4).
    pub message_id: MessageId,
    /// Unix timestamp when message was created.
    pub timestamp: u64,
    /// The actual message content.
    pub payload: MessagePayload,
}

/// Types of messages that can be sent over the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePayload {
    /// Encrypted update message (Double Ratchet encrypted).
    EncryptedUpdate(EncryptedUpdate),
    /// Delivery acknowledgment.
    Acknowledgment(Acknowledgment),
    /// Connection handshake.
    Handshake(Handshake),
    /// Presence/status update.
    Presence(PresenceUpdate),
    /// Device-to-device sync message (between own devices).
    DeviceSync(DeviceSyncMessage),
}

/// An encrypted update destined for a specific recipient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedUpdate {
    /// Recipient's public key fingerprint (contact ID).
    pub recipient_id: String,
    /// Sender's public key fingerprint.
    pub sender_id: String,
    /// Double Ratchet message header.
    pub ratchet_header: RatchetHeader,
    /// The encrypted payload (CardDelta or other update).
    pub ciphertext: Vec<u8>,
}

/// Ratchet header for wire serialization.
///
/// Contains the public key and chain indices needed for decryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetHeader {
    /// Sender's current DH public key.
    #[serde(with = "bytes_array_32")]
    pub dh_public: [u8; 32],
    /// DH ratchet generation.
    pub dh_generation: u32,
    /// Message index within the chain.
    pub message_index: u32,
    /// Previous chain length (for skipped message handling).
    pub previous_chain_length: u32,
}

/// Delivery acknowledgment message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acknowledgment {
    /// ID of the message being acknowledged.
    pub message_id: MessageId,
    /// Status of delivery.
    pub status: AckStatus,
    /// Optional error message if delivery failed.
    pub error: Option<String>,
}

/// Acknowledgment status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AckStatus {
    /// Message delivered to relay successfully.
    Delivered,
    /// Message delivered to recipient (end-to-end ack).
    ReceivedByRecipient,
    /// Delivery failed.
    Failed,
}

/// Connection handshake message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    /// Client's identity public key.
    #[serde(with = "bytes_array_32")]
    pub identity_public_key: [u8; 32],
    /// Nonce for this session.
    #[serde(with = "bytes_array_32")]
    pub nonce: [u8; 32],
    /// Signature over (nonce || timestamp) proving identity ownership.
    #[serde(with = "bytes_array_64")]
    pub signature: [u8; 64],
}

/// Presence/status update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUpdate {
    /// Current presence status.
    pub status: PresenceStatus,
    /// Optional status message.
    pub message: Option<String>,
}

/// Presence status values.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PresenceStatus {
    Online,
    Away,
    Offline,
}

/// Device-to-device sync message for inter-device synchronization.
///
/// Used for syncing data between devices belonging to the same identity.
/// The payload is encrypted using the target device's exchange key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncMessage {
    /// Target device ID (one of our own devices).
    #[serde(with = "bytes_array_32")]
    pub target_device_id: [u8; 32],
    /// Sender device ID.
    #[serde(with = "bytes_array_32")]
    pub sender_device_id: [u8; 32],
    /// Encrypted sync payload (SyncItems encrypted with device exchange key).
    pub ciphertext: Vec<u8>,
    /// Nonce for AES-GCM decryption.
    #[serde(with = "bytes_array_12")]
    pub nonce: [u8; 12],
    /// Sync version number for ordering/deduplication.
    pub sync_version: u64,
}

/// Serde helper for 32-byte arrays.
mod bytes_array_32 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 32-byte array"))
    }
}

/// Serde helper for 64-byte arrays.
mod bytes_array_64 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 64-byte array"))
    }
}

/// Serde helper for 12-byte arrays (AES-GCM nonce).
mod bytes_array_12 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 12], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 12], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 12-byte array"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_envelope_serialize_roundtrip() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-id-123".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::Presence(PresenceUpdate {
                status: PresenceStatus::Online,
                message: Some("Hello".to_string()),
            }),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let restored: MessageEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.version, envelope.version);
        assert_eq!(restored.message_id, envelope.message_id);
        assert_eq!(restored.timestamp, envelope.timestamp);
    }

    #[test]
    fn test_encrypted_update_serialize() {
        let update = EncryptedUpdate {
            recipient_id: "recipient-123".to_string(),
            sender_id: "sender-456".to_string(),
            ratchet_header: RatchetHeader {
                dh_public: [1u8; 32],
                dh_generation: 5,
                message_index: 10,
                previous_chain_length: 3,
            },
            ciphertext: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let json = serde_json::to_string(&update).unwrap();
        let restored: EncryptedUpdate = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.recipient_id, update.recipient_id);
        assert_eq!(restored.sender_id, update.sender_id);
        assert_eq!(
            restored.ratchet_header.dh_public,
            update.ratchet_header.dh_public
        );
        assert_eq!(restored.ciphertext, update.ciphertext);
    }

    #[test]
    fn test_acknowledgment_serialize() {
        let ack = Acknowledgment {
            message_id: "msg-123".to_string(),
            status: AckStatus::Delivered,
            error: None,
        };

        let json = serde_json::to_string(&ack).unwrap();
        let restored: Acknowledgment = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.message_id, ack.message_id);
        assert_eq!(restored.status, AckStatus::Delivered);
    }

    #[test]
    fn test_handshake_signature_bytes() {
        let handshake = Handshake {
            identity_public_key: [2u8; 32],
            nonce: [3u8; 32],
            signature: [4u8; 64],
        };

        let json = serde_json::to_string(&handshake).unwrap();
        let restored: Handshake = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.identity_public_key, handshake.identity_public_key);
        assert_eq!(restored.nonce, handshake.nonce);
        assert_eq!(restored.signature, handshake.signature);
    }

    #[test]
    fn test_ack_status_values() {
        assert_ne!(AckStatus::Delivered, AckStatus::Failed);
        assert_ne!(AckStatus::ReceivedByRecipient, AckStatus::Failed);
    }

    #[test]
    fn test_presence_status_values() {
        assert_ne!(PresenceStatus::Online, PresenceStatus::Offline);
        assert_ne!(PresenceStatus::Away, PresenceStatus::Online);
    }

    // ============================================================
    // Phase 2: Device Sync Message Tests (TDD)
    // Based on features/device_management.feature @sync scenarios
    // ============================================================

    /// Test DeviceSyncMessage serialization for inter-device communication
    #[test]
    fn test_device_sync_message_serialize() {
        let msg = DeviceSyncMessage {
            target_device_id: [0x41u8; 32],
            sender_device_id: [0x42u8; 32],
            ciphertext: vec![0xDE, 0xAD, 0xBE, 0xEF],
            nonce: [0x55u8; 12],
            sync_version: 42,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let restored: DeviceSyncMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.target_device_id, msg.target_device_id);
        assert_eq!(restored.sender_device_id, msg.sender_device_id);
        assert_eq!(restored.ciphertext, msg.ciphertext);
        assert_eq!(restored.nonce, msg.nonce);
        assert_eq!(restored.sync_version, msg.sync_version);
    }

    /// Test DeviceSyncMessage in MessagePayload envelope
    #[test]
    fn test_device_sync_message_in_envelope() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "device-sync-123".to_string(),
            timestamp: 1234567890,
            payload: MessagePayload::DeviceSync(DeviceSyncMessage {
                target_device_id: [0x41u8; 32],
                sender_device_id: [0x42u8; 32],
                ciphertext: vec![1, 2, 3, 4],
                nonce: [0x55u8; 12],
                sync_version: 1,
            }),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let restored: MessageEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.message_id, envelope.message_id);
        if let MessagePayload::DeviceSync(msg) = restored.payload {
            assert_eq!(msg.target_device_id, [0x41u8; 32]);
            assert_eq!(msg.sync_version, 1);
        } else {
            panic!("Expected DeviceSync payload");
        }
    }
}
