// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
}
