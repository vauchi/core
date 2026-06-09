// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! FieldNote domain persistence view (contact_field_notes).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};

/// Scoped persistence view for the field_notes domain.
pub struct FieldNoteStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the field_notes domain.
    pub fn field_notes(&self) -> FieldNoteStore<'_> {
        FieldNoteStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl FieldNoteStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
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
        let encrypted = crate::crypto::encrypt(self.key, note)
            .map_err(|e| StorageError::Migration(format!("Encrypt field note: {}", e)))?;
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_field_notes
             (contact_id, field_id, note_encrypted, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![contact_id, field_id, encrypted, now as i64],
        )?;
        Ok(())
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
            let (field_id, encrypted) = row?;
            let plain = crate::crypto::decrypt(self.key, &encrypted)
                .map_err(|e| StorageError::Encryption(format!("Decrypt field note: {}", e)))?;
            map.insert(field_id, plain);
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
