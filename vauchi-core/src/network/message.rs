// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Network Message Types
//!
//! Wire protocol message types for relay communication.

use serde::{Deserialize, Serialize};

use crate::identifiers::{DhPublicKey, IdentityKey};

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
#[non_exhaustive]
pub enum MessagePayload {
    /// Encrypted update message (Double Ratchet encrypted).
    EncryptedUpdate(EncryptedUpdate),
    /// Delivery acknowledgment.
    Acknowledgment(Acknowledgment),
    /// Connection handshake.
    Handshake(Handshake),
    /// Presence/status update.
    Presence(PresenceUpdate),
    /// Identity revocation signal (sent when card owner deletes identity).
    IdentityRevoked(IdentityRevoked),
    /// Identity deletion notification sent to contacts.
    #[serde(alias = "AccountDeletionNotice")]
    IdentityDeletionNotice(IdentityDeletionNotice),
    /// Relay purge request (sent during shred to delete server-side data).
    PurgeRequest(PurgeRequest),
    /// Forwarding hints from the relay indicating blobs stored on other relays.
    ForwardingHints(ForwardingHints),
    /// Client registers mailbox tokens for message delivery routing (SP-33).
    RegisterMailbox(RegisterMailbox),
    /// Client deregisters mailbox tokens (SP-33).
    DeregisterMailbox(DeregisterMailbox),
}

/// Client registers mailbox tokens for message delivery routing.
///
/// Sent after handshake to tell the relay which tokens to route to this
/// connection. Tokens are opaque 64-char hex strings that rotate daily.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMailbox {
    /// Padded batch of 256 hex-encoded mailbox tokens.
    pub tokens: Vec<String>,
}

/// Client deregisters mailbox tokens (e.g., after historical catchup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeregisterMailbox {
    /// Tokens to deregister.
    pub tokens: Vec<String>,
}

/// Identity revocation signal sent to contacts when the card owner deletes their identity.
///
/// NOT Double Ratchet encrypted — signed only — so it can be processed even if
/// the ratchet state is corrupted or missing. The signature is Ed25519 over a
/// canonical byte string (see [`crate::network::revocation::canonical_revocation_bytes`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRevoked {
    /// Owner's public key fingerprint (hex-encoded signing public key).
    pub sender_id: String,
    /// Contact's public key fingerprint (hex-encoded).
    pub recipient_id: String,
    /// Unix timestamp of revocation.
    pub timestamp: u64,
    /// Ed25519 signature over canonical revocation bytes (64 bytes).
    #[serde(with = "bytes_array_64")]
    pub signature: [u8; 64],
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
    #[serde(with = "crate::identifiers::wire_dh_public_key_base64")]
    pub dh_public: DhPublicKey,
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
#[non_exhaustive]
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
    #[serde(with = "crate::identifiers::wire_identity_key_base64")]
    pub identity_public_key: IdentityKey,
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
#[non_exhaustive]
pub enum PresenceStatus {
    Online,
    Away,
    Offline,
}

/// Identity deletion notification sent to contacts.
///
/// Informs contacts that the sender is deleting their identity.
/// Cryptographically signed by the sender's identity key so contacts
/// can verify the notice is authentic (not spoofed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityDeletionNotice {
    /// Current deletion stage.
    pub stage: DeletionStage,
    /// Sender's public signing key (for signature verification).
    #[serde(with = "crate::identifiers::wire_identity_key_base64")]
    pub public_key: IdentityKey,
    /// Unix timestamp when the notice was created.
    pub timestamp: u64,
    /// Ed25519 signature over (public_key || stage || timestamp).
    #[serde(with = "bytes_array_64")]
    pub signature: [u8; 64],
}

/// Relay purge request sent during identity shredding.
///
/// Requests the relay to delete all stored messages and data for this identity.
/// Signed by the identity's Ed25519 key so the relay can verify authenticity.
/// Contains a one-time purge_token for replay prevention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeRequest {
    /// Signing public key (Ed25519, 32 bytes).
    #[serde(with = "crate::identifiers::wire_identity_key_base64")]
    pub public_key: IdentityKey,
    /// Ed25519 signature over (public_key || purge_token || timestamp).
    pub signature: Vec<u8>,
    /// One-time token for replay prevention (32 bytes).
    #[serde(with = "bytes_array_32")]
    pub purge_token: [u8; 32],
    /// Unix timestamp when the request was signed.
    pub timestamp: u64,
}

/// Forwarding hints indicating blobs stored on federated relay peers.
///
/// When a relay offloads blobs to peer relays, it sends forwarding hints
/// to the recipient so they can fetch the blobs from the correct relay.
///
/// ## Signed Hints (Tracker #117)
///
/// When signed by the relay, `relay_signing_key` contains the relay's
/// Ed25519 public key and `signature` contains the Ed25519 signature
/// over the canonical hint data. Clients should verify the signature
/// against a pinned relay public key before following the hints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingHints {
    /// List of forwarding hints.
    pub hints: Vec<ForwardingHint>,
    /// Relay's Ed25519 signing public key (32 bytes, hex-encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_signing_key: Option<String>,
    /// Ed25519 signature over the canonical hint data (hex-encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// A single forwarding hint pointing to a blob on another relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingHint {
    /// The blob ID to fetch.
    pub blob_id: String,
    /// The relay URL where the blob is stored.
    pub relay_url: String,
    /// Unix timestamp when the hint expires.
    pub expires_at_secs: u64,
}

impl ForwardingHints {
    /// Computes the canonical byte representation for signature verification.
    ///
    /// Hints are sorted by `blob_id` to ensure deterministic ordering.
    pub fn canonical_data(&self) -> Vec<u8> {
        let mut sorted_hints: Vec<&ForwardingHint> = self.hints.iter().collect();
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

/// Stages of identity deletion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeletionStage {
    /// Deletion scheduled, grace period active. Can still be cancelled.
    Pending,
    /// Deletion confirmed and executed. Identity is irrecoverably destroyed.
    Confirmed,
    /// Deletion cancelled during grace period.
    Cancelled,
}

/// An emergency alert payload embedded inside an encrypted update.
///
/// When serialized and encrypted inside an `EncryptedUpdate`, this is
/// indistinguishable from a normal card update on the wire. The alert
/// type is only revealed after decryption by the recipient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyAlert {
    /// Sender's public key fingerprint (contact ID).
    pub sender_id: String,
    /// Alert message text.
    pub message: String,
    /// Unix timestamp when the alert was created.
    pub timestamp: u64,
    /// Optional geographic location of the sender.
    pub location: Option<GeoLocation>,
}

/// Geographic location with optional accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    /// Latitude in decimal degrees.
    pub latitude: f64,
    /// Longitude in decimal degrees.
    pub longitude: f64,
    /// Location accuracy in meters.
    pub accuracy_meters: Option<f32>,
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
    common.last().copied()
}

impl IdentityRevoked {
    /// Creates and signs a revocation message.
    pub fn create(
        identity: &crate::identity::Identity,
        recipient_id: &str,
        timestamp: u64,
    ) -> Self {
        let sender_id = identity.public_id();

        // Decode hex IDs to raw bytes for canonical signature.
        // Exchanged contacts have hex-encoded 32-byte public key IDs.
        // Imported contacts have UUID IDs that aren't valid hex — for them
        // the signature covers zeros, producing a message that `verify()`
        // will reject. This is harmless: imported contacts don't participate
        // in the relay protocol, so no one processes their revocations.
        let sender_bytes: [u8; 32] = *identity.signing_public_key();
        let recipient_bytes: [u8; 32] = decode_hex_id(recipient_id).unwrap_or([0u8; 32]);

        let canonical = super::revocation::canonical_revocation_bytes(
            &sender_bytes,
            &recipient_bytes,
            timestamp,
        );
        let signature = identity.sign(&canonical);

        IdentityRevoked {
            sender_id,
            recipient_id: recipient_id.to_string(),
            timestamp,
            signature: *signature.as_bytes(),
        }
    }

    /// Verifies the revocation signature against the given public key.
    pub fn verify(&self, public_key: &[u8; 32]) -> bool {
        // Reject malformed recipient IDs instead of silently verifying against zeros.
        let Some(recipient_bytes) = decode_hex_id(&self.recipient_id) else {
            return false;
        };

        let canonical = super::revocation::canonical_revocation_bytes(
            public_key,
            &recipient_bytes,
            self.timestamp,
        );

        let pubkey = crate::crypto::PublicKey::from_bytes(*public_key);
        let signature = crate::crypto::Signature::from_bytes(self.signature);
        pubkey.verify(&canonical, &signature)
    }
}

/// Decode a hex-encoded 32-byte public key fingerprint to raw bytes.
/// Returns `None` if the hex is invalid or not exactly 32 bytes.
fn decode_hex_id(hex_str: &str) -> Option<[u8; 32]> {
    let decoded = hex::decode(hex_str).ok()?;
    <[u8; 32]>::try_from(decoded.as_slice()).ok()
}

/// Serde helper for 32-byte arrays.
mod bytes_array_32 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a 32-byte array to a base64-encoded string for network message transmission.
    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    /// Deserializes a 32-byte array from a base64-encoded string.
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

    /// Serializes a 64-byte array to a base64-encoded string for network message transmission.
    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    /// Deserializes a 64-byte array from a base64-encoded string.
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

// INLINE_TEST_REQUIRED: serde roundtrip tests need private access to byte-array helper modules
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
    fn test_identity_deletion_notice_serde_roundtrip() {
        let notice = IdentityDeletionNotice {
            stage: DeletionStage::Pending,
            public_key: IdentityKey::from_bytes([0x42; 32]),
            timestamp: 1700000000,
            signature: [0xAB; 64],
        };

        let json = serde_json::to_string(&notice).unwrap();
        let deserialized: IdentityDeletionNotice = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.stage, DeletionStage::Pending);
        assert_eq!(deserialized.public_key, [0x42; 32]);
        assert_eq!(deserialized.timestamp, 1700000000);
        assert_eq!(deserialized.signature, [0xAB; 64]);
    }

    #[test]
    fn test_deletion_stage_all_variants_serialize() {
        for stage in [
            DeletionStage::Pending,
            DeletionStage::Confirmed,
            DeletionStage::Cancelled,
        ] {
            let json = serde_json::to_string(&stage).unwrap();
            let deserialized: DeletionStage = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, stage);
        }
    }

    #[test]
    fn test_identity_deletion_notice_in_payload() {
        let notice = IdentityDeletionNotice {
            stage: DeletionStage::Confirmed,
            public_key: IdentityKey::from_bytes([0x01; 32]),
            timestamp: 1700000000,
            signature: [0x02; 64],
        };

        let payload = MessagePayload::IdentityDeletionNotice(notice);
        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: MessagePayload = serde_json::from_str(&json).unwrap();

        match deserialized {
            MessagePayload::IdentityDeletionNotice(n) => {
                assert_eq!(n.stage, DeletionStage::Confirmed);
                assert_eq!(n.public_key, [0x01; 32]);
            }
            _ => panic!("Expected IdentityDeletionNotice variant"),
        }
    }

    #[test]
    fn test_identity_deletion_notice_in_envelope() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-id-123".to_string(),
            timestamp: 1700000000,
            payload: MessagePayload::IdentityDeletionNotice(IdentityDeletionNotice {
                stage: DeletionStage::Pending,
                public_key: IdentityKey::from_bytes([0xFF; 32]),
                timestamp: 1700000000,
                signature: [0xEE; 64],
            }),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let deserialized: MessageEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, PROTOCOL_VERSION);
        assert_eq!(deserialized.message_id, "test-id-123");
        match deserialized.payload {
            MessagePayload::IdentityDeletionNotice(n) => {
                assert_eq!(n.stage, DeletionStage::Pending);
            }
            _ => panic!("Expected IdentityDeletionNotice"),
        }
    }

    #[test]
    fn test_purge_request_serde_roundtrip() {
        let request = PurgeRequest {
            public_key: IdentityKey::from_bytes([0x42; 32]),
            signature: vec![0xAB; 64],
            purge_token: [0xCD; 32],
            timestamp: 1700000000,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: PurgeRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.public_key, [0x42; 32]);
        assert_eq!(deserialized.signature, vec![0xAB; 64]);
        assert_eq!(deserialized.purge_token, [0xCD; 32]);
        assert_eq!(deserialized.timestamp, 1700000000);
    }

    #[test]
    fn test_purge_request_in_payload() {
        let request = PurgeRequest {
            public_key: IdentityKey::from_bytes([0x01; 32]),
            signature: vec![0x02; 64],
            purge_token: [0x03; 32],
            timestamp: 1700000000,
        };

        let payload = MessagePayload::PurgeRequest(request);
        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: MessagePayload = serde_json::from_str(&json).unwrap();

        match deserialized {
            MessagePayload::PurgeRequest(r) => {
                assert_eq!(r.public_key, [0x01; 32]);
                assert_eq!(r.purge_token, [0x03; 32]);
            }
            _ => panic!("Expected PurgeRequest variant"),
        }
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
