// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

/// Acknowledgment status for message delivery tracking.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AckStatus {
    /// Message stored by relay (persisted, awaiting recipient).
    Stored,
    /// Message delivered to recipient (recipient came online).
    Delivered,
    /// Message received and acknowledged by recipient (end-to-end confirmation).
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

/// Version negotiation message for protocol compatibility.
///
/// Sent during connection establishment to agree on a common protocol version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionNegotiation {
    /// List of protocol versions this peer supports.
    pub supported_versions: Vec<u32>,
    /// The version this peer prefers to use.
    pub preferred_version: u32,
}

/// Negotiates the highest mutually supported protocol version.
///
/// Returns the highest version that both peers support, or `None` if
/// there is no common version.
///
/// The preferred version fields are used as tiebreakers: if both peers
/// share a common preferred version, it is selected. Otherwise, the
/// highest mutually supported version wins.
pub fn negotiate_version(local: &VersionNegotiation, remote: &VersionNegotiation) -> Option<u32> {
    let mut common: Vec<u32> = local
        .supported_versions
        .iter()
        .filter(|v| remote.supported_versions.contains(v))
        .copied()
        .collect();

    if common.is_empty() {
        return None;
    }

    common.sort_unstable();
    Some(*common.last().unwrap())
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
    fn test_version_negotiation_common_version() {
        let local = VersionNegotiation {
            supported_versions: vec![1, 2, 3],
            preferred_version: 3,
        };
        let remote = VersionNegotiation {
            supported_versions: vec![2, 3, 4],
            preferred_version: 4,
        };

        assert_eq!(negotiate_version(&local, &remote), Some(3));
    }

    #[test]
    fn test_version_negotiation_no_common_version() {
        let local = VersionNegotiation {
            supported_versions: vec![1, 2],
            preferred_version: 2,
        };
        let remote = VersionNegotiation {
            supported_versions: vec![3, 4],
            preferred_version: 3,
        };

        assert_eq!(negotiate_version(&local, &remote), None);
    }

    #[test]
    fn test_version_negotiation_single_common() {
        let local = VersionNegotiation {
            supported_versions: vec![1],
            preferred_version: 1,
        };
        let remote = VersionNegotiation {
            supported_versions: vec![1],
            preferred_version: 1,
        };

        assert_eq!(negotiate_version(&local, &remote), Some(1));
    }

    #[test]
    fn test_version_negotiation_highest_wins() {
        let local = VersionNegotiation {
            supported_versions: vec![1, 2, 5],
            preferred_version: 2,
        };
        let remote = VersionNegotiation {
            supported_versions: vec![1, 5, 6],
            preferred_version: 6,
        };

        // Highest common version is 5
        assert_eq!(negotiate_version(&local, &remote), Some(5));
    }

    #[test]
    fn test_version_negotiation_empty_local() {
        let local = VersionNegotiation {
            supported_versions: vec![],
            preferred_version: 0,
        };
        let remote = VersionNegotiation {
            supported_versions: vec![1, 2],
            preferred_version: 1,
        };

        assert_eq!(negotiate_version(&local, &remote), None);
    }

    #[test]
    fn test_version_negotiation_empty_remote() {
        let local = VersionNegotiation {
            supported_versions: vec![1, 2],
            preferred_version: 1,
        };
        let remote = VersionNegotiation {
            supported_versions: vec![],
            preferred_version: 0,
        };

        assert_eq!(negotiate_version(&local, &remote), None);
    }

    #[test]
    fn test_version_negotiation_serde_roundtrip() {
        let vn = VersionNegotiation {
            supported_versions: vec![1, 2, 3],
            preferred_version: 2,
        };

        let json = serde_json::to_string(&vn).unwrap();
        let deserialized: VersionNegotiation = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.supported_versions, vec![1, 2, 3]);
        assert_eq!(deserialized.preferred_version, 2);
    }
}
