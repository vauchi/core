// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Additional contact storage operations: personal notes, count/limits, own card,
//! sync timestamps, CEK ops, delta versions, revoked senders, dismissed duplicates.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::contact_card::ContactCard;
use crate::crypto::cek::ContentEncryptionKey;

impl Storage {
    /// Saves personal notes for a contact, encrypting at the storage layer.
    ///
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `personal_notes_encrypted` column.
    pub fn save_personal_notes(&self, contact_id: &str, notes: &[u8]) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(&self.encryption_key, notes)
            .map_err(|e| StorageError::Migration(format!("Encrypt personal notes: {}", e)))?;
        let rows_affected = self.conn.execute(
            "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
            params![encrypted, contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }

        Ok(())
    }

    /// Loads personal notes for a contact, decrypting at the storage layer.
    ///
    /// Returns decrypted plaintext bytes, or `None` if no notes are stored.
    /// Self-healing: if the stored data is legacy plaintext (pre-encryption gap),
    /// returns it as-is — the next save will encrypt it properly.
    pub fn load_personal_notes(&self, contact_id: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let result = self.conn.query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(Some(encrypted)) => {
                let plain =
                    crate::crypto::decrypt(&self.encryption_key, &encrypted).map_err(|e| {
                        StorageError::Encryption(format!("Decrypt personal notes: {}", e))
                    })?;
                Ok(Some(plain))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes personal notes for a contact.
    ///
    /// Sets the `personal_notes_encrypted` column to NULL.
    pub fn delete_personal_notes(&self, contact_id: &str) -> Result<(), StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE contacts SET personal_notes_encrypted = NULL WHERE id = ?1",
            params![contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }

        Ok(())
    }

    // Contact field notes: see storage/field_notes.rs

    /// Counts the total number of contacts in storage.
    pub fn count_contacts(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM contacts", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Returns the maximum number of contacts allowed.
    ///
    /// Reads from the `contact_limits` table (created by migration v4).
    /// Returns 10,000 as the default if no limit has been configured.
    pub fn get_contact_limit(&self) -> Result<usize, StorageError> {
        let result = self.conn.query_row(
            "SELECT max_contacts FROM contact_limits WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        );

        match result {
            Ok(limit) => Ok(limit as usize),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(10_000), // Default limit
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Sets the maximum number of contacts allowed.
    ///
    /// Updates the `contact_limits` table (created by migration v4).
    /// A limit of zero means no contacts are allowed.
    pub fn set_contact_limit(&self, max_contacts: usize) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_limits (id, max_contacts) VALUES (1, ?1)",
            params![max_contacts as i64],
        )?;
        Ok(())
    }

    /// Saves the user's own contact card (encrypted).
    pub fn save_own_card(&self, card: &ContactCard) -> Result<(), StorageError> {
        let card_json =
            serde_json::to_string(card).map_err(|e| StorageError::Serialization(e.to_string()))?;

        let card_encrypted = crate::crypto::encrypt(&self.encryption_key, card_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO own_card (id, card_json, card_json_encrypted, updated_at) VALUES (1, '', ?1, ?2)",
            params![card_encrypted, now as i64],
        )?;

        Ok(())
    }

    /// Loads the user's own contact card (decrypted).
    ///
    /// Reads from encrypted column first; falls back to plaintext for
    /// pre-v13 databases where migration hasn't populated the encrypted column.
    pub fn load_own_card(&self) -> Result<Option<ContactCard>, StorageError> {
        let result = self.conn.query_row(
            "SELECT card_json_encrypted, card_json FROM own_card WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let card = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(card))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                // Fallback: read from plaintext column (pre-v13 data)
                let card = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(card))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
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
        let ts_bytes = (timestamp as i64).to_le_bytes();
        let encrypted = crate::crypto::encrypt(&self.encryption_key, &ts_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_sync_timestamps (contact_id, last_sync_at, last_sync_at_encrypted)
             VALUES (?1, ?2, ?3)",
            params![contact_id, timestamp as i64, encrypted],
        )?;
        Ok(())
    }

    /// Gets the last sync timestamp for a contact (decrypted).
    ///
    /// Returns None if the contact hasn't been synced yet.
    pub fn get_contact_last_sync(&self, contact_id: &str) -> Result<Option<u64>, StorageError> {
        let result = self.conn.query_row(
            "SELECT last_sync_at_encrypted, last_sync_at FROM contact_sync_timestamps WHERE contact_id = ?1",
            params![contact_id],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: i64 = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                if decrypted.len() == 8 {
                    let ts = i64::from_le_bytes(
                        decrypted
                            .try_into()
                            .map_err(|_| StorageError::Encryption("Invalid timestamp".into()))?,
                    );
                    Ok(Some(ts as u64))
                } else {
                    Err(StorageError::Encryption("Invalid timestamp length".into()))
                }
            }
            Ok((_, plaintext)) => Ok(Some(plaintext as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Saves a CEK for a contact, encrypted with the storage master key.
    ///
    /// The CEK controls at-rest readability of the contact card (crypto-shredding).
    pub fn save_contact_cek(
        &self,
        contact_id: &str,
        cek: &ContentEncryptionKey,
    ) -> Result<(), StorageError> {
        let cek_encrypted = crate::crypto::encrypt(&self.encryption_key, &cek.to_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let rows_affected = self.conn.execute(
            "UPDATE contacts SET cek_encrypted = ?1 WHERE id = ?2",
            params![cek_encrypted, contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }

        Ok(())
    }

    /// Loads the CEK for a contact. Returns None for legacy contacts (pre-CEK).
    pub fn load_contact_cek(
        &self,
        contact_id: &str,
    ) -> Result<Option<ContentEncryptionKey>, StorageError> {
        let result = self.conn.query_row(
            "SELECT cek_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(Some(cek_encrypted)) => {
                let cek_bytes = crate::crypto::decrypt(&self.encryption_key, &cek_encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let cek_array: [u8; 32] = cek_bytes
                    .try_into()
                    .map_err(|_| StorageError::Encryption("Invalid CEK length".into()))?;
                Ok(Some(ContentEncryptionKey::from_bytes(cek_array)))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes the CEK for a contact (crypto-shredding).
    ///
    /// Sets `cek_encrypted` to NULL, rendering the card permanently unreadable
    /// if it was encrypted with the CEK.
    pub fn delete_contact_cek(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE contacts SET cek_encrypted = NULL WHERE id = ?1",
            params![contact_id],
        )?;
        Ok(())
    }

    /// Returns the last applied delta version for a contact (#42).
    ///
    /// Returns 0 if no version has been recorded (new or legacy contact).
    pub fn last_delta_version(&self, contact_id: &str) -> Result<u32, StorageError> {
        let version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(last_delta_version, 0) FROM contacts WHERE id = ?1",
                params![contact_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound("Contact not found".to_string())
                }
                other => StorageError::Database(other),
            })?;
        Ok(version as u32)
    }

    /// Records the last applied delta version for a contact (#42).
    pub fn record_delta_version(&self, contact_id: &str, version: u32) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE contacts SET last_delta_version = ?1 WHERE id = ?2",
            params![version as i64, contact_id],
        )?;
        Ok(())
    }

    /// Returns the last sent delta version for a contact.
    ///
    /// Returns 0 if no version has been sent (new contact).
    pub fn last_sent_delta_version(&self, contact_id: &str) -> Result<u32, StorageError> {
        let version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(last_sent_delta_version, 0) FROM contacts WHERE id = ?1",
                params![contact_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound("Contact not found".to_string())
                }
                other => StorageError::Database(other),
            })?;
        Ok(version as u32)
    }

    /// Records the last sent delta version for a contact.
    pub fn record_sent_delta_version(
        &self,
        contact_id: &str,
        version: u32,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE contacts SET last_sent_delta_version = ?1 WHERE id = ?2",
            params![version as i64, contact_id],
        )?;
        Ok(())
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
        self.conn.execute(
            "INSERT OR REPLACE INTO revoked_senders (sender_id, revoked_at) VALUES (?1, ?2)",
            params![sender_id, revoked_at as i64],
        )?;
        Ok(())
    }

    /// Checks if a sender has been revoked.
    pub fn is_sender_revoked(&self, sender_id: &str) -> Result<bool, StorageError> {
        let result = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM revoked_senders WHERE sender_id = ?1",
            params![sender_id],
            |row| row.get::<_, bool>(0),
        );

        match result {
            Ok(revoked) => Ok(revoked),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Records a dismissed duplicate pair.
    ///
    /// The pair is normalized so id1 < id2 lexicographically, ensuring
    /// (A, B) and (B, A) are stored identically.
    pub fn dismiss_duplicate(&self, id1: &str, id2: &str) -> Result<(), StorageError> {
        let (norm_id1, norm_id2) = crate::contact::merge::normalize_pair_key(id1, id2);
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR IGNORE INTO dismissed_duplicates (id1, id2, dismissed_at) VALUES (?1, ?2, ?3)",
            params![norm_id1, norm_id2, now as i64],
        )?;
        Ok(())
    }

    /// Loads all dismissed duplicate pairs.
    ///
    /// Returns a set of (id1, id2) tuples where id1 < id2 lexicographically.
    pub fn load_dismissed_duplicates(
        &self,
    ) -> Result<std::collections::HashSet<(String, String)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id1, id2 FROM dismissed_duplicates")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut dismissed = std::collections::HashSet::new();
        for row_result in rows {
            dismissed.insert(row_result?);
        }
        Ok(dismissed)
    }

    /// Removes a dismissed duplicate pair (e.g., when contacts are deleted).
    pub fn undismiss_duplicate(&self, id1: &str, id2: &str) -> Result<(), StorageError> {
        let (norm_id1, norm_id2) = crate::contact::merge::normalize_pair_key(id1, id2);
        self.conn.execute(
            "DELETE FROM dismissed_duplicates WHERE id1 = ?1 AND id2 = ?2",
            params![norm_id1, norm_id2],
        )?;
        Ok(())
    }
}
