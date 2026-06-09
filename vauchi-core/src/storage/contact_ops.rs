// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact-ops forwarders to [`ContactStore`](super::ContactStore); last-sync stays a SyncStore forwarder.

use super::{Storage, StorageError};
use crate::contact_card::ContactCard;
use crate::crypto::cek::ContentEncryptionKey;

impl Storage {
    /// Saves personal notes for a contact, encrypting at the storage layer.
    ///
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `personal_notes_encrypted` column.
    pub fn save_personal_notes(&self, contact_id: &str, notes: &[u8]) -> Result<(), StorageError> {
        self.contacts().save_personal_notes(contact_id, notes)
    }
    /// Loads personal notes for a contact, decrypting at the storage layer.
    ///
    /// Returns decrypted plaintext bytes, or `None` if no notes are stored.
    /// Self-healing: if the stored data is legacy plaintext (pre-encryption gap),
    /// returns it as-is — the next save will encrypt it properly.
    pub fn load_personal_notes(&self, contact_id: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.contacts().load_personal_notes(contact_id)
    }
    /// Deletes personal notes for a contact.
    ///
    /// Sets the `personal_notes_encrypted` column to NULL.
    pub fn delete_personal_notes(&self, contact_id: &str) -> Result<(), StorageError> {
        self.contacts().delete_personal_notes(contact_id)
    }
    // Contact field notes: see storage/field_notes.rs

    /// Counts the total number of contacts in storage.
    pub fn count_contacts(&self) -> Result<usize, StorageError> {
        self.contacts().count_contacts()
    }
    /// Returns the maximum number of contacts allowed.
    ///
    /// Reads from the `contact_limits` table (created by migration v4).
    /// Returns 10,000 as the default if no limit has been configured.
    pub fn get_contact_limit(&self) -> Result<usize, StorageError> {
        self.contacts().get_contact_limit()
    }
    /// Sets the maximum number of contacts allowed.
    ///
    /// Updates the `contact_limits` table (created by migration v4).
    /// A limit of zero means no contacts are allowed.
    pub fn set_contact_limit(&self, max_contacts: usize) -> Result<(), StorageError> {
        self.contacts().set_contact_limit(max_contacts)
    }
    /// Saves the user's own contact card (encrypted).
    pub fn save_own_card(&self, card: &ContactCard) -> Result<(), StorageError> {
        self.contacts().save_own_card(card)
    }
    /// Loads the user's own contact card (decrypted).
    ///
    /// Reads from encrypted column first; falls back to plaintext for
    /// pre-v13 databases where migration hasn't populated the encrypted column.
    pub fn load_own_card(&self) -> Result<Option<ContactCard>, StorageError> {
        self.contacts().load_own_card()
    }
    /// Saves a CEK for a contact, encrypted with the storage master key.
    ///
    /// The CEK controls at-rest readability of the contact card (crypto-shredding).
    pub fn save_contact_cek(
        &self,
        contact_id: &str,
        cek: &ContentEncryptionKey,
    ) -> Result<(), StorageError> {
        self.contacts().save_contact_cek(contact_id, cek)
    }
    /// Loads the CEK for a contact. Returns None for legacy contacts (pre-CEK).
    pub fn load_contact_cek(
        &self,
        contact_id: &str,
    ) -> Result<Option<ContentEncryptionKey>, StorageError> {
        self.contacts().load_contact_cek(contact_id)
    }
    /// Deletes the CEK for a contact (crypto-shredding).
    ///
    /// Sets `cek_encrypted` to NULL, rendering the card permanently unreadable
    /// if it was encrypted with the CEK.
    pub fn delete_contact_cek(&self, contact_id: &str) -> Result<(), StorageError> {
        self.contacts().delete_contact_cek(contact_id)
    }
    /// Returns the last applied delta version for a contact (#42).
    ///
    /// Returns 0 if no version has been recorded (new or legacy contact).
    pub fn last_delta_version(&self, contact_id: &str) -> Result<u32, StorageError> {
        self.contacts().last_delta_version(contact_id)
    }
    /// Records the last applied delta version for a contact (#42).
    pub fn record_delta_version(&self, contact_id: &str, version: u32) -> Result<(), StorageError> {
        self.contacts().record_delta_version(contact_id, version)
    }
    /// Returns the last sent delta version for a contact.
    ///
    /// Returns 0 if no version has been sent (new contact).
    pub fn last_sent_delta_version(&self, contact_id: &str) -> Result<u32, StorageError> {
        self.contacts().last_sent_delta_version(contact_id)
    }
    /// Records the last sent delta version for a contact.
    pub fn record_sent_delta_version(
        &self,
        contact_id: &str,
        version: u32,
    ) -> Result<(), StorageError> {
        self.contacts()
            .record_sent_delta_version(contact_id, version)
    }
    /// Returns the set of field ids last sent (visible) to a contact.
    ///
    /// `None` means nothing has been sent yet (no baseline — distinguishes
    /// "never sent" from "sent nothing"). Used by `repropagate_to_contact` to
    /// emit `Removed` deltas on revocation
    /// (`2026-06-08-card-revocation-not-propagated`).
    pub fn load_last_sent_visible_fields(
        &self,
        contact_id: &str,
    ) -> Result<Option<std::collections::HashSet<String>>, StorageError> {
        self.contacts().load_last_sent_visible_fields(contact_id)
    }
    /// Records the set of field ids last sent (visible) to a contact (the
    /// baseline a later revocation is diffed against).
    pub fn save_last_sent_visible_fields(
        &self,
        contact_id: &str,
        fields: &std::collections::HashSet<String>,
    ) -> Result<(), StorageError> {
        self.contacts()
            .save_last_sent_visible_fields(contact_id, fields)
    }
    /// Records a revoked sender in the tombstone table.
    ///
    /// Prevents future updates from this sender from being processed,
    /// even if the contact row has been deleted.
    pub fn record_revoked_sender(
        &self,
        sender_id: &str,
        revoked_at: u64,
    ) -> Result<(), StorageError> {
        self.contacts().record_revoked_sender(sender_id, revoked_at)
    }
    /// Checks if a sender has been revoked.
    pub fn is_sender_revoked(&self, sender_id: &str) -> Result<bool, StorageError> {
        self.contacts().is_sender_revoked(sender_id)
    }
    /// Records a dismissed duplicate pair.
    ///
    /// The pair is normalized so id1 < id2 lexicographically, ensuring
    /// (A, B) and (B, A) are stored identically.
    pub fn dismiss_duplicate(&self, id1: &str, id2: &str) -> Result<(), StorageError> {
        self.contacts().dismiss_duplicate(id1, id2)
    }
    /// Loads all dismissed duplicate pairs.
    ///
    /// Returns a set of (id1, id2) tuples where id1 < id2 lexicographically.
    pub fn load_dismissed_duplicates(
        &self,
    ) -> Result<std::collections::HashSet<(String, String)>, StorageError> {
        self.contacts().load_dismissed_duplicates()
    }
    /// Removes a dismissed duplicate pair (e.g., when contacts are deleted).
    pub fn undismiss_duplicate(&self, id1: &str, id2: &str) -> Result<(), StorageError> {
        self.contacts().undismiss_duplicate(id1, id2)
    }

    /// Sets the last sync timestamp for a contact.
    ///
    /// This is used to track when the last successful sync occurred.
    /// Uses a separate table from contacts to allow tracking sync timestamps
    /// independently of whether the contact exists in the contacts table.
    pub fn set_contact_last_sync(
        &self,
        contact_id: &str,
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.sync().set_contact_last_sync(contact_id, timestamp)
    }

    /// Gets the last sync timestamp for a contact (decrypted).
    ///
    /// Returns None if the contact hasn't been synced yet.
    pub fn get_contact_last_sync(&self, contact_id: &str) -> Result<Option<u64>, StorageError> {
        self.sync().get_contact_last_sync(contact_id)
    }
}
