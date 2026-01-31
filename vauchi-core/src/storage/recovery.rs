// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery storage operations.
//!
//! Provides persistence for recovery responses and rate limiting data.

use rusqlite::params;

use super::{Storage, StorageError};

impl Storage {
    // === Recovery Response Operations ===

    /// Saves a recovery response to storage.
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_responses
             (claim_id, contact_id, response, remind_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                claim_id,
                contact_id,
                response,
                remind_at.map(|t| t as i64),
                now as i64,
            ],
        )?;

        Ok(())
    }

    /// Retrieves a recovery response by claim ID.
    ///
    /// Returns `Ok(Some((contact_id, response, remind_at)))` if found,
    /// or `Ok(None)` if no response exists for the given claim.
    pub fn get_recovery_response(
        &self,
        claim_id: &str,
    ) -> Result<Option<(String, String, Option<u64>)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT contact_id, response, remind_at
             FROM recovery_responses
             WHERE claim_id = ?1",
            params![claim_id],
            |row| {
                let contact_id: String = row.get(0)?;
                let response: String = row.get(1)?;
                let remind_at: Option<i64> = row.get(2)?;
                Ok((contact_id, response, remind_at.map(|t| t as u64)))
            },
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Recovery Rate Limit Operations ===

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
}
