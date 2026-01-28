// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delta Encoding for Contact Card Updates
//!
//! Provides efficient delta-based updates that only transmit changed fields
//! rather than the entire contact card. Includes signature verification
//! to ensure authenticity of updates.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::contact_card::{ContactCard, ContactField};
use crate::identity::Identity;

/// Delta encoding error types.
#[derive(Error, Debug)]
pub enum DeltaError {
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u32, actual: u32 },

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Field not found: {0}")]
    FieldNotFound(String),

    #[error("Cannot apply change: {0}")]
    ApplyError(String),
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
    /// Ed25519 signature of the delta (64 bytes).
    #[serde(with = "signature_serde")]
    pub signature: [u8; 64],
}

/// Represents a single field change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FieldChange {
    /// A new field was added.
    Added { field: ContactField },
    /// An existing field's value was modified.
    Modified { field_id: String, new_value: String },
    /// A field was removed.
    Removed { field_id: String },
    /// The display name was changed.
    DisplayNameChanged { new_name: String },
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
                    field: (*new_field).clone(),
                });
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        CardDelta {
            version: 1, // Will be set properly during signing
            timestamp: now,
            changes,
            signature: [0u8; 64], // Will be set during signing
        }
    }

    /// Signs the delta with the given identity.
    ///
    /// Creates a signature over the delta content (excluding the signature field).
    pub fn sign(&mut self, identity: &Identity) {
        let message = self.signable_bytes();
        let signature = identity.sign(&message);
        self.signature = *signature.as_bytes();
    }

    /// Verifies the delta signature against a public key.
    pub fn verify(&self, public_key: &[u8; 32]) -> bool {
        use crate::crypto::PublicKey;

        let message = self.signable_bytes();
        let signature = crate::crypto::Signature::from_bytes(self.signature);
        let pubkey = PublicKey::from_bytes(*public_key);

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
            signature: self.signature,
        }
    }

    /// Returns the bytes to be signed/verified.
    fn signable_bytes(&self) -> Vec<u8> {
        // Create a version without the signature for signing
        let signable = SignableDelta {
            version: self.version,
            timestamp: self.timestamp,
            changes: &self.changes,
        };
        serde_json::to_vec(&signable).unwrap_or_default()
    }
}

/// Helper struct for creating signable representation.
#[derive(Serialize)]
struct SignableDelta<'a> {
    version: u32,
    timestamp: u64,
    changes: &'a Vec<FieldChange>,
}

/// Custom serde for fixed-size signature arrays.
mod signature_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    }

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
