// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Retry queue storage operations.

use rusqlite::params;

use super::error::RetryEntry;
use super::{Storage, StorageError};

/// SQL columns selected for retry entry queries (with encrypted payload).
const RETRY_SELECT: &str = "message_id, recipient_id, payload_encrypted, payload, attempt, next_retry, created_at, max_attempts";

/// Intermediate row before payload decryption.
struct RetryRow {
    message_id: String,
    recipient_id: String,
    payload_encrypted: Option<Vec<u8>>,
    payload_plaintext: Vec<u8>,
    attempt: u32,
    next_retry: u64,
    created_at: u64,
    max_attempts: u32,
}

fn row_to_retry_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RetryRow> {
    Ok(RetryRow {
        message_id: row.get(0)?,
        recipient_id: row.get(1)?,
        payload_encrypted: row.get(2)?,
        payload_plaintext: row.get(3)?,
        attempt: row.get::<_, i32>(4)? as u32,
        next_retry: row.get::<_, i64>(5)? as u64,
        created_at: row.get::<_, i64>(6)? as u64,
        max_attempts: row.get::<_, i32>(7)? as u32,
    })
}

impl Storage {
    /// Decrypts a RetryRow's payload and converts to RetryEntry.
    fn decrypt_retry_row(&self, row: RetryRow) -> Result<RetryEntry, StorageError> {
        let payload = if let Some(enc) = row.payload_encrypted {
            if !enc.is_empty() {
                crate::crypto::decrypt(&self.encryption_key, &enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?
            } else {
                row.payload_plaintext
            }
        } else {
            row.payload_plaintext
        };

        Ok(RetryEntry {
            message_id: row.message_id,
            recipient_id: row.recipient_id,
            payload,
            attempt: row.attempt,
            next_retry: row.next_retry,
            created_at: row.created_at,
            max_attempts: row.max_attempts,
        })
    }

    /// Creates a new retry entry (payload encrypted).
    pub fn create_retry_entry(&self, entry: &RetryEntry) -> Result<(), StorageError> {
        let payload_encrypted = crate::crypto::encrypt(&self.encryption_key, &entry.payload)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT INTO retry_entries
             (message_id, recipient_id, payload, payload_encrypted, attempt, next_retry, created_at, max_attempts)
             VALUES (?1, ?2, X'', ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.message_id,
                entry.recipient_id,
                payload_encrypted,
                entry.attempt as i32,
                entry.next_retry as i64,
                entry.created_at as i64,
                entry.max_attempts as i32,
            ],
        )?;

        Ok(())
    }

    /// Gets a retry entry by message ID.
    pub fn get_retry_entry(&self, message_id: &str) -> Result<Option<RetryEntry>, StorageError> {
        let sql = format!(
            "SELECT {} FROM retry_entries WHERE message_id = ?1",
            RETRY_SELECT
        );
        let result = self
            .conn
            .query_row(&sql, params![message_id], row_to_retry_row);

        match result {
            Ok(row) => Ok(Some(self.decrypt_retry_row(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Gets all retry entries that are due for retry (next_retry <= now).
    pub fn get_due_retries(&self, now: u64) -> Result<Vec<RetryEntry>, StorageError> {
        let sql = format!(
            "SELECT {} FROM retry_entries WHERE next_retry <= ?1 ORDER BY next_retry",
            RETRY_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<RetryRow> = stmt
            .query_map(params![now as i64], row_to_retry_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_retry_row(r))
            .collect()
    }

    /// Gets all retry entries for a recipient.
    pub fn get_retry_entries_for_recipient(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<RetryEntry>, StorageError> {
        let sql = format!(
            "SELECT {} FROM retry_entries WHERE recipient_id = ?1 ORDER BY created_at",
            RETRY_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<RetryRow> = stmt
            .query_map(params![recipient_id], row_to_retry_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_retry_row(r))
            .collect()
    }

    /// Gets all retry entries.
    pub fn get_all_retry_entries(&self) -> Result<Vec<RetryEntry>, StorageError> {
        let sql = format!(
            "SELECT {} FROM retry_entries ORDER BY next_retry",
            RETRY_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<RetryRow> = stmt
            .query_map([], row_to_retry_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_retry_row(r))
            .collect()
    }

    /// Increments the retry attempt count and updates the next retry time.
    pub fn increment_retry_attempt(
        &self,
        message_id: &str,
        next_retry: u64,
    ) -> Result<bool, StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE retry_entries SET attempt = attempt + 1, next_retry = ?1
             WHERE message_id = ?2",
            params![next_retry as i64, message_id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Deletes a retry entry.
    pub fn delete_retry_entry(&self, message_id: &str) -> Result<bool, StorageError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM retry_entries WHERE message_id = ?1",
            params![message_id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Counts the total number of retry entries.
    pub fn count_retry_entries(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM retry_entries", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Updates the next retry time for an entry (for manual retry).
    pub fn update_retry_next_time(
        &self,
        message_id: &str,
        next_retry: u64,
    ) -> Result<bool, StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE retry_entries SET next_retry = ?1 WHERE message_id = ?2",
            params![next_retry as i64, message_id],
        )?;
        Ok(rows_affected > 0)
    }
}
