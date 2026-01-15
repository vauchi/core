//! Contact Module
//!
//! Represents contacts obtained through exchange, with shared encryption keys
//! and visibility rules.

mod visibility;

pub use visibility::{FieldVisibility, VisibilityRules};

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;

/// A contact obtained through exchange.
///
/// Contains their contact card, shared encryption key, and visibility rules.
#[derive(Clone, Debug)]
pub struct Contact {
    /// Their public key fingerprint (unique identifier)
    id: String,
    /// Their Ed25519 public key
    public_key: [u8; 32],
    /// Their display name (from their card)
    display_name: String,
    /// Their contact card
    card: ContactCard,
    /// Shared symmetric key for communication
    shared_key: SymmetricKey,
    /// Unix timestamp of when the exchange occurred
    exchange_timestamp: u64,
    /// Whether the user manually verified their fingerprint
    fingerprint_verified: bool,
    /// Our visibility rules for this contact (what they can see of our card)
    visibility_rules: VisibilityRules,
}

impl Contact {
    /// Creates a new contact from exchange data.
    pub fn from_exchange(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
    ) -> Self {
        let id = hex::encode(&public_key);
        let display_name = card.display_name().to_string();
        let exchange_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Contact {
            id,
            public_key,
            display_name,
            card,
            shared_key,
            exchange_timestamp,
            fingerprint_verified: false,
            visibility_rules: VisibilityRules::new(),
        }
    }

    /// Returns the contact's unique ID (public key fingerprint).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the contact's public key.
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    /// Returns the contact's display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the contact's card.
    pub fn card(&self) -> &ContactCard {
        &self.card
    }

    /// Returns the shared encryption key.
    pub fn shared_key(&self) -> &SymmetricKey {
        &self.shared_key
    }

    /// Returns the exchange timestamp.
    pub fn exchange_timestamp(&self) -> u64 {
        self.exchange_timestamp
    }

    /// Returns whether the fingerprint was manually verified.
    pub fn is_fingerprint_verified(&self) -> bool {
        self.fingerprint_verified
    }

    /// Marks the fingerprint as verified.
    pub fn mark_fingerprint_verified(&mut self) {
        self.fingerprint_verified = true;
    }

    /// Returns a reference to the visibility rules.
    pub fn visibility_rules(&self) -> &VisibilityRules {
        &self.visibility_rules
    }

    /// Returns a mutable reference to the visibility rules.
    pub fn visibility_rules_mut(&mut self) -> &mut VisibilityRules {
        &mut self.visibility_rules
    }

    /// Updates this contact's card (from a sync update).
    pub fn update_card(&mut self, card: ContactCard) {
        self.display_name = card.display_name().to_string();
        self.card = card;
    }

    /// Returns a human-readable fingerprint for verification.
    pub fn fingerprint(&self) -> String {
        // Format as groups of 4 hex chars for readability
        let hex = hex::encode(&self.public_key);
        hex.chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;

    fn create_test_contact() -> Contact {
        let public_key = [0u8; 32];
        let card = ContactCard::new("Test User");
        let shared_key = SymmetricKey::generate();

        Contact::from_exchange(public_key, card, shared_key)
    }

    #[test]
    fn test_create_contact() {
        let contact = create_test_contact();

        assert!(!contact.id().is_empty());
        assert_eq!(contact.display_name(), "Test User");
        assert!(!contact.is_fingerprint_verified());
    }

    #[test]
    fn test_fingerprint_verification() {
        let mut contact = create_test_contact();

        assert!(!contact.is_fingerprint_verified());
        contact.mark_fingerprint_verified();
        assert!(contact.is_fingerprint_verified());
    }

    #[test]
    fn test_fingerprint_format() {
        let contact = create_test_contact();
        let fp = contact.fingerprint();

        // Should be formatted with spaces every 4 chars
        assert!(fp.contains(' '));
        // Should be uppercase
        assert_eq!(fp, fp.to_uppercase());
    }

    #[test]
    fn test_visibility_rules() {
        let mut contact = create_test_contact();

        // Initially no specific rules
        assert!(contact.visibility_rules().can_see("any_field", &contact.id()));

        // Set a field as private
        contact.visibility_rules_mut().set_nobody("private_field");
        assert!(!contact.visibility_rules().can_see("private_field", &contact.id()));
    }
}
