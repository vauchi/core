// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-field private note storage operations.
//!
//! Stores encrypted notes on contacts' shared fields in the `contact_field_notes`
//! table (migration V32). Notes are private ("your eyes only"), synced to own
//! linked devices, never shared with the contact.

use rusqlite::params;

use super::{Storage, StorageError};

impl Storage {
    /// Saves an encrypted per-field note for a contact.
    ///
    /// Inserts or replaces the note for the given `(contact_id, field_id)` pair.
    /// The caller is responsible for encrypting the note before passing it in.
    // TODO(security): note_encrypted column accepts raw bytes — callers should encrypt
    // with the storage material key before calling. Same gap as personal_notes_encrypted.
    pub fn save_contact_field_note(
        &self,
        contact_id: &str,
        field_id: &str,
        note_encrypted: &[u8],
    ) -> Result<(), StorageError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_field_notes
             (contact_id, field_id, note_encrypted, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![contact_id, field_id, note_encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Loads all encrypted per-field notes for a contact.
    ///
    /// Returns a `HashMap<field_id, note_encrypted>`. Returns an empty map if
    /// the contact has no field notes.
    pub fn load_contact_field_notes(
        &self,
        contact_id: &str,
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT field_id, note_encrypted
             FROM contact_field_notes
             WHERE contact_id = ?1",
        )?;
        let rows = stmt.query_map(params![contact_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (field_id, note) = row?;
            map.insert(field_id, note);
        }
        Ok(map)
    }

    /// Deletes the encrypted note for a specific `(contact_id, field_id)` pair.
    ///
    /// No error is returned if the note does not exist (idempotent).
    pub fn delete_contact_field_note(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM contact_field_notes
             WHERE contact_id = ?1 AND field_id = ?2",
            params![contact_id, field_id],
        )?;
        Ok(())
    }
}
