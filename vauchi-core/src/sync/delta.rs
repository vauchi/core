// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delta Encoding for Contact Card Updates
//!
//! Provides efficient delta-based updates that only transmit changed fields
//! rather than the entire contact card. Includes signature verification
//! to ensure authenticity of updates.
//!
//! ## Version-Tagged Payloads
//!
//! The inner payload (before Double Ratchet encryption) uses a version byte prefix:
//! - `0x02`: CEK-wrapped payload (`CekWrappedPayload`)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

use crate::contact_card::{ContactCard, ContactField};
use crate::identity::Identity;

/// Aggregated validation count for a single field in a card update.
///
/// Embedded in card deltas so recipients can see how many people
/// verified a field, without revealing validator identities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationSummary {
    /// Number of validators who confirmed this field value.
    pub count: u32,
    /// Human-readable trust level derived from the count (e.g. "none", "unverified", "verified").
    pub trust_level: String,
}

/// Version byte for CEK-wrapped payloads (crypto-shredding enabled).
pub const PAYLOAD_VERSION_CEK: u8 = 0x02;
/// Version byte for reciprocity confirmation payload.
pub const PAYLOAD_VERSION_RECIPROCITY: u8 = 0x03;

/// Delta encoding error types.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum DeltaError {
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u32, actual: u32 },

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Field not found: {0}")]
    FieldNotFound(String),

    #[error("Cannot apply change: {0}")]
    ApplyError(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Unknown payload version: 0x{0:02X}")]
    UnknownPayloadVersion(u8),

    #[error("Payload too short")]
    PayloadTooShort,

    #[error("CEK payload decode error: {0}")]
    CekDecodeError(String),

    #[error("invalid payload: {0}")]
    InvalidPayload(String),
}

/// A delta update containing only changed fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardDelta {
    /// Version number for ordering updates.
    pub version: u32,
    /// Timestamp when the delta was created.
    pub timestamp: u64,
    /// List of field changes.
    pub changes: Vec<FieldChange>,
    /// Random nonce for replay attack detection (32 bytes).
    /// Defaults to all zeros when deserializing legacy deltas without a nonce.
    #[serde(default = "default_nonce", with = "nonce_serde")]
    pub nonce: [u8; 32],
    /// Ed25519 signature of the delta (64 bytes).
    #[serde(with = "signature_serde")]
    pub signature: [u8; 64],
    /// Per-field validation summaries (counts only, privacy-preserving).
    /// Optional for backward compatibility with older clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_summary: Option<HashMap<String, ValidationSummary>>,
}

/// Represents a single field change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum FieldChange {
    /// A new field was added.
    ///
    /// INVARIANT: `field` must be the result of `ContactField::strip_private()`.
    /// Private annotations (e.g., `note`) must never appear in outbound deltas.
    Added { field: ContactField },
    /// An existing field's value was modified.
    Modified { field_id: String, new_value: String },
    /// A field was removed.
    Removed { field_id: String },
    /// The display name was changed.
    DisplayNameChanged { new_name: String },
}

/// Returns a zero nonce for deserializing legacy deltas without a nonce field.
fn default_nonce() -> [u8; 32] {
    [0u8; 32]
}

impl CardDelta {
    /// Computes the delta between two card states.
    ///
    /// Returns a delta containing all changes needed to transform
    /// `old` into `new`.
    pub fn compute(old: &ContactCard, new: &ContactCard) -> Self {
        let mut changes = Vec::new();

        // Check display name change
        if old.display_name() != new.display_name() {
            changes.push(FieldChange::DisplayNameChanged {
                new_name: new.display_name().to_string(),
            });
        }

        // Build lookup map for old fields
        let old_fields: std::collections::HashMap<&str, &ContactField> =
            old.fields().iter().map(|f| (f.id(), f)).collect();

        // Build lookup map for new fields
        let new_fields: std::collections::HashMap<&str, &ContactField> =
            new.fields().iter().map(|f| (f.id(), f)).collect();

        // Check for modified or removed fields
        for (id, old_field) in &old_fields {
            match new_fields.get(id) {
                Some(new_field) => {
                    // Field exists in both - check if modified
                    if old_field.value() != new_field.value() {
                        changes.push(FieldChange::Modified {
                            field_id: id.to_string(),
                            new_value: new_field.value().to_string(),
                        });
                    }
                }
                None => {
                    // Field was removed
                    changes.push(FieldChange::Removed {
                        field_id: id.to_string(),
                    });
                }
            }
        }

        // Check for added fields
        for (id, new_field) in &new_fields {
            if !old_fields.contains_key(id) {
                changes.push(FieldChange::Added {
                    field: new_field.strip_private(),
                });
            }
        }

        let now = crate::clock::ambient_now_secs();

        // Generate random nonce for replay detection
        let nonce: [u8; 32] = crate::crypto::random_bytes();

        CardDelta {
            version: 1, // Default; callers should set via set_version() before signing
            timestamp: now,
            changes,
            nonce,
            signature: [0u8; 64], // Will be set during signing
            validation_summary: None,
        }
    }

    /// Sets the version number (#42). Must be called before `sign()` because
    /// the version is included in the signed payload.
    pub fn set_version(&mut self, version: u32) {
        self.version = version;
    }

    /// Signs the delta with the given identity, binding to the recipient.
    ///
    /// The signature covers the delta content plus the sender and recipient
    /// identity keys. This prevents a signed delta from being forwarded to
    /// a different recipient with a valid signature.
    pub fn sign(&mut self, identity: &Identity, recipient_pk: &[u8; 32]) {
        let message = self.signable_bytes(identity.signing_public_key(), recipient_pk);
        let signature = identity.sign(&message);
        self.signature = *signature.as_bytes();
    }

    /// Verifies the delta signature against sender and recipient public keys.
    ///
    /// Both keys must match the values used during signing for the signature
    /// to verify. This binds the delta to a specific sender-recipient pair.
    pub fn verify(&self, sender_pk: &[u8; 32], recipient_pk: &[u8; 32]) -> bool {
        use crate::crypto::PublicKey;

        let message = self.signable_bytes(sender_pk, recipient_pk);
        let signature = crate::crypto::Signature::from_bytes(self.signature);
        let pubkey = PublicKey::from_bytes(*sender_pk);

        pubkey.verify(&message, &signature)
    }

    /// Applies this delta to a contact card.
    ///
    /// Modifies the card in place to reflect all changes in the delta.
    pub fn apply(&self, card: &mut ContactCard) -> Result<(), DeltaError> {
        for change in &self.changes {
            match change {
                FieldChange::DisplayNameChanged { new_name } => {
                    card.set_display_name(new_name)
                        .map_err(|e| DeltaError::ApplyError(e.to_string()))?;
                }
                FieldChange::Added { field } => {
                    card.add_field(field.clone())
                        .map_err(|e| DeltaError::ApplyError(e.to_string()))?;
                }
                FieldChange::Modified {
                    field_id,
                    new_value,
                } => {
                    let found = card.fields_mut().iter_mut().find(|f| f.id() == field_id);

                    match found {
                        Some(field) => {
                            field.set_value(new_value);
                        }
                        None => {
                            return Err(DeltaError::FieldNotFound(field_id.clone()));
                        }
                    }
                }
                FieldChange::Removed { field_id } => {
                    // Ignore errors for removal - field might already be removed
                    let _ = card.remove_field(field_id);
                }
            }
        }

        Ok(())
    }

    /// Returns true if this delta contains no changes.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns a list of descriptive labels for the changes in this delta.
    pub fn changed_fields(&self) -> Vec<String> {
        self.changes
            .iter()
            .map(|change| match change {
                FieldChange::Added { field } => field.label().to_string(),
                FieldChange::Modified { field_id, .. } => field_id.clone(),
                FieldChange::Removed { field_id } => format!("{} (removed)", field_id),
                FieldChange::DisplayNameChanged { new_name } => format!("name: {}", new_name),
            })
            .collect()
    }

    /// Filters this delta based on visibility rules for a specific contact.
    ///
    /// Returns a new delta containing only the changes that the contact
    /// is allowed to see according to the visibility rules.
    pub fn filter_for_contact(
        &self,
        contact_id: &str,
        rules: &crate::contact::VisibilityRules,
    ) -> Self {
        let filtered_changes: Vec<FieldChange> = self
            .changes
            .iter()
            .filter(|change| {
                match change {
                    // Display name changes are always visible
                    FieldChange::DisplayNameChanged { .. } => true,
                    // For field changes, check visibility rules
                    FieldChange::Added { field } => rules.can_see(field.id(), contact_id),
                    FieldChange::Modified { field_id, .. } => rules.can_see(field_id, contact_id),
                    FieldChange::Removed { field_id } => rules.can_see(field_id, contact_id),
                }
            })
            .cloned()
            .collect();

        CardDelta {
            version: self.version,
            timestamp: self.timestamp,
            changes: filtered_changes,
            nonce: self.nonce,
            signature: self.signature,
            validation_summary: self.validation_summary.clone(),
        }
    }

    /// Filters this delta using a custom visibility predicate.
    ///
    /// Returns a new delta containing only the changes where `can_see(field_id)`
    /// returns true. Display name changes are always included.
    ///
    /// This is useful when visibility rules come from multiple sources (labels,
    /// per-contact overrides, default rules) and the caller has already resolved
    /// the effective visibility.
    pub fn filter_with<F: Fn(&str) -> bool>(&self, can_see: F) -> Self {
        let filtered_changes: Vec<FieldChange> = self
            .changes
            .iter()
            .filter(|change| match change {
                FieldChange::DisplayNameChanged { .. } => true,
                FieldChange::Added { field } => can_see(field.id()),
                FieldChange::Modified { field_id, .. } => can_see(field_id),
                FieldChange::Removed { field_id } => can_see(field_id),
            })
            .cloned()
            .collect();

        CardDelta {
            version: self.version,
            timestamp: self.timestamp,
            changes: filtered_changes,
            nonce: self.nonce,
            signature: self.signature,
            validation_summary: self.validation_summary.clone(),
        }
    }

    /// Compresses a payload using DEFLATE compression.
    ///
    /// Useful for reducing the size of delta payloads before transmission.
    pub fn compress_payload(payload: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Write;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(payload)
            .expect("Writing to Vec should not fail");
        encoder.finish().expect("Finishing deflate should not fail")
    }

    /// Maximum decompressed payload size (10 MB).
    ///
    /// Limits decompression output to prevent zip-bomb denial-of-service attacks
    /// where a small compressed payload expands to exhaust memory.
    const MAX_DECOMPRESSED_SIZE: u64 = 10 * 1024 * 1024;

    /// Decompresses a DEFLATE-compressed payload.
    ///
    /// Returns the decompressed bytes, or an error if the data is malformed
    /// or exceeds `Self::MAX_DECOMPRESSED_SIZE`.
    pub fn decompress_payload(compressed: &[u8]) -> Result<Vec<u8>, DeltaError> {
        use flate2::read::DeflateDecoder;
        use std::io::Read;

        let decoder = DeflateDecoder::new(compressed);
        let mut limited = decoder.take(Self::MAX_DECOMPRESSED_SIZE + 1);
        let mut decompressed = Vec::new();
        limited
            .read_to_end(&mut decompressed)
            .map_err(|e| DeltaError::CompressionError(e.to_string()))?;

        if decompressed.len() as u64 > Self::MAX_DECOMPRESSED_SIZE {
            return Err(DeltaError::CompressionError(format!(
                "decompressed payload exceeds maximum size of {} bytes",
                Self::MAX_DECOMPRESSED_SIZE
            )));
        }

        Ok(decompressed)
    }

    /// Returns the bytes to be signed/verified.
    ///
    /// Includes sender and recipient identity keys to bind the signature
    /// to a specific sender-recipient pair.
    fn signable_bytes(&self, sender_pk: &[u8; 32], recipient_pk: &[u8; 32]) -> Vec<u8> {
        let signable = SignableDelta {
            domain: "vauchi-delta-v2",
            version: self.version,
            timestamp: self.timestamp,
            changes: &self.changes,
            nonce: &self.nonce,
            sender_pk,
            recipient_pk,
        };
        serde_json::to_vec(&signable).expect("SignableDelta serialization is infallible")
    }
}

/// Helper struct for creating signable representation.
///
/// Includes domain separator and identity keys to prevent
/// cross-context signature misuse.
#[derive(Serialize)]
struct SignableDelta<'a> {
    domain: &'static str,
    version: u32,
    timestamp: u64,
    changes: &'a Vec<FieldChange>,
    nonce: &'a [u8; 32],
    sender_pk: &'a [u8; 32],
    recipient_pk: &'a [u8; 32],
}

// =============================================================================
// CEK-Wrapped Payload (Version 0x02)
// =============================================================================

/// Payload encrypted by Double Ratchet, containing CEK + CEK-encrypted card delta.
///
/// The CEK (Content Encryption Key) is rotated with each card update.
/// The recipient stores the CEK to control at-rest readability of the card.
/// Destroying the CEK renders the card permanently unreadable (crypto-shredding).
#[derive(Debug, Clone)]
pub struct CekWrappedPayload {
    /// Current CEK for this relationship (rotated each update). 32 raw bytes.
    pub cek: [u8; 32],
    /// Card delta encrypted with the CEK (XChaCha20-Poly1305).
    pub cek_ciphertext: Vec<u8>,
    /// Ed25519 signature over the plaintext delta.
    pub signature: [u8; 64],
    /// Nonce for replay detection.
    pub nonce: [u8; 32],
}

impl CekWrappedPayload {
    /// Encode this payload to bytes (without version prefix).
    ///
    /// Wire format: `cek(32) || signature(64) || nonce(32) || cek_ciphertext_len(4 BE) || cek_ciphertext`
    pub fn encode(&self) -> Vec<u8> {
        let ct_len = (self.cek_ciphertext.len() as u32).to_be_bytes();
        let mut buf = Vec::with_capacity(32 + 64 + 32 + 4 + self.cek_ciphertext.len());
        buf.extend_from_slice(&self.cek);
        buf.extend_from_slice(&self.signature);
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&ct_len);
        buf.extend_from_slice(&self.cek_ciphertext);
        buf
    }

    /// Decode from bytes (without version prefix).
    pub fn decode(data: &[u8]) -> Result<Self, DeltaError> {
        // Minimum: 32 (cek) + 64 (sig) + 32 (nonce) + 4 (len) = 132 bytes
        if data.len() < 132 {
            return Err(DeltaError::CekDecodeError(format!(
                "payload too short: {} bytes, need at least 132",
                data.len()
            )));
        }

        let cek: [u8; 32] = data[0..32]
            .try_into()
            .map_err(|_| DeltaError::CekDecodeError("invalid CEK length".into()))?;
        let signature: [u8; 64] = data[32..96]
            .try_into()
            .map_err(|_| DeltaError::CekDecodeError("invalid signature length".into()))?;
        let nonce: [u8; 32] = data[96..128]
            .try_into()
            .map_err(|_| DeltaError::CekDecodeError("invalid nonce length".into()))?;

        let ct_len =
            u32::from_be_bytes(data[128..132].try_into().map_err(|_| {
                DeltaError::CekDecodeError("invalid ciphertext length field".into())
            })?) as usize;

        if data.len() < 132 + ct_len {
            return Err(DeltaError::CekDecodeError(format!(
                "ciphertext truncated: expected {} bytes, have {}",
                ct_len,
                data.len() - 132
            )));
        }

        let cek_ciphertext = data[132..132 + ct_len].to_vec();

        Ok(CekWrappedPayload {
            cek,
            cek_ciphertext,
            signature,
            nonce,
        })
    }
}

// =============================================================================
// Version-Tagged Payload Envelope
// =============================================================================

/// Version-tagged payload decoded from the inner Double Ratchet plaintext.
///
/// The first byte determines the format:
/// - `0x02`: CEK-wrapped payload (remaining bytes are `CekWrappedPayload`)
#[derive(Debug)]
#[non_exhaustive]
pub enum VersionedPayload {
    /// CEK-wrapped format: contains rotated CEK + CEK-encrypted delta (version 0x02).
    CekWrapped(CekWrappedPayload),
    /// Reciprocity confirmation (version 0x03): token + Ed25519 signature.
    ReciprocityConfirm(ReciprocityConfirmPayload),
}

impl VersionedPayload {
    /// Encode a CEK-wrapped payload with version prefix.
    pub fn encode_cek(payload: &CekWrappedPayload) -> Vec<u8> {
        let inner = payload.encode();
        let mut buf = Vec::with_capacity(1 + inner.len());
        buf.push(PAYLOAD_VERSION_CEK);
        buf.extend_from_slice(&inner);
        buf
    }

    /// Encode a reciprocity confirmation with version prefix.
    pub fn encode_reciprocity(payload: &ReciprocityConfirmPayload) -> Vec<u8> {
        let inner = payload.encode();
        let mut buf = Vec::with_capacity(1 + inner.len());
        buf.push(PAYLOAD_VERSION_RECIPROCITY);
        buf.extend_from_slice(&inner);
        buf
    }

    /// Decode a version-tagged payload.
    pub fn decode(data: &[u8]) -> Result<Self, DeltaError> {
        if data.is_empty() {
            return Err(DeltaError::PayloadTooShort);
        }

        match data[0] {
            PAYLOAD_VERSION_CEK => {
                let payload = CekWrappedPayload::decode(&data[1..])?;
                Ok(VersionedPayload::CekWrapped(payload))
            }
            PAYLOAD_VERSION_RECIPROCITY => {
                let payload = ReciprocityConfirmPayload::decode(&data[1..])?;
                Ok(VersionedPayload::ReciprocityConfirm(payload))
            }
            v => Err(DeltaError::UnknownPayloadVersion(v)),
        }
    }
}

// =============================================================================
// Reciprocity Confirmation Payload (v0x03)
// =============================================================================

const RECIPROCITY_DOMAIN: &[u8] = b"vauchi-reciprocity-confirm-v1";

/// Reciprocity confirmation payload (version byte 0x03).
///
/// Wire format: `0x03 || token(32) || signature(64)` = 97 bytes.
/// Signature covers: `domain(29) || sender_pk(32) || recipient_pk(32) || token(32)` = 125 bytes.
#[derive(Debug)]
pub struct ReciprocityConfirmPayload {
    token: [u8; 32],
    signature: [u8; 64],
}

impl ReciprocityConfirmPayload {
    /// Create and sign a new reciprocity confirmation.
    pub fn new(
        token: [u8; 32],
        identity: &crate::identity::Identity,
        recipient_pk: &[u8; 32],
    ) -> Self {
        let message =
            Self::build_signed_message(&token, identity.signing_public_key(), recipient_pk);
        let sig = identity.sign(&message);
        let signature = *sig.as_bytes();
        Self { token, signature }
    }

    /// Encode to wire format (without version byte prefix).
    ///
    /// Use `VersionedPayload::encode_reciprocity()` to get the full
    /// wire format with the 0x03 prefix.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(96);
        buf.extend_from_slice(&self.token);
        buf.extend_from_slice(&self.signature);
        buf
    }

    /// Decode from wire format (after version byte has been stripped).
    pub fn decode(data: &[u8]) -> Result<Self, DeltaError> {
        if data.len() != 96 {
            return Err(DeltaError::InvalidPayload(format!(
                "reciprocity confirm: expected 96 bytes, got {}",
                data.len()
            )));
        }
        let mut token = [0u8; 32];
        token.copy_from_slice(&data[..32]);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&data[32..96]);
        Ok(Self { token, signature })
    }

    /// Returns the confirmation token.
    pub fn token(&self) -> &[u8; 32] {
        &self.token
    }

    /// Verify the Ed25519 signature against sender and recipient public keys.
    pub fn verify(&self, sender_pk: &[u8; 32], recipient_pk: &[u8; 32]) -> bool {
        use crate::crypto::signing::{PublicKey, Signature};
        let message = Self::build_signed_message(&self.token, sender_pk, recipient_pk);
        let pk = PublicKey::from_bytes(*sender_pk);
        let sig = Signature::from_bytes(self.signature);
        pk.verify(&message, &sig)
    }

    fn build_signed_message(
        token: &[u8; 32],
        sender_pk: &[u8; 32],
        recipient_pk: &[u8; 32],
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(125);
        msg.extend_from_slice(RECIPROCITY_DOMAIN);
        msg.extend_from_slice(sender_pk);
        msg.extend_from_slice(recipient_pk);
        msg.extend_from_slice(token);
        msg
    }
}

/// Custom serde for 32-byte nonce arrays.
mod nonce_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a 32-byte nonce to a base64-encoded string for sync delta payloads.
    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    }

    /// Deserializes a 32-byte nonce from a base64-encoded string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid nonce length"))
    }
}

/// Custom serde for fixed-size signature arrays.
mod signature_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a 64-byte signature to a base64-encoded string for sync delta integrity.
    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    }

    /// Deserializes a 64-byte signature from a base64-encoded string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid signature length"))
    }
}
