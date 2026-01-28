// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Retry queue storage operations.

use rusqlite::params;

use super::error::RetryEntry;
use super::{Storage, StorageError};

impl Storage {
    // === Retry Queue Operations ===

    /// Creates a new retry entry.
    pub fn create_retry_entry(&self, entry: &RetryEntry) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO retry_entries
             (message_id, recipient_id, payload, attempt, next_retry, created_at, max_attempts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.message_id,
                entry.recipient_id,
                entry.payload,
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
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, payload, attempt, next_retry, created_at, max_attempts
             FROM retry_entries WHERE message_id = ?1",
        )?;

        let mut rows = stmt.query(params![message_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_retry_entry(row)?)),
            None => Ok(None),
        }
    }

    /// Gets all retry entries that are due for retry (next_retry <= now).
    pub fn get_due_retries(&self, now: u64) -> Result<Vec<RetryEntry>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, payload, attempt, next_retry, created_at, max_attempts
             FROM retry_entries WHERE next_retry <= ?1 ORDER BY next_retry",
        )?;

        let rows = stmt.query_map(params![now as i64], row_to_retry_entry)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Gets all retry entries for a recipient.
    pub fn get_retry_entries_for_recipient(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<RetryEntry>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, payload, attempt, next_retry, created_at, max_attempts
             FROM retry_entries WHERE recipient_id = ?1 ORDER BY created_at",
        )?;

        let rows = stmt.query_map(params![recipient_id], row_to_retry_entry)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Gets all retry entries.
    pub fn get_all_retry_entries(&self) -> Result<Vec<RetryEntry>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, payload, attempt, next_retry, created_at, max_attempts
             FROM retry_entries ORDER BY next_retry",
        )?;

        let rows = stmt.query_map([], row_to_retry_entry)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
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

/// Converts database row to RetryEntry.
fn row_to_retry_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RetryEntry> {
    Ok(RetryEntry {
        message_id: row.get(0)?,
        recipient_id: row.get(1)?,
        payload: row.get(2)?,
        attempt: row.get::<_, i32>(3)? as u32,
        next_retry: row.get::<_, i64>(4)? as u64,
        created_at: row.get::<_, i64>(5)? as u64,
        max_attempts: row.get::<_, i32>(6)? as u32,
    })
}
