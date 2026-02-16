// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoy Contact Storage
//!
//! CRUD operations for fake contacts displayed during duress mode.
//! Decoy contacts are stored in the `decoy_contacts` table (migration V21).

use rusqlite::params;

use super::{Storage, StorageError};
use crate::contact_card::ContactCard;

impl Storage {
    /// Saves a decoy contact.
    ///
    /// The card is encrypted with the storage key before persisting.
    /// Uses INSERT OR REPLACE for idempotent saves.
    pub fn save_decoy_contact(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
    ) -> Result<(), StorageError> {
        let card_json = serde_json::to_vec(card)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(&self.encryption_key, &card_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO decoy_contacts (id, display_name, card_encrypted, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, display_name, encrypted, now as i64, now as i64],
        )?;

        Ok(())
    }

    /// Loads all decoy contacts.
    ///
    /// Returns a list of (id, display_name, card) tuples.
    pub fn load_decoy_contacts(
        &self,
    ) -> Result<Vec<(String, String, ContactCard)>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, display_name, card_encrypted FROM decoy_contacts ORDER BY created_at",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;

        let mut contacts = Vec::new();
        for row in rows {
            let (id, display_name, encrypted) = row?;
            let card_json = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            contacts.push((id, display_name, card));
        }

        Ok(contacts)
    }

    /// Deletes a single decoy contact by ID.
    pub fn delete_decoy_contact(&self, id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM decoy_contacts WHERE id = ?1",
            params![id],
        )?;

        Ok(())
    }

    /// Deletes all decoy contacts.
    pub fn clear_all_decoy_contacts(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM decoy_contacts", [])?;

        Ok(())
    }
}
