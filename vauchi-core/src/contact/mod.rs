//! Contact Module
//!
//! Represents contacts obtained through exchange, with shared encryption keys
//! and visibility rules.

pub mod labels;

#[cfg(feature = "testing")]
pub mod visibility;
#[cfg(not(feature = "testing"))]
mod visibility;

pub use labels::{LabelError, LabelManager, VisibilityLabel, MAX_LABELS, SUGGESTED_LABELS};
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
