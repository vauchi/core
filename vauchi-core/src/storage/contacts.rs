// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact storage forwarders to [`ContactStore`](super::ContactStore); plus the cross-cutting `delete_contact` orchestrator.

use super::{Storage, StorageError};
use crate::contact::Contact;
use rusqlite::params;

impl Storage {
    /// Saves a contact to storage.
    ///
    /// If the contact has a CEK, the card is encrypted with the CEK (not the
    /// storage key) and the `display_name` column is set to NULL. The CEK itself
    /// is encrypted with the storage key and stored in `cek_encrypted`.
    ///
    /// Legacy contacts (no CEK) use storage-key encryption with plaintext
    /// display_name (existing behavior).
    pub fn save_contact(&self, contact: &Contact) -> Result<(), StorageError> {
        self.contacts().save_contact(contact)
    }
    /// Loads a contact by ID.
    pub fn load_contact(&self, id: &str) -> Result<Option<Contact>, StorageError> {
        self.contacts().load_contact(id)
    }
    /// Lists all contacts, excluding soft-deleted and archived contacts.
    pub fn list_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        self.contacts().list_contacts()
    }
    /// Lists contacts with pagination support.
    ///
    /// Returns contacts ordered by display_name, starting from `offset`
    /// and returning at most `limit` results.
    pub fn list_contacts_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Contact>, StorageError> {
        self.contacts().list_contacts_paginated(offset, limit)
    }
    /// Searches contacts by display name using case-insensitive matching.
    ///
    /// Returns all contacts whose display_name contains the query string.
    /// An empty query returns all contacts.
    ///
    /// Hybrid approach for performance:
    /// - Legacy contacts (non-empty display_name in DB): searched via SQL LIKE
    /// - CEK-protected contacts (empty display_name in DB): loaded, decrypted,
    ///   and filtered in memory
    pub fn search_contacts(&self, query: &str) -> Result<Vec<Contact>, StorageError> {
        self.contacts().search_contacts(query)
    }
    /// Lists contacts that are archived (but not soft-deleted).
    pub fn list_archived_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        self.contacts().list_archived_contacts()
    }
    /// Finds contact IDs that were soft-deleted before the given timestamp.
    ///
    /// Used by the garbage collector to find contacts eligible for permanent deletion.
    pub fn find_stale_soft_deletes(&self, older_than: u64) -> Result<Vec<String>, StorageError> {
        self.contacts().find_stale_soft_deletes(older_than)
    }
    /// Finds an imported contact by its original UID.
    ///
    /// Returns `Some(contact_id)` if a contact with the given UID exists,
    /// `None` otherwise. Only searches imported contacts (`contact_kind = 'imported'`).
    pub fn find_imported_by_uid(&self, uid: &str) -> Result<Option<String>, StorageError> {
        self.contacts().find_imported_by_uid(uid)
    }
    /// List contacts with a specific reciprocity status (e.g., "pending").
    ///
    /// Used by the relaunch recovery scan to find contacts whose
    /// reciprocity confirmation cascade should be resumed or expired.
    pub fn list_contacts_by_reciprocity(
        &self,
        reciprocity: &str,
    ) -> Result<Vec<Contact>, StorageError> {
        self.contacts().list_contacts_by_reciprocity(reciprocity)
    }
    /// Save encrypted confirmation state for crash recovery (design spec §5.1).
    pub fn update_confirmation_state(
        &self,
        contact_id: &str,
        state_bytes: &[u8],
    ) -> Result<(), StorageError> {
        self.contacts()
            .update_confirmation_state(contact_id, state_bytes)
    }
    /// Load and decrypt confirmation state for crash recovery.
    pub fn load_confirmation_state(
        &self,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.contacts().load_confirmation_state(contact_id)
    }
    /// Test-only: overwrite a single TEXT column on the `contacts` row
    /// for the given contact_id. Used to inject deserialization-failure
    /// inputs for the trust-input columns (site 8 of
    /// `2026-05-21-silent-failures-in-security-paths`): the public
    /// setters refuse garbage by type, so direct SQL is the only way
    /// to reach the parser in [`Storage::row_to_contact`].
    ///
    /// The column name is interpolated into the SQL string with an
    /// allow-list check so the helper cannot be turned into a SQL
    /// injection vector by a wandering caller.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_corrupt_contact_text_column(
        &self,
        contact_id: &str,
        column: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        self.contacts()
            .test_corrupt_contact_text_column(contact_id, column, value)
    }

    /// Deletes a contact by ID.
    pub fn delete_contact(&self, id: &str) -> Result<bool, StorageError> {
        // Clear relationship-scoped state that neither lives on the contacts
        // row (nickname/avatar/notes/cek drop with the row) nor cascades via
        // FK (contact_field_notes/contact_shared_names/contact_shared_avatars).
        // Without this these rows orphan; a stale contact_sync_timestamps row
        // in particular wrongly gates sync on contact_id reuse (read with
        // `.unwrap_or(0)` in sync/state.rs). See problem
        // 2026-06-01-contact-delete-orphans.
        self.conn.execute(
            "DELETE FROM contact_ratchets WHERE contact_id = ?1",
            params![id],
        )?;
        self.sync().forget_contact(id)?;
        self.delete_pending_updates_for_contact(id)?;
        self.delete_all_contact_overrides(id)?;
        self.conn.execute(
            "DELETE FROM dismissed_duplicates WHERE id1 = ?1 OR id2 = ?1",
            params![id],
        )?;

        let rows_affected = self
            .conn
            .execute("DELETE FROM contacts WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }
}
