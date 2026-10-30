// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Contact domain persistence view — operations (impl ContactStore).

use rusqlite::params;

use super::super::StorageError;
use super::ContactStore;
use crate::card::ContactCard;
use crate::crypto::cek::ContentEncryptionKey;

/// A revoked sender's tombstone. The key is the `sender_id`; the name stays
/// so a recovery claim for that key can still be named (vauchi/private#935).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokedSender {
    pub sender_id: String,
    pub revoked_at: u64,
    pub display_name: Option<String>,
}

impl ContactStore<'_> {
    /// Saves personal notes for a contact, encrypting at the storage layer.
    ///
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `personal_notes_encrypted` column.
    pub fn save_personal_notes(&self, contact_id: &str, notes: &[u8]) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, notes)
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
                let plain = crate::crypto::decrypt(self.key, &encrypted).map_err(|e| {
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
        self.own_card().save(card)
    }
    /// Loads the user's own contact card (decrypted).
    pub fn load_own_card(&self) -> Result<Option<ContactCard>, StorageError> {
        self.own_card().load()
    }
    /// Saves a CEK for a contact, encrypted with the storage master key.
    ///
    /// The CEK controls at-rest readability of the contact card (crypto-shredding).
    pub fn save_contact_cek(
        &self,
        contact_id: &str,
        cek: &ContentEncryptionKey,
    ) -> Result<(), StorageError> {
        let cek_encrypted = crate::crypto::encrypt(self.key, &cek.to_bytes())
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
                let cek_bytes = crate::crypto::decrypt(self.key, &cek_encrypted)
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
    /// Returns the last applied delta version for one of a contact's peer
    /// devices (#42).
    ///
    /// Each sender device numbers deltas from its own storage, so the
    /// stale-version floor is kept per (contact, peer device). Returns 0 when
    /// nothing has been recorded for the pair (new contact or fresh device).
    pub fn last_delta_version_for_device(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
    ) -> Result<u32, StorageError> {
        let result = self.conn.query_row(
            "SELECT last_version FROM contact_device_delta_versions
             WHERE contact_id = ?1 AND peer_device_id = ?2",
            params![contact_id, peer_device_id],
            |row| row.get::<_, i64>(0),
        );
        match result {
            Ok(version) => Ok(version as u32),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Records the last applied delta version for one of a contact's peer
    /// devices (#42).
    pub fn record_delta_version_for_device(
        &self,
        contact_id: &str,
        peer_device_id: &[u8; 32],
        version: u32,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_device_delta_versions
             (contact_id, peer_device_id, last_version)
             VALUES (?1, ?2, ?3)",
            params![contact_id, peer_device_id, version as i64],
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
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT last_sent_visible_fields FROM contacts WHERE id = ?1",
                params![contact_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound("Contact not found".to_string())
                }
                other => StorageError::Database(other),
            })?;
        match raw {
            None => Ok(None),
            Some(json) => Ok(Some(serde_json::from_str(&json).unwrap_or_default())),
        }
    }
    /// Records the set of field ids last sent (visible) to a contact (the
    /// baseline a later revocation is diffed against).
    pub fn save_last_sent_visible_fields(
        &self,
        contact_id: &str,
        fields: &std::collections::HashSet<String>,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(fields)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        self.conn.execute(
            "UPDATE contacts SET last_sent_visible_fields = ?1 WHERE id = ?2",
            params![json, contact_id],
        )?;
        Ok(())
    }
    /// The display name last sent (repropagated) to a contact, or `None` if no
    /// name has been sent yet. `repropagate_to_contact` uses this as the diff
    /// baseline so a rename emits a `DisplayNameChanged` exactly when it changes.
    pub fn load_last_sent_display_name(
        &self,
        contact_id: &str,
    ) -> Result<Option<String>, StorageError> {
        let encrypted: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT last_sent_display_name_encrypted FROM contacts WHERE id = ?1",
                params![contact_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound("Contact not found".to_string())
                }
                other => StorageError::Database(other),
            })?;
        encrypted
            .map(|ciphertext| {
                let plain = crate::crypto::decrypt(self.key, &ciphertext)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                String::from_utf8(plain).map_err(|e| StorageError::Serialization(e.to_string()))
            })
            .transpose()
    }
    /// Records the display name last sent (repropagated) to a contact.
    pub fn save_last_sent_display_name(
        &self,
        contact_id: &str,
        name: &str,
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, name.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "UPDATE contacts SET last_sent_display_name_encrypted = ?1 WHERE id = ?2",
            params![encrypted, contact_id],
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
    /// Records a revoked contact's tombstone with its display name, so a
    /// recovery claim for that key can still be named (vauchi/private#935).
    pub fn record_revoked_contact(
        &self,
        sender_id: &str,
        revoked_at: u64,
        display_name: &str,
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, display_name.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO revoked_senders (sender_id, revoked_at, display_name_encrypted)
             VALUES (?1, ?2, ?3)",
            params![sender_id, revoked_at as i64, encrypted],
        )?;
        Ok(())
    }

    /// The tombstone of a revoked sender, if any.
    pub fn revoked_sender(&self, sender_id: &str) -> Result<Option<RevokedSender>, StorageError> {
        let row = self.conn.query_row(
            "SELECT revoked_at, display_name_encrypted FROM revoked_senders WHERE sender_id = ?1",
            params![sender_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
        );
        let (revoked_at, encrypted) = match row {
            Ok(found) => found,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(StorageError::Database(e)),
        };
        let display_name = encrypted
            .map(|ciphertext| {
                let plain = crate::crypto::decrypt(self.key, &ciphertext)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                String::from_utf8(plain).map_err(|e| StorageError::Serialization(e.to_string()))
            })
            .transpose()?;
        Ok(Some(RevokedSender {
            sender_id: sender_id.to_string(),
            revoked_at: revoked_at as u64,
            display_name,
        }))
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
