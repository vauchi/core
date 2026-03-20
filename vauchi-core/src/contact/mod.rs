// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Module
//!
//! Represents contacts obtained through exchange, with shared encryption keys
//! and visibility rules.

pub mod labels;
pub mod merge;
pub mod statistics;
pub mod warnings;

#[cfg(feature = "testing")]
pub mod visibility;
#[cfg(not(feature = "testing"))]
mod visibility;

pub use labels::{
    Group, GroupError, GroupManager, MAX_LABELS, SUGGESTED_LABELS, resolve_visible_fields,
};
pub use visibility::{FieldVisibility, VisibilityRules};

use std::time::{SystemTime, UNIX_EPOCH};

use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::crypto::cek::ContentEncryptionKey;
use crate::exchange::{ExchangeTransport, ProximityConfidence};

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
    /// Whether this contact is trusted for recovery purposes.
    /// Only trusted contacts can vouch during social recovery.
    /// This is private — the contact is never told their trust status.
    recovery_trusted: bool,
    /// Whether this contact is marked as a favorite (local-only, never shared).
    favorite: bool,
    /// Proximity confidence level from the exchange.
    proximity_confidence: ProximityConfidence,
    /// Optional Content Encryption Key for crypto-shredding.
    /// When present, the card is encrypted at rest with this CEK (not the storage key).
    /// Destroying this key renders the card permanently unreadable.
    cek: Option<ContentEncryptionKey>,
    /// How this contact was established (QR, NFC, BLE).
    /// Persisted in storage to provide trust context.
    exchange_transport: ExchangeTransport,
    /// Whether this contact has undergone identity recovery.
    /// Set to true when `accept_recovery()` is called. Never reset.
    has_recovered: bool,
    /// Timestamp of the last card update (separate from exchange_timestamp).
    /// None until the first `update_card()` call.
    card_updated_at: Option<u64>,
    /// Relay URL learned during exchange (for per-contact relay routing).
    /// When set, updates for this contact are sent to their relay instead of our home relay.
    relay_url: Option<String>,
    /// Relay's Noise NK public key, pinned during in-person exchange.
    /// Used to verify the relay's identity on connect (eliminates TOFU).
    relay_noise_pubkey: Option<[u8; 32]>,
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
            recovery_trusted: false,
            favorite: false,
            proximity_confidence: ProximityConfidence::Unknown,
            cek: None,
            exchange_transport: ExchangeTransport::Qr,
            has_recovered: false,
            card_updated_at: None,
            relay_url: None,
            relay_noise_pubkey: None,
        }
    }

    /// Creates a new contact from exchange data with proximity confidence.
    pub fn from_exchange_with_proximity(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        proximity_confidence: ProximityConfidence,
    ) -> Self {
        let mut contact = Self::from_exchange(public_key, card, shared_key);
        contact.proximity_confidence = proximity_confidence;
        contact
    }

    /// Creates a new contact from exchange data with proximity and transport.
    pub fn from_exchange_full(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        proximity_confidence: ProximityConfidence,
        exchange_transport: ExchangeTransport,
    ) -> Self {
        let mut contact = Self::from_exchange(public_key, card, shared_key);
        contact.proximity_confidence = proximity_confidence;
        contact.exchange_transport = exchange_transport;
        contact
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
            false, // recovery_trusted
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
        recovery_trusted: bool,
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
            recovery_trusted,
            favorite: false,
            proximity_confidence: ProximityConfidence::Unknown,
            cek: None,
            exchange_transport: ExchangeTransport::Qr,
            has_recovered: false,
            card_updated_at: None,
            relay_url: None,
            relay_noise_pubkey: None,
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
        self.card_updated_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs(),
        );
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
        self.has_recovered = true;
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

    // ========================================
    // Recovery Trust
    // ========================================

    /// Returns whether this contact is trusted for recovery purposes.
    ///
    /// Only recovery-trusted contacts can vouch during social recovery.
    /// Trust status is private and never shared with the contact.
    pub fn is_recovery_trusted(&self) -> bool {
        self.recovery_trusted
    }

    /// Marks this contact as trusted for recovery.
    pub fn trust_for_recovery(&mut self) {
        self.recovery_trusted = true;
    }

    /// Removes recovery trust from this contact.
    pub fn untrust_for_recovery(&mut self) {
        self.recovery_trusted = false;
    }

    /// Sets the recovery trust status directly.
    pub fn set_recovery_trusted(&mut self, trusted: bool) {
        self.recovery_trusted = trusted;
    }

    // ========================================
    // Favorite
    // ========================================

    /// Returns whether this contact is a favorite.
    pub fn is_favorite(&self) -> bool {
        self.favorite
    }

    /// Sets the favorite status for this contact.
    pub fn set_favorite(&mut self, favorite: bool) {
        self.favorite = favorite;
    }

    /// Returns a sort key that places favorites first, then alphabetical.
    pub fn effective_sort_key(&self) -> String {
        let prefix = if self.favorite { "0" } else { "1" };
        format!("{}:{}", prefix, self.display_name.to_lowercase())
    }

    // ========================================
    // Proximity Confidence
    // ========================================

    /// Returns the proximity confidence level from the exchange.
    pub fn proximity_confidence(&self) -> &ProximityConfidence {
        &self.proximity_confidence
    }

    /// Sets the proximity confidence level.
    pub fn set_proximity_confidence(&mut self, confidence: ProximityConfidence) {
        self.proximity_confidence = confidence;
    }

    // ========================================
    // Content Encryption Key (CEK)
    // ========================================

    /// Returns the CEK if present. CEK-protected contacts have their card
    /// encrypted at rest with this key instead of the storage master key.
    pub fn cek(&self) -> Option<&ContentEncryptionKey> {
        self.cek.as_ref()
    }

    /// Sets the CEK for this contact, enabling crypto-shredding.
    pub fn set_cek(&mut self, cek: ContentEncryptionKey) {
        self.cek = Some(cek);
    }

    /// Clears the CEK (used after crypto-shredding / CEK deletion).
    pub fn clear_cek(&mut self) {
        self.cek = None;
    }

    // ========================================
    // Trust Metrics
    // ========================================

    /// Returns the exchange transport method.
    pub fn exchange_transport(&self) -> ExchangeTransport {
        self.exchange_transport
    }

    /// Sets the exchange transport method.
    pub fn set_exchange_transport(&mut self, transport: ExchangeTransport) {
        self.exchange_transport = transport;
    }

    /// Returns whether this contact has undergone identity recovery.
    pub fn has_recovered(&self) -> bool {
        self.has_recovered
    }

    /// Sets the has_recovered flag.
    pub fn set_has_recovered(&mut self, recovered: bool) {
        self.has_recovered = recovered;
    }

    /// Returns the timestamp of the last card update, if any.
    pub fn card_updated_at(&self) -> Option<u64> {
        self.card_updated_at
    }

    /// Sets the card_updated_at timestamp.
    pub fn set_card_updated_at(&mut self, timestamp: Option<u64>) {
        self.card_updated_at = timestamp;
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

    // ========================================
    // Relay Metadata (per-contact routing)
    // ========================================

    /// Returns the contact's relay URL, if known.
    ///
    /// When set, updates for this contact should be sent to this relay
    /// instead of our home relay. Learned during in-person exchange.
    pub fn relay_url(&self) -> Option<&str> {
        self.relay_url.as_deref()
    }

    /// Sets the contact's relay URL.
    pub fn set_relay_url(&mut self, url: Option<String>) {
        self.relay_url = url;
    }

    /// Returns the contact's relay Noise NK public key, if known.
    ///
    /// When set, connections to this contact's relay must verify the
    /// relay's Noise static key matches this pinned value.
    pub fn relay_noise_pubkey(&self) -> Option<&[u8; 32]> {
        self.relay_noise_pubkey.as_ref()
    }

    /// Sets the contact's relay Noise NK public key.
    pub fn set_relay_noise_pubkey(&mut self, pubkey: Option<[u8; 32]>) {
        self.relay_noise_pubkey = pubkey;
    }
}
