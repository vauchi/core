//! Contact Module
//!
//! Represents contacts obtained through exchange, with shared encryption keys
//! and visibility rules.

#[cfg(feature = "testing")]
pub mod visibility;
#[cfg(not(feature = "testing"))]
mod visibility;

pub use visibility::{FieldVisibility, VisibilityRules};

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
    /// Whether this contact is hidden from the main contact list.
    /// Hidden contacts are only visible via secret access (gesture/PIN).
    hidden: bool,
    /// Whether this contact is blocked.
    /// Blocked contacts don't receive updates and their updates are ignored.
    blocked: bool,
}

impl Contact {
    /// Creates a new contact from exchange data.
    pub fn from_exchange(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
    ) -> Self {
        let id = hex::encode(public_key);
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
            hidden: false,
            blocked: false,
        }
    }

    /// Creates a contact from device sync data.
    ///
    /// Used when syncing contacts to a new device.
    pub fn from_sync_data(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        exchange_timestamp: u64,
        fingerprint_verified: bool,
        visibility_rules: VisibilityRules,
    ) -> Self {
        Self::from_sync_data_full(
            public_key,
            card,
            shared_key,
            exchange_timestamp,
            fingerprint_verified,
            visibility_rules,
            false, // hidden
            false, // blocked
        )
    }

    /// Creates a contact from device sync data with all fields.
    #[allow(clippy::too_many_arguments)]
    pub fn from_sync_data_full(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        exchange_timestamp: u64,
        fingerprint_verified: bool,
        visibility_rules: VisibilityRules,
        hidden: bool,
        blocked: bool,
    ) -> Self {
        let id = hex::encode(public_key);
        let display_name = card.display_name().to_string();

        Contact {
            id,
            public_key,
            display_name,
            card,
            shared_key,
            exchange_timestamp,
            fingerprint_verified,
            visibility_rules,
            hidden,
            blocked,
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

    /// Accepts a recovery, updating the contact's public key and shared secret.
    ///
    /// This is called when the user accepts a recovery proof from this contact.
    /// The old shared secret is discarded and fingerprint verification is reset.
    pub fn accept_recovery(&mut self, new_public_key: [u8; 32], new_shared_key: SymmetricKey) {
        self.public_key = new_public_key;
        self.id = hex::encode(new_public_key);
        self.shared_key = new_shared_key;
        self.fingerprint_verified = false;
        // Update exchange timestamp to mark when recovery was accepted
        self.exchange_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
    }

    /// Accepts a recovery with a new contact card.
    ///
    /// This is called when the recovering contact also provides an updated card.
    pub fn accept_recovery_with_card(
        &mut self,
        new_public_key: [u8; 32],
        new_shared_key: SymmetricKey,
        new_card: ContactCard,
    ) {
        self.accept_recovery(new_public_key, new_shared_key);
        self.update_card(new_card);
    }

    /// Updates the contact's display name.
    pub fn set_display_name(
        &mut self,
        name: &str,
    ) -> Result<(), crate::contact_card::ContactCardError> {
        self.card.set_display_name(name)?;
        self.display_name = name.to_string();
        Ok(())
    }

    /// Returns a human-readable fingerprint for verification.
    pub fn fingerprint(&self) -> String {
        // Format as groups of 4 hex chars for readability
        let hex = hex::encode(self.public_key);
        hex.chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase()
    }

    // ========================================
    // Hidden Contacts (Plausible Deniability)
    // ========================================

    /// Returns whether this contact is hidden from the main contact list.
    ///
    /// Hidden contacts provide plausible deniability - they only appear when
    /// accessed via a secret gesture, PIN, or special settings navigation.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Hides this contact from the main contact list.
    ///
    /// The contact will only be visible via secret access methods.
    /// Updates from hidden contacts are still received but notifications
    /// are suppressed.
    pub fn hide(&mut self) {
        self.hidden = true;
    }

    /// Unhides this contact, making it visible in the main contact list.
    pub fn unhide(&mut self) {
        self.hidden = false;
    }

    /// Sets the hidden status directly.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    // ========================================
    // Blocked Contacts
    // ========================================

    /// Returns whether this contact is blocked.
    ///
    /// Blocked contacts:
    /// - Don't receive updates from you
    /// - Their updates to you are ignored
    /// - Still appear in the contact list (unless also hidden)
    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// Blocks this contact.
    pub fn block(&mut self) {
        self.blocked = true;
    }

    /// Unblocks this contact.
    pub fn unblock(&mut self) {
        self.blocked = false;
    }

    /// Sets the blocked status directly.
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    /// Returns true if this contact should be visible in the main contact list.
    ///
    /// A contact is visible if it's not hidden.
    /// Blocked contacts can still be visible (to show they're blocked).
    pub fn is_visible_in_main_list(&self) -> bool {
        !self.hidden
    }

    /// Returns true if updates should be processed from this contact.
    ///
    /// Updates are ignored from blocked contacts.
    pub fn should_process_updates(&self) -> bool {
        !self.blocked
    }

    /// Returns true if updates should be sent to this contact.
    ///
    /// Updates are not sent to blocked contacts.
    pub fn should_send_updates(&self) -> bool {
        !self.blocked
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
        assert!(contact
            .visibility_rules()
            .can_see("any_field", &contact.id()));

        // Set a field as private
        contact.visibility_rules_mut().set_nobody("private_field");
        assert!(!contact
            .visibility_rules()
            .can_see("private_field", &contact.id()));
    }

    // ============================================================
    // Additional tests (added for coverage)
    // ============================================================

    #[test]
    fn test_contact_from_sync_data() {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new("Synced User");
        let shared_key = SymmetricKey::generate();
        let mut visibility_rules = VisibilityRules::new();
        visibility_rules.set_nobody("private_field");

        let contact = Contact::from_sync_data(
            public_key,
            card,
            shared_key,
            1234567890, // Specific timestamp
            true,       // Pre-verified
            visibility_rules,
        );

        assert_eq!(contact.display_name(), "Synced User");
        assert_eq!(contact.exchange_timestamp(), 1234567890);
        assert!(contact.is_fingerprint_verified());
        assert!(!contact
            .visibility_rules()
            .can_see("private_field", "anyone"));
    }

    #[test]
    fn test_contact_update_card() {
        let mut contact = create_test_contact();
        assert_eq!(contact.display_name(), "Test User");

        // Update with new card
        let new_card = ContactCard::new("Updated User");
        contact.update_card(new_card);

        assert_eq!(contact.display_name(), "Updated User");
        assert_eq!(contact.card().display_name(), "Updated User");
    }

    #[test]
    fn test_contact_set_display_name() {
        let mut contact = create_test_contact();

        contact.set_display_name("New Name").unwrap();
        assert_eq!(contact.display_name(), "New Name");
        assert_eq!(contact.card().display_name(), "New Name");
    }

    #[test]
    fn test_contact_set_display_name_empty_error() {
        let mut contact = create_test_contact();

        let result = contact.set_display_name("");
        assert!(result.is_err());
    }

    #[test]
    fn test_contact_accessors() {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new("Alice");
        let shared_key = SymmetricKey::generate();

        let contact = Contact::from_exchange(public_key, card, shared_key.clone());

        // Test all accessors return correct values
        assert_eq!(contact.public_key(), &public_key);
        assert_eq!(contact.card().display_name(), "Alice");
        // shared_key returns reference, just verify it's accessible
        let _ = contact.shared_key();
        // exchange_timestamp should be recent (within last minute)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(contact.exchange_timestamp() <= now);
        assert!(contact.exchange_timestamp() > now - 60);
    }

    #[test]
    fn test_contact_id_is_hex_encoded_public_key() {
        let public_key = [0xABu8; 32];
        let card = ContactCard::new("Test");
        let shared_key = SymmetricKey::generate();

        let contact = Contact::from_exchange(public_key, card, shared_key);

        // ID should be hex-encoded public key
        assert_eq!(contact.id(), hex::encode(public_key));
    }

    #[test]
    fn test_fingerprint_readability() {
        let mut public_key = [0u8; 32];
        // Set known values for predictable fingerprint
        public_key[0] = 0xAB;
        public_key[1] = 0xCD;
        public_key[2] = 0xEF;
        public_key[3] = 0x01;

        let card = ContactCard::new("Test");
        let shared_key = SymmetricKey::generate();
        let contact = Contact::from_exchange(public_key, card, shared_key);

        let fp = contact.fingerprint();

        // Should start with known values grouped
        assert!(fp.starts_with("ABCD EF01"));
        // Should have proper spacing
        let parts: Vec<&str> = fp.split(' ').collect();
        assert!(parts.iter().all(|p| p.len() == 4));
    }

    // ============================================================
    // Hidden Contacts Tests
    // ============================================================

    #[test]
    fn test_contact_hidden_default_false() {
        let contact = create_test_contact();
        assert!(!contact.is_hidden());
        assert!(contact.is_visible_in_main_list());
    }

    #[test]
    fn test_contact_hide_and_unhide() {
        let mut contact = create_test_contact();

        // Hide the contact
        contact.hide();
        assert!(contact.is_hidden());
        assert!(!contact.is_visible_in_main_list());

        // Unhide the contact
        contact.unhide();
        assert!(!contact.is_hidden());
        assert!(contact.is_visible_in_main_list());
    }

    #[test]
    fn test_contact_set_hidden() {
        let mut contact = create_test_contact();

        contact.set_hidden(true);
        assert!(contact.is_hidden());

        contact.set_hidden(false);
        assert!(!contact.is_hidden());
    }

    // ============================================================
    // Blocked Contacts Tests
    // ============================================================

    #[test]
    fn test_contact_blocked_default_false() {
        let contact = create_test_contact();
        assert!(!contact.is_blocked());
        assert!(contact.should_process_updates());
        assert!(contact.should_send_updates());
    }

    #[test]
    fn test_contact_block_and_unblock() {
        let mut contact = create_test_contact();

        // Block the contact
        contact.block();
        assert!(contact.is_blocked());
        assert!(!contact.should_process_updates());
        assert!(!contact.should_send_updates());

        // Unblock the contact
        contact.unblock();
        assert!(!contact.is_blocked());
        assert!(contact.should_process_updates());
        assert!(contact.should_send_updates());
    }

    #[test]
    fn test_contact_set_blocked() {
        let mut contact = create_test_contact();

        contact.set_blocked(true);
        assert!(contact.is_blocked());

        contact.set_blocked(false);
        assert!(!contact.is_blocked());
    }

    #[test]
    fn test_contact_hidden_and_blocked_independent() {
        let mut contact = create_test_contact();

        // Can be hidden but not blocked
        contact.hide();
        assert!(contact.is_hidden());
        assert!(!contact.is_blocked());
        assert!(contact.should_process_updates()); // Still processes updates

        // Can be blocked but not hidden
        contact.unhide();
        contact.block();
        assert!(!contact.is_hidden());
        assert!(contact.is_blocked());
        assert!(contact.is_visible_in_main_list()); // Still visible

        // Can be both hidden and blocked
        contact.hide();
        assert!(contact.is_hidden());
        assert!(contact.is_blocked());
        assert!(!contact.is_visible_in_main_list());
        assert!(!contact.should_process_updates());
    }

    #[test]
    fn test_contact_from_sync_data_full() {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new("Synced User");
        let shared_key = SymmetricKey::generate();
        let visibility_rules = VisibilityRules::new();

        let contact = Contact::from_sync_data_full(
            public_key,
            card,
            shared_key,
            1234567890,
            true,
            visibility_rules,
            true, // hidden
            true, // blocked
        );

        assert!(contact.is_hidden());
        assert!(contact.is_blocked());
        assert!(contact.is_fingerprint_verified());
    }
}
