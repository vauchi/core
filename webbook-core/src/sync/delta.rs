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
            .unwrap()
            .as_secs();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::FieldType;

    #[test]
    fn test_delta_compute_no_changes() {
        let card = ContactCard::new("Alice");
        let delta = CardDelta::compute(&card, &card);

        assert!(delta.is_empty());
    }

    #[test]
    fn test_delta_compute_display_name_change() {
        let old = ContactCard::new("Alice");
        let new = ContactCard::new("Alice Smith");

        let delta = CardDelta::compute(&old, &new);

        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(
            &delta.changes[0],
            FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith"
        ));
    }

    #[test]
    fn test_delta_compute_field_added() {
        let old = ContactCard::new("Alice");

        let mut new = ContactCard::new("Alice");
        let _ = new.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@example.com",
        ));

        let delta = CardDelta::compute(&old, &new);

        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(&delta.changes[0], FieldChange::Added { .. }));
    }

    #[test]
    fn test_delta_compute_field_modified() {
        let mut old = ContactCard::new("Alice");
        let _ = old.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "old@example.com",
        ));

        let mut new = ContactCard::new("Alice");
        let _ = new.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "new@example.com",
        ));

        let delta = CardDelta::compute(&old, &new);

        // The field IDs are generated, so both have different IDs
        // This will show as added + removed rather than modified
        // For true modification tracking, we'd need stable field IDs
        assert!(!delta.is_empty());
    }

    #[test]
    fn test_delta_compute_field_removed() {
        let mut old = ContactCard::new("Alice");
        let field = ContactField::new(FieldType::Email, "email", "alice@example.com");
        let field_id = field.id().to_string();
        let _ = old.add_field(field);

        let new = ContactCard::new("Alice");

        let delta = CardDelta::compute(&old, &new);

        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(
            &delta.changes[0],
            FieldChange::Removed { field_id: id } if *id == field_id
        ));
    }

    #[test]
    fn test_delta_apply_display_name() {
        let mut card = ContactCard::new("Alice");

        let delta = CardDelta {
            version: 1,
            timestamp: 12345,
            changes: vec![FieldChange::DisplayNameChanged {
                new_name: "Alice Smith".to_string(),
            }],
            signature: [0u8; 64],
        };

        delta.apply(&mut card).unwrap();

        assert_eq!(card.display_name(), "Alice Smith");
    }

    #[test]
    fn test_delta_apply_add_field() {
        let mut card = ContactCard::new("Alice");
        let new_field = ContactField::new(FieldType::Email, "email", "alice@example.com");

        let delta = CardDelta {
            version: 1,
            timestamp: 12345,
            changes: vec![FieldChange::Added { field: new_field }],
            signature: [0u8; 64],
        };

        delta.apply(&mut card).unwrap();

        assert_eq!(card.fields().len(), 1);
        assert_eq!(card.fields()[0].value(), "alice@example.com");
    }

    #[test]
    fn test_delta_apply_remove_field() {
        let mut card = ContactCard::new("Alice");
        let field = ContactField::new(FieldType::Email, "email", "alice@example.com");
        let field_id = field.id().to_string();
        let _ = card.add_field(field);

        let delta = CardDelta {
            version: 1,
            timestamp: 12345,
            changes: vec![FieldChange::Removed { field_id }],
            signature: [0u8; 64],
        };

        delta.apply(&mut card).unwrap();

        assert!(card.fields().is_empty());
    }

    #[test]
    fn test_delta_roundtrip() {
        let mut old = ContactCard::new("Alice");
        let _ = old.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

        let mut new = ContactCard::new("Alice Smith");
        let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));
        let _ = new.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@example.com",
        ));

        let delta = CardDelta::compute(&old, &new);

        // Apply to a copy of old
        let mut result = old.clone();
        delta.apply(&mut result).unwrap();

        assert_eq!(result.display_name(), "Alice Smith");
        assert_eq!(result.fields().len(), 2);
    }

    #[test]
    fn test_delta_sign_and_verify() {
        let identity = Identity::create("Test User");

        let old = ContactCard::new("Alice");
        let new = ContactCard::new("Alice Smith");

        let mut delta = CardDelta::compute(&old, &new);
        delta.sign(&identity);

        // Verify with correct public key
        assert!(delta.verify(identity.signing_public_key()));

        // Verify with wrong public key should fail
        let other_identity = Identity::create("Other User");
        assert!(!delta.verify(other_identity.signing_public_key()));
    }

    #[test]
    fn test_delta_serialization_roundtrip() {
        let mut old = ContactCard::new("Alice");
        let _ = old.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "old@example.com",
        ));

        let mut new = ContactCard::new("Alice");
        let _ = new.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "new@example.com",
        ));

        let delta = CardDelta::compute(&old, &new);

        let json = serde_json::to_string(&delta).unwrap();
        let restored: CardDelta = serde_json::from_str(&json).unwrap();

        assert_eq!(delta.version, restored.version);
        assert_eq!(delta.timestamp, restored.timestamp);
        assert_eq!(delta.changes.len(), restored.changes.len());
    }

    #[test]
    fn test_delta_multiple_changes() {
        let mut old = ContactCard::new("Alice");
        let field1 = ContactField::new(FieldType::Email, "email", "alice@example.com");
        let field1_id = field1.id().to_string();
        let _ = old.add_field(field1);

        let mut new = ContactCard::new("Alice Smith");
        // email field is removed, phone is added
        let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

        let delta = CardDelta::compute(&old, &new);

        // Should have: DisplayNameChanged, Removed (email), Added (phone)
        assert_eq!(delta.changes.len(), 3);

        let has_name_change = delta.changes.iter().any(|c| {
            matches!(c, FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith")
        });
        assert!(has_name_change);

        let has_removed = delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Removed { field_id } if *field_id == field1_id));
        assert!(has_removed);

        let has_added = delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Added { .. }));
        assert!(has_added);
    }

    #[test]
    fn test_delta_filter_for_contact_all_visible() {
        use crate::contact::VisibilityRules;

        let old = ContactCard::new("Alice");
        let mut new = ContactCard::new("Alice");
        let _ = new.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@example.com",
        ));
        let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

        let delta = CardDelta::compute(&old, &new);
        let rules = VisibilityRules::new(); // Default: everyone can see all

        let filtered = delta.filter_for_contact("bob", &rules);

        // Bob should see both fields (default visibility is Everyone)
        assert_eq!(filtered.changes.len(), 2);
    }

    #[test]
    fn test_delta_filter_for_contact_some_hidden() {
        use crate::contact::VisibilityRules;

        let old = ContactCard::new("Alice");
        let mut new = ContactCard::new("Alice");
        let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com");
        let email_id = email_field.id().to_string();
        let _ = new.add_field(email_field);
        let _ = new.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

        let delta = CardDelta::compute(&old, &new);

        // Hide email from Bob
        let mut rules = VisibilityRules::new();
        rules.set_nobody(&email_id);

        let filtered = delta.filter_for_contact("bob", &rules);

        // Bob should only see the phone field
        assert_eq!(filtered.changes.len(), 1);
        assert!(
            matches!(&filtered.changes[0], FieldChange::Added { field } if field.label() == "phone")
        );
    }

    #[test]
    fn test_delta_filter_for_contact_restricted_access() {
        use crate::contact::VisibilityRules;
        use std::collections::HashSet;

        let old = ContactCard::new("Alice");
        let mut new = ContactCard::new("Alice");
        let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com");
        let email_id = email_field.id().to_string();
        let _ = new.add_field(email_field);

        let delta = CardDelta::compute(&old, &new);

        // Email only visible to specific contacts
        let mut rules = VisibilityRules::new();
        let mut allowed = HashSet::new();
        allowed.insert("charlie".to_string());
        rules.set_contacts(&email_id, allowed);

        // Bob is not in the allowed list
        let bob_filtered = delta.filter_for_contact("bob", &rules);
        assert!(bob_filtered.is_empty());

        // Charlie is in the allowed list
        let charlie_filtered = delta.filter_for_contact("charlie", &rules);
        assert_eq!(charlie_filtered.changes.len(), 1);
    }

    #[test]
    fn test_delta_filter_display_name_always_visible() {
        use crate::contact::VisibilityRules;

        let old = ContactCard::new("Alice");
        let new = ContactCard::new("Alice Smith");

        let delta = CardDelta::compute(&old, &new);
        let rules = VisibilityRules::new();

        let filtered = delta.filter_for_contact("bob", &rules);

        // Display name changes are always visible
        assert_eq!(filtered.changes.len(), 1);
        assert!(matches!(
            &filtered.changes[0],
            FieldChange::DisplayNameChanged { .. }
        ));
    }
}
