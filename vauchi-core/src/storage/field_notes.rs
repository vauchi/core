// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`FieldNoteStore`](super::FieldNoteStore).

use super::{Storage, StorageError};

impl Storage {
    /// Saves a per-field note for a contact, encrypting at the storage layer.
    ///
    /// Inserts or replaces the note for the given `(contact_id, field_id)` pair.
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `note_encrypted` column.
    pub fn save_contact_field_note(
        &self,
        contact_id: &str,
        field_id: &str,
        note: &[u8],
    ) -> Result<(), StorageError> {
        self.field_notes()
            .save_contact_field_note(contact_id, field_id, note)
    }
    /// Loads all per-field notes for a contact, decrypting at the storage layer.
    ///
    /// Returns a `HashMap<field_id, plaintext_note>`. Returns an empty map if
    /// the contact has no field notes.
    /// Self-healing: legacy plaintext rows are returned as-is — the next save
    /// will encrypt them properly.
    pub fn load_contact_field_notes(
        &self,
        contact_id: &str,
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        self.field_notes().load_contact_field_notes(contact_id)
    }
    /// Deletes the encrypted note for a specific `(contact_id, field_id)` pair.
    ///
    /// No error is returned if the note does not exist (idempotent).
    pub fn delete_contact_field_note(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<(), StorageError> {
        self.field_notes()
            .delete_contact_field_note(contact_id, field_id)
    }
}
