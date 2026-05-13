// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Module
//!
//! Represents contacts obtained through exchange or import, with shared encryption keys
//! and visibility rules for exchanged contacts.

pub mod display;
pub mod kind;
pub mod labels;
pub mod local_group;
pub mod merge;
pub mod statistics;
pub mod trust;
pub mod warnings;

#[cfg(feature = "testing")]
pub mod visibility;
#[cfg(not(feature = "testing"))]
mod visibility;

pub use kind::{ContactKind, ExchangedData, ImportSource, ImportedData};
pub use labels::{
    Group, GroupError, GroupManager, MAX_LABELS, SUGGESTED_LABELS, resolve_visible_fields,
};
pub use local_group::LocalGroup;
pub use trust::TrustLevel;
pub use visibility::{FieldVisibility, VisibilityRules};

use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::crypto::cek::ContentEncryptionKey;
use crate::exchange::TrustMetrics;
use crate::exchange::reciprocity::{ConfirmationChannel, Reciprocity};
use crate::types::{ExchangeTransport, ProximityConfidence};

/// The duration of the undo window for soft-delete operations.
///
/// After a soft delete, the user has this long to undo before hard deletion.
pub const SOFT_DELETE_UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);

/// Error type for contact operations that require a specific contact kind.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ContactError {
    /// The operation requires an exchanged contact but was called on an imported one.
    #[error("Operation requires an exchanged contact (with crypto keys)")]
    OperationRequiresExchangedContact,

    /// Recovery trust requires in-person verified contact (High or Verified trust).
    #[error(
        "Only in-person verified contacts can be trusted for recovery \
         (current level: {0:?})"
    )]
    InsufficientTrustLevel(TrustLevel),

    /// Blocked contacts cannot be trusted for recovery.
    #[error("Blocked contacts cannot be trusted for recovery")]
    ContactIsBlocked,
}

/// A contact obtained through exchange or import.
///
/// Contains their contact card, and for exchanged contacts: shared encryption key,
/// visibility rules, and trust state. Imported contacts have no crypto fields.
#[derive(Clone, Debug)]
pub struct Contact {
    // === Shared fields (both kinds) ===
    /// Their public key fingerprint (unique identifier) for exchanged contacts,
    /// or a UUID v4 for imported contacts.
    id: String,
    /// Their display name (from their card)
    display_name: String,
    /// Their contact card
    card: ContactCard,
    /// Distinguishes exchanged (crypto) from imported (no crypto) contacts.
    kind: ContactKind,
    // === Local-only flags (safe for both kinds) ===
    /// Whether this contact is hidden from the main contact list.
    hidden: bool,
    /// Whether this contact is blocked.
    blocked: bool,
    /// Whether this contact is marked as a favorite (local-only, never shared).
    favorite: bool,
    /// Optional Content Encryption Key for crypto-shredding.
    cek: Option<ContentEncryptionKey>,
    /// Timestamp of the last card update (separate from exchange_timestamp).
    card_updated_at: Option<u64>,
    /// Timestamp of soft-deletion (None = not deleted).
    deleted_at: Option<u64>,
    /// Whether this contact is archived.
    archived: bool,
    /// Timestamp of archival (None = not archived).
    archived_at: Option<u64>,
}

impl Contact {
    /// Creates a new contact from exchange data.
    pub fn from_exchange(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        now: u64,
    ) -> Self {
        let id = hex::encode(public_key);
        let display_name = card.display_name().to_string();
        let exchange_timestamp = now;

        Contact {
            id,
            display_name,
            card,
            kind: ContactKind::Exchanged(ExchangedData {
                public_key,
                shared_key,
                exchange_timestamp,
                exchange_transport: ExchangeTransport::Qr,
                fingerprint_verified: false,
                recovery_trusted: false,
                proposal_trusted: false,
                proximity_confidence: ProximityConfidence::Unknown,
                has_recovered: false,
                relay_url: None,
                relay_noise_pubkey: None,
                trust_metrics: None,
                visibility_rules: VisibilityRules::new(),
                reciprocity: None,
                confirmation_channel: None,
            }),
            hidden: false,
            blocked: false,
            favorite: false,
            cek: None,
            card_updated_at: None,
            deleted_at: None,
            archived: false,
            archived_at: None,
        }
    }

    /// Creates a new contact from exchange data with proximity confidence.
    pub fn from_exchange_with_proximity(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        proximity_confidence: ProximityConfidence,
        now: u64,
    ) -> Self {
        let mut contact = Self::from_exchange(public_key, card, shared_key, now);
        contact.set_proximity_confidence(proximity_confidence);
        contact
    }

    /// Creates a new contact from exchange data with proximity and transport.
    pub fn from_exchange_full(
        public_key: [u8; 32],
        card: ContactCard,
        shared_key: SymmetricKey,
        proximity_confidence: ProximityConfidence,
        exchange_transport: ExchangeTransport,
        now: u64,
    ) -> Self {
        let mut contact = Self::from_exchange(public_key, card, shared_key, now);
        contact.set_proximity_confidence(proximity_confidence);
        contact.set_exchange_transport(exchange_transport);
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
            display_name,
            card,
            kind: ContactKind::Exchanged(ExchangedData {
                public_key,
                shared_key,
                exchange_timestamp,
                exchange_transport: ExchangeTransport::Qr,
                fingerprint_verified,
                recovery_trusted,
                proposal_trusted: false,
                proximity_confidence: ProximityConfidence::Unknown,
                has_recovered: false,
                relay_url: None,
                relay_noise_pubkey: None,
                trust_metrics: None,
                visibility_rules,
                reciprocity: None,
                confirmation_channel: None,
            }),
            hidden,
            blocked,
            favorite: false,
            cek: None,
            card_updated_at: None,
            deleted_at: None,
            archived: false,
            archived_at: None,
        }
    }

    /// Creates a contact from imported data (no crypto keys).
    ///
    /// The ID is a UUID v4 (not derived from a public key, since imported
    /// contacts have no keys). The display name comes from the contact card.
    pub fn from_import(
        card: ContactCard,
        source: ImportSource,
        original_uid: Option<String>,
        now: u64,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let display_name = card.display_name().to_string();
        let imported_at = now;

        Contact {
            id,
            display_name,
            card,
            kind: ContactKind::Imported(ImportedData {
                source,
                imported_at,
                original_uid,
            }),
            hidden: false,
            blocked: false,
            favorite: false,
            cek: None,
            card_updated_at: None,
            deleted_at: None,
            archived: false,
            archived_at: None,
        }
    }

    /// Reconstructs an imported contact from storage data.
    ///
    /// Unlike `from_import`, this uses the existing ID and imported_at timestamp
    /// rather than generating new ones.
    pub(crate) fn from_import_stored(
        id: String,
        card: ContactCard,
        source: ImportSource,
        imported_at: u64,
        original_uid: Option<String>,
    ) -> Self {
        let display_name = card.display_name().to_string();

        Contact {
            id,
            display_name,
            card,
            kind: ContactKind::Imported(ImportedData {
                source,
                imported_at,
                original_uid,
            }),
            hidden: false,
            blocked: false,
            favorite: false,
            cek: None,
            card_updated_at: None,
            deleted_at: None,
            archived: false,
            archived_at: None,
        }
    }

    // ========================================
    // Kind accessors
    // ========================================

    /// Returns the contact kind (Exchanged or Imported).
    pub fn kind(&self) -> &ContactKind {
        &self.kind
    }

    /// Returns `true` if this is an exchanged (crypto) contact.
    pub fn is_exchanged(&self) -> bool {
        self.kind.is_exchanged()
    }

    /// Returns `true` if this is an imported (non-crypto) contact.
    pub fn is_imported(&self) -> bool {
        self.kind.is_imported()
    }

    // ========================================
    // Shared getters (work for both kinds)
    // ========================================

    /// Returns the contact's unique ID (public key fingerprint or UUID).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the contact's display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the contact's card.
    pub fn card(&self) -> &ContactCard {
        &self.card
    }

    /// Returns the timestamp of the last card update, if any.
    pub fn card_updated_at(&self) -> Option<u64> {
        self.card_updated_at
    }

    /// Sets the card_updated_at timestamp.
    pub fn set_card_updated_at(&mut self, timestamp: Option<u64>) {
        self.card_updated_at = timestamp;
    }

    // ========================================
    // Exchanged-only getters (return Option)
    // ========================================

    /// Returns the contact's public key, if this is an exchanged contact.
    pub fn public_key(&self) -> Option<&[u8; 32]> {
        self.kind.exchanged_data().map(|d| &d.public_key)
    }

    /// Returns the shared encryption key, if this is an exchanged contact.
    pub fn shared_key(&self) -> Option<&SymmetricKey> {
        self.kind.exchanged_data().map(|d| &d.shared_key)
    }

    /// Returns the exchange timestamp, if this is an exchanged contact.
    pub fn exchange_timestamp(&self) -> Option<u64> {
        self.kind.exchanged_data().map(|d| d.exchange_timestamp)
    }

    /// Returns whether the fingerprint was manually verified.
    /// Returns `false` for imported contacts (no fingerprint to verify).
    pub fn is_fingerprint_verified(&self) -> bool {
        self.kind
            .exchanged_data()
            .is_some_and(|d| d.fingerprint_verified)
    }

    /// Returns a reference to the visibility rules, if this is an exchanged contact.
    pub fn visibility_rules(&self) -> Option<&VisibilityRules> {
        self.kind.exchanged_data().map(|d| &d.visibility_rules)
    }

    /// Returns a mutable reference to the visibility rules, if exchanged.
    pub fn visibility_rules_mut(&mut self) -> Option<&mut VisibilityRules> {
        self.kind
            .exchanged_data_mut()
            .map(|d| &mut d.visibility_rules)
    }

    /// Returns the proximity confidence level from the exchange.
    /// Returns `Unknown` for imported contacts.
    pub fn proximity_confidence(&self) -> &ProximityConfidence {
        self.kind
            .exchanged_data()
            .map_or(&ProximityConfidence::Unknown, |d| &d.proximity_confidence)
    }

    /// Returns the exchange transport method, if this is an exchanged contact.
    pub fn exchange_transport(&self) -> Option<ExchangeTransport> {
        self.kind.exchanged_data().map(|d| d.exchange_transport)
    }

    /// Returns whether this contact has undergone identity recovery.
    /// Returns `false` for imported contacts.
    pub fn has_recovered(&self) -> bool {
        self.kind.exchanged_data().is_some_and(|d| d.has_recovered)
    }

    /// Returns the full trust metrics from the exchange, if present.
    pub fn trust_metrics(&self) -> Option<&TrustMetrics> {
        self.kind
            .exchanged_data()
            .and_then(|d| d.trust_metrics.as_ref())
    }

    /// Returns the contact's relay URL, if known.
    pub fn relay_url(&self) -> Option<&str> {
        self.kind
            .exchanged_data()
            .and_then(|d| d.relay_url.as_deref())
    }

    /// Returns the contact's relay Noise NK public key, if known.
    pub fn relay_noise_pubkey(&self) -> Option<&[u8; 32]> {
        self.kind
            .exchanged_data()
            .and_then(|d| d.relay_noise_pubkey.as_ref())
    }

    // ========================================
    // Exchanged-only setters
    // ========================================

    /// Sets the proximity confidence level (no-op for imported contacts).
    pub fn set_proximity_confidence(&mut self, confidence: ProximityConfidence) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.proximity_confidence = confidence;
        }
    }

    /// Sets the exchange transport method (no-op for imported contacts).
    pub fn set_exchange_transport(&mut self, transport: ExchangeTransport) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.exchange_transport = transport;
        }
    }

    /// Sets the has_recovered flag (no-op for imported contacts).
    pub fn set_has_recovered(&mut self, recovered: bool) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.has_recovered = recovered;
        }
    }

    /// Sets or clears the trust metrics (no-op for imported contacts).
    pub fn set_trust_metrics(&mut self, metrics: Option<TrustMetrics>) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.trust_metrics = metrics;
        }
    }

    /// Returns the reciprocity status. `None` in ExchangedData maps to `Unknown` (legacy).
    ///
    /// Includes a passive 7-day timer: `Pending` contacts whose exchange timestamp
    /// is older than 7 days are reported as `Unreciprocated` (design spec §6.3).
    /// This is a read-time check — the stored value stays `Pending` until explicitly
    /// written by the relaunch recovery scan.
    pub fn reciprocity(&self, now: u64) -> Reciprocity {
        match self.kind.exchanged_data().and_then(|d| d.reciprocity) {
            Some(Reciprocity::Pending) => {
                let exchange_ts = self
                    .kind
                    .exchanged_data()
                    .map(|d| d.exchange_timestamp)
                    .unwrap_or(0);
                if now > exchange_ts + 7 * 24 * 60 * 60 {
                    Reciprocity::Unreciprocated
                } else {
                    Reciprocity::Pending
                }
            }
            Some(r) => r,
            None => Reciprocity::Unknown,
        }
    }

    /// Returns the confirmation channel, if reciprocity has been resolved.
    pub fn confirmation_channel(&self) -> Option<ConfirmationChannel> {
        self.kind
            .exchanged_data()
            .and_then(|d| d.confirmation_channel)
    }

    /// Sets the reciprocity status (no-op for imported contacts).
    pub fn set_reciprocity(&mut self, reciprocity: Reciprocity) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.reciprocity = Some(reciprocity);
        }
    }

    /// Sets the confirmation channel (no-op for imported contacts).
    pub fn set_confirmation_channel(&mut self, channel: ConfirmationChannel) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.confirmation_channel = Some(channel);
        }
    }

    /// Sets the contact's relay URL (no-op for imported contacts).
    pub fn set_relay_url(&mut self, url: Option<String>) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.relay_url = url;
        }
    }

    /// Sets the contact's relay Noise NK public key (no-op for imported contacts).
    pub fn set_relay_noise_pubkey(&mut self, pubkey: Option<[u8; 32]>) {
        if let Some(data) = self.kind.exchanged_data_mut() {
            data.relay_noise_pubkey = pubkey;
        }
    }

    /// Marks the fingerprint as verified.
    ///
    /// Returns `Err` if called on an imported contact.
    /// Marks this contact's key fingerprint as manually verified.
    ///
    /// Clears `has_recovered` — fingerprint verification is an
    /// in-person act that re-establishes trust after recovery.
    pub fn mark_fingerprint_verified(&mut self) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.fingerprint_verified = true;
        data.has_recovered = false;
        Ok(())
    }

    /// Removes fingerprint verification.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn mark_fingerprint_unverified(&mut self) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.fingerprint_verified = false;
        Ok(())
    }

    /// Updates this contact's card (from a sync update).
    pub fn update_card(&mut self, card: ContactCard, now: u64) {
        self.display_name = card.display_name().to_string();
        self.card = card;
        self.card_updated_at = Some(now);
    }

    /// Accepts a recovery, updating the contact's public key and shared secret.
    ///
    /// This is called when the user accepts a recovery proof from this contact.
    /// The old shared secret is discarded and fingerprint verification is reset.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn accept_recovery(
        &mut self,
        new_public_key: [u8; 32],
        new_shared_key: SymmetricKey,
        now: u64,
    ) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.public_key = new_public_key;
        data.shared_key = new_shared_key;
        data.fingerprint_verified = false;
        data.has_recovered = true;
        data.exchange_timestamp = now;
        self.id = hex::encode(new_public_key);
        Ok(())
    }

    /// Accepts a recovery with a new contact card.
    ///
    /// This is called when the recovering contact also provides an updated card.
    pub fn accept_recovery_with_card(
        &mut self,
        new_public_key: [u8; 32],
        new_shared_key: SymmetricKey,
        new_card: ContactCard,
        now: u64,
    ) -> Result<(), ContactError> {
        self.accept_recovery(new_public_key, new_shared_key, now)?;
        self.update_card(new_card, now);
        Ok(())
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
    ///
    /// Returns an empty string for imported contacts (no public key).
    pub fn fingerprint(&self) -> String {
        let pk = match self.kind.exchanged_data() {
            Some(data) => data.public_key,
            None => return String::new(),
        };
        let hex = hex::encode(pk);
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
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Hides this contact from the main contact list.
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
    /// Returns `false` for imported contacts.
    pub fn is_recovery_trusted(&self) -> bool {
        self.kind
            .exchanged_data()
            .is_some_and(|d| d.recovery_trusted)
    }

    /// Marks this contact as trusted for recovery.
    ///
    /// Returns `Err` if called on an imported contact, a blocked contact,
    /// or if trust level is below High (not in-person verified).
    pub fn trust_for_recovery(&mut self) -> Result<(), ContactError> {
        let _data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;

        if self.is_blocked() {
            return Err(ContactError::ContactIsBlocked);
        }

        let level = self.trust_level();
        if !matches!(level, TrustLevel::High | TrustLevel::Verified) {
            return Err(ContactError::InsufficientTrustLevel(level));
        }

        // Re-borrow after trust_level() released the immutable ref
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.recovery_trusted = true;
        Ok(())
    }

    /// Removes recovery trust from this contact.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn untrust_for_recovery(&mut self) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.recovery_trusted = false;
        Ok(())
    }

    /// Sets the recovery trust status directly.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn set_recovery_trusted(&mut self, trusted: bool) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.recovery_trusted = trusted;
        Ok(())
    }

    // ========================================
    // Proposal Trust
    // ========================================

    /// Returns whether this contact is trusted for simplified contact proposals.
    /// Returns `false` for imported contacts.
    pub fn is_proposal_trusted(&self) -> bool {
        self.kind
            .exchanged_data()
            .is_some_and(|d| d.proposal_trusted)
    }

    /// Marks this contact as trusted for proposals.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn trust_for_proposals(&mut self) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.proposal_trusted = true;
        Ok(())
    }

    /// Removes proposal trust from this contact.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn untrust_for_proposals(&mut self) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.proposal_trusted = false;
        Ok(())
    }

    /// Sets the proposal trust status directly.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn set_proposal_trusted(&mut self, trusted: bool) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.proposal_trusted = trusted;
        Ok(())
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
    // Soft-Delete
    // ========================================

    /// Returns the soft-deletion timestamp, if set.
    pub fn deleted_at(&self) -> Option<u64> {
        self.deleted_at
    }

    /// Returns whether this contact has been soft-deleted.
    pub fn is_soft_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// Soft-deletes this contact, recording the given timestamp.
    pub fn soft_delete(&mut self, timestamp: u64) {
        self.deleted_at = Some(timestamp);
    }

    /// Undoes a soft-delete, clearing the deletion timestamp.
    pub fn undo_soft_delete(&mut self) {
        self.deleted_at = None;
    }

    // ========================================
    // Archive
    // ========================================

    /// Returns whether this contact is archived.
    pub fn is_archived(&self) -> bool {
        self.archived
    }

    /// Returns the archive timestamp, if set.
    pub fn archived_at(&self) -> Option<u64> {
        self.archived_at
    }

    /// Archives this contact, recording the given timestamp.
    pub fn archive(&mut self, timestamp: u64) {
        self.archived = true;
        self.archived_at = Some(timestamp);
    }

    /// Unarchives this contact, clearing the archived flag and timestamp.
    pub fn unarchive(&mut self) {
        self.archived = false;
        self.archived_at = None;
    }

    // ========================================
    // Content Encryption Key (CEK)
    // ========================================

    /// Returns the CEK if present.
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
    // Trust Level (derived, read-only)
    // ========================================

    /// Derives the trust level from cryptographic exchange facts.
    ///
    /// Returns `Standard` for imported contacts (no crypto trust basis).
    ///
    /// Priority order (highest wins):
    /// 1. `Cautious` — identity was recovered (ratchet may have reset)
    /// 2. `Verified` — fingerprint manually confirmed out-of-band
    /// 3. `High`     — strong transport proximity or high verifier confidence
    /// 4. `Standard` — all other cases
    pub fn trust_level(&self) -> TrustLevel {
        let data = match self.kind.exchanged_data() {
            Some(d) => d,
            None => return TrustLevel::Standard,
        };

        // Priority 1: Recovery state overrides everything.
        // A recovered identity drops to Cautious until the user
        // re-verifies the fingerprint in person — which clears
        // has_recovered via mark_fingerprint_verified().
        // Principle 2: "trust is earned in person."
        if data.has_recovered {
            return TrustLevel::Cautious;
        }

        // Priority 2: Manual fingerprint verification (out-of-band)
        if data.fingerprint_verified {
            return TrustLevel::Verified;
        }

        // Priority 3: Use TrustMetrics if available (new path)
        if let Some(ref metrics) = data.trust_metrics {
            let strong_transport = metrics.transport_proximity.is_strong();
            let strong_verifier = metrics.proximity == ProximityConfidence::High;

            if strong_transport || strong_verifier {
                return TrustLevel::High;
            }
            return TrustLevel::Standard;
        }

        // Legacy path: no TrustMetrics (pre-migration contacts)
        if data.proximity_confidence == ProximityConfidence::High
            && matches!(
                data.exchange_transport,
                ExchangeTransport::Nfc | ExchangeTransport::Ble
            )
        {
            return TrustLevel::High;
        }
        TrustLevel::Standard
    }

    // ========================================
    // Utility
    // ========================================

    /// Returns true if this contact should be visible in the main contact list.
    pub fn is_visible_in_main_list(&self) -> bool {
        !self.hidden
    }

    /// Returns true if updates should be processed from this contact.
    pub fn should_process_updates(&self) -> bool {
        !self.blocked
    }

    /// Returns true if updates should be sent to this contact.
    pub fn should_send_updates(&self) -> bool {
        !self.blocked
    }
}
