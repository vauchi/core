// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery domain persistence view.
//!
//! `RecoveryStore` is a scoped, zero-cost view over the shared storage
//! connection: a function handed a `&RecoveryStore` can reach the recovery
//! tables and nothing else. Constructed only via [`Storage::recovery`], which
//! is the sole holder of the connection and encryption key. See problem record
//! `2026-06-09-storage-per-domain-store-boundaries` (Phase 0).

use std::sync::Arc;

use rusqlite::{Connection, params};

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use crate::recovery::{RecoveryProgress, RecoverySettings};

/// Scoped persistence view for the recovery domain (responses, rate limits,
/// settings, in-progress state).
pub struct RecoveryStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the recovery domain.
    ///
    /// The returned [`RecoveryStore`] borrows this storage's connection,
    /// encryption key, and clock — it cannot outlive `self` and cannot reach
    /// any other domain's tables.
    pub fn recovery(&self) -> RecoveryStore<'_> {
        RecoveryStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

/// A vouched claim's old and new signing key, `(old_pk, new_pk)`.
pub type VouchedKeys = ([u8; 32], [u8; 32]);

impl RecoveryStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }

    /// Saves a recovery response to storage (encrypted).
    ///
    /// Records the user's response (accept, reject, or remind_me_later) to
    /// a recovery claim. The response is stored with a unique constraint on
    /// claim_id, so subsequent calls overwrite previous responses.
    pub fn save_recovery_response(
        &self,
        claim_id: &str,
        contact_id: &str,
        response: &str,
        remind_at: Option<u64>,
    ) -> Result<(), StorageError> {
        let response_encrypted = crate::crypto::encrypt(self.key, response.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_responses
             (claim_id, contact_id, response, response_encrypted, remind_at, created_at)
             VALUES (?1, ?2, '', ?3, ?4, ?5)",
            params![
                claim_id,
                contact_id,
                response_encrypted,
                remind_at.map(|t| t as i64),
                now as i64,
            ],
        )?;

        Ok(())
    }

    /// Retrieves a recovery response by claim ID (decrypted).
    ///
    /// Returns `Ok(Some((contact_id, response, remind_at)))` if found,
    /// or `Ok(None)` if no response exists for the given claim.
    pub fn get_recovery_response(
        &self,
        claim_id: &str,
    ) -> Result<Option<(String, String, Option<u64>)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT contact_id, response_encrypted, response, remind_at
             FROM recovery_responses
             WHERE claim_id = ?1",
            params![claim_id],
            |row| {
                let contact_id: String = row.get(0)?;
                let encrypted: Option<Vec<u8>> = row.get(1)?;
                let plaintext: String = row.get(2)?;
                let remind_at: Option<i64> = row.get(3)?;
                Ok((
                    contact_id,
                    encrypted,
                    plaintext,
                    remind_at.map(|t| t as u64),
                ))
            },
        );

        match result {
            Ok((contact_id, Some(encrypted), _, remind_at)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let response = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some((contact_id, response, remind_at)))
            }
            Ok((contact_id, _, plaintext, remind_at)) if !plaintext.is_empty() => {
                Ok(Some((contact_id, plaintext, remind_at)))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Checks the recovery rate limit for a given identity public key.
    ///
    /// Returns `(count, window_start)` where count is the number of claims
    /// in the current window, and window_start is the Unix timestamp when
    /// the window began. Returns `(0, 0)` if no rate limit record exists.
    pub fn check_recovery_rate_limit(
        &self,
        identity_pk: &[u8],
    ) -> Result<(u32, u64), StorageError> {
        let result = self.conn.query_row(
            "SELECT claim_count, window_start
             FROM recovery_rate_limits
             WHERE identity_pk = ?1",
            params![identity_pk],
            |row| {
                let count: i32 = row.get(0)?;
                let window_start: i64 = row.get(1)?;
                Ok((count as u32, window_start as u64))
            },
        );

        match result {
            Ok(record) => Ok(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok((0, 0)),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Updates (or inserts) the recovery rate limit for a given identity public key.
    ///
    /// This upserts the rate limit record, setting the claim count and window
    /// start timestamp for the given identity.
    pub fn update_recovery_rate_limit(
        &self,
        identity_pk: &[u8],
        count: u32,
        window_start: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_rate_limits
             (identity_pk, claim_count, window_start)
             VALUES (?1, ?2, ?3)",
            params![identity_pk, count as i32, window_start as i64],
        )?;

        Ok(())
    }

    /// Saves recovery settings to storage (encrypted).
    pub fn save_recovery_settings(&self, settings: &RecoverySettings) -> Result<(), StorageError> {
        let json =
            serde_json::to_vec(settings).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, &json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_settings (id, settings_encrypted, updated_at)
             VALUES (1, ?1, ?2)",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Loads recovery settings from storage.
    ///
    /// Returns `None` if no settings have been saved, in which case callers
    /// should use `RecoverySettings::default()`.
    pub fn load_recovery_settings(&self) -> Result<Option<RecoverySettings>, StorageError> {
        let result = self.conn.query_row(
            "SELECT settings_encrypted FROM recovery_settings WHERE id = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        );

        match result {
            Ok(encrypted) => {
                let json = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let settings: RecoverySettings = serde_json::from_slice(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(settings))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Saves in-progress recovery state (encrypted).
    ///
    /// Only one recovery can be in progress at a time. Subsequent calls
    /// overwrite the previous state.
    pub fn save_recovery_progress(&self, progress: &RecoveryProgress) -> Result<(), StorageError> {
        let json =
            serde_json::to_vec(progress).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, &json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_progress (id, progress_encrypted, updated_at)
             VALUES (1, ?1, ?2)",
            params![encrypted, self.now_secs() as i64,],
        )?;
        Ok(())
    }

    /// Loads in-progress recovery state, if any.
    pub fn load_recovery_progress(&self) -> Result<Option<RecoveryProgress>, StorageError> {
        let result = self.conn.query_row(
            "SELECT progress_encrypted FROM recovery_progress WHERE id = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        );

        match result {
            Ok(encrypted) => {
                let json = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let progress: RecoveryProgress = serde_json::from_slice(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(progress))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes in-progress recovery state (recovery completed or abandoned).
    pub fn clear_recovery_progress(&self) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM recovery_progress WHERE id = 1", [])?;
        Ok(())
    }

    /// Records that this device vouched for a claim from `old_pk` to
    /// `new_pk`, so it may hand the guardian backup back to `new_pk`
    /// (vauchi/private#796 step 2b). A second vouch moves the time.
    ///
    /// # Errors
    /// Storage errors.
    pub fn record_vouched_claim(
        &self,
        old_pk: &[u8; 32],
        new_pk: &[u8; 32],
        vouched_at: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO vouched_claims (old_pk, new_pk, vouched_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(old_pk, new_pk) DO UPDATE SET vouched_at = excluded.vouched_at",
            params![
                &old_pk[..],
                &new_pk[..],
                i64::try_from(vouched_at).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// The claims vouched for at or after `not_before` whose hand-back has
    /// not gone out yet, as `(old_pk, new_pk)` (step 2c).
    ///
    /// # Errors
    /// Storage errors; [`StorageError::InvalidData`] for a stored key that
    /// is not 32 bytes.
    pub fn owed_hand_backs(&self, not_before: u64) -> Result<Vec<VouchedKeys>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT old_pk, new_pk FROM vouched_claims
             WHERE handed_back_at IS NULL AND vouched_at >= ?1 ORDER BY vouched_at",
        )?;
        let rows = statement
            .query_map(
                params![i64::try_from(not_before).unwrap_or(i64::MAX)],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(old, new)| {
                let key = |bytes: Vec<u8>| {
                    <[u8; 32]>::try_from(bytes)
                        .map_err(|_| StorageError::InvalidData("vouched claim key".into()))
                };
                Ok((key(old)?, key(new)?))
            })
            .collect()
    }

    /// Puts `contact_id` in guardian group `group_id` (1 to 9), moving it
    /// from any group it was in (D17).
    ///
    /// # Errors
    /// Storage errors, a group outside 1 to 9 among them.
    pub fn set_guardian_group(&self, contact_id: &str, group_id: u8) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO guardian_groups (contact_id, group_id) VALUES (?1, ?2)
             ON CONFLICT(contact_id) DO UPDATE SET group_id = excluded.group_id",
            params![contact_id, group_id],
        )?;
        Ok(())
    }

    /// Takes `contact_id` out of its guardian group.
    ///
    /// # Errors
    /// Storage errors.
    pub fn remove_guardian_group(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM guardian_groups WHERE contact_id = ?1",
            params![contact_id],
        )?;
        Ok(())
    }

    /// The guardian groups by number, each with its members' contact ids
    /// in the order they joined.
    ///
    /// # Errors
    /// Storage errors.
    pub fn guardian_groups(&self) -> Result<Vec<(u8, Vec<String>)>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT group_id, contact_id FROM guardian_groups ORDER BY group_id, rowid")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, u8>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut groups: Vec<(u8, Vec<String>)> = Vec::new();
        for (group_id, contact_id) in rows {
            match groups.last_mut() {
                Some((last, members)) if *last == group_id => members.push(contact_id),
                _ => groups.push((group_id, vec![contact_id])),
            }
        }
        Ok(groups)
    }

    /// Records that the hand-back for the claim from `old_pk` to `new_pk`
    /// went out.
    ///
    /// # Errors
    /// Storage errors.
    pub fn mark_handed_back(
        &self,
        old_pk: &[u8; 32],
        new_pk: &[u8; 32],
        at: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE vouched_claims SET handed_back_at = ?3 WHERE old_pk = ?1 AND new_pk = ?2",
            params![
                &old_pk[..],
                &new_pk[..],
                i64::try_from(at).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// The old key of the latest claim this device vouched for whose new
    /// key is `new_pk`, vouched at or after `not_before`.
    ///
    /// # Errors
    /// Storage errors; [`StorageError::InvalidData`] for a stored key that
    /// is not 32 bytes.
    pub fn vouched_old_key_for(
        &self,
        new_pk: &[u8; 32],
        not_before: u64,
    ) -> Result<Option<[u8; 32]>, StorageError> {
        use rusqlite::OptionalExtension;
        let old_pk: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT old_pk FROM vouched_claims WHERE new_pk = ?1 AND vouched_at >= ?2
                 ORDER BY vouched_at DESC LIMIT 1",
                params![&new_pk[..], i64::try_from(not_before).unwrap_or(i64::MAX)],
                |row| row.get(0),
            )
            .optional()?;
        old_pk
            .map(|key| {
                <[u8; 32]>::try_from(key)
                    .map_err(|_| StorageError::InvalidData("vouched claim key".into()))
            })
            .transpose()
    }

    /// Keeps the owner's recovery passphrase as its salt and derived key
    /// (the key encrypted at rest), replacing any earlier one.
    pub fn save_recovery_passphrase(
        &self,
        salt: &[u8; 16],
        key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let key_encrypted = crate::crypto::encrypt(self.key, key.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_passphrase (id, salt, key_encrypted)
             VALUES (1, ?1, ?2)",
            params![&salt[..], key_encrypted],
        )?;
        Ok(())
    }

    /// The owner's recovery passphrase salt and key, if one is set.
    ///
    /// # Errors
    /// Storage errors; [`StorageError::InvalidData`] for a malformed row.
    pub fn load_recovery_passphrase(
        &self,
    ) -> Result<Option<([u8; 16], SymmetricKey)>, StorageError> {
        use rusqlite::OptionalExtension;
        use zeroize::Zeroize;
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT salt, key_encrypted FROM recovery_passphrase WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((salt, key_encrypted)) = row else {
            return Ok(None);
        };
        let salt = <[u8; 16]>::try_from(salt)
            .map_err(|_| StorageError::InvalidData("recovery passphrase salt".into()))?;
        let mut plain = crate::crypto::decrypt(self.key, &key_encrypted)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let bytes = <[u8; 32]>::try_from(plain.as_slice())
            .map_err(|_| StorageError::InvalidData("recovery passphrase key".into()));
        plain.zeroize();
        Ok(Some((salt, SymmetricKey::from_bytes(bytes?))))
    }

    /// Marks a contact restored from the guardian backup as waiting for its
    /// rekey to the new identity; marking it again changes nothing.
    pub fn mark_rekey_pending(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO recovery_rekey_pending (contact_id, restored_at)
             VALUES (?1, ?2)",
            params![
                contact_id,
                i64::try_from(self.now_secs()).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// The contacts still waiting for their rekey, in the order restored.
    pub fn rekey_pending(&self) -> Result<Vec<String>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id FROM recovery_rekey_pending ORDER BY restored_at, rowid")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }

    /// Forgets the owner's recovery passphrase.
    pub fn clear_recovery_passphrase(&self) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM recovery_passphrase WHERE id = 1", [])?;
        Ok(())
    }
}
