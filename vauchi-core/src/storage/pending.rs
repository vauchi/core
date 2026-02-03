// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pending update storage operations.

use rusqlite::params;

use super::error::{PendingUpdate, UpdateStatus};
use super::{Storage, StorageError};

/// SQL columns selected for pending update queries (with encrypted payload).
const PENDING_SELECT: &str =
    "id, contact_id, update_type, payload_encrypted, payload, created_at, retry_count, status, error_message, retry_at";

/// Intermediate row before payload decryption.
struct PendingRow {
    id: String,
    contact_id: String,
    update_type: String,
    payload_encrypted: Option<Vec<u8>>,
    payload_plaintext: Vec<u8>,
    created_at: u64,
    retry_count: u32,
    status: UpdateStatus,
}

fn row_to_pending_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PendingRow> {
    let status_str: String = row.get(7)?;
    let error_msg: Option<String> = row.get(8)?;
    let retry_at: Option<i64> = row.get(9)?;

    let status = match status_str.as_str() {
        "pending" => UpdateStatus::Pending,
        "sending" => UpdateStatus::Sending,
        "failed" => UpdateStatus::Failed {
            error: error_msg.unwrap_or_default(),
            retry_at: retry_at.unwrap_or(0) as u64,
        },
        _ => UpdateStatus::Pending,
    };

    Ok(PendingRow {
        id: row.get(0)?,
        contact_id: row.get(1)?,
        update_type: row.get(2)?,
        payload_encrypted: row.get(3)?,
        payload_plaintext: row.get(4)?,
        created_at: row.get::<_, i64>(5)? as u64,
        retry_count: row.get::<_, i32>(6)? as u32,
        status,
    })
}

impl Storage {
    /// Decrypts a PendingRow's payload and converts to PendingUpdate.
    fn decrypt_pending_row(&self, row: PendingRow) -> Result<PendingUpdate, StorageError> {
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

        Ok(PendingUpdate {
            id: row.id,
            contact_id: row.contact_id,
            update_type: row.update_type,
            payload,
            created_at: row.created_at,
            retry_count: row.retry_count,
            status: row.status,
        })
    }

    // === Pending Updates Operations ===

    /// Queues a pending update for a contact (payload encrypted).
    pub fn queue_update(&self, update: &PendingUpdate) -> Result<(), StorageError> {
        let (status, error_msg, retry_at) = match &update.status {
            UpdateStatus::Pending => ("pending", None, None),
            UpdateStatus::Sending => ("sending", None, None),
            UpdateStatus::Failed { error, retry_at } => {
                ("failed", Some(error.as_str()), Some(*retry_at as i64))
            }
        };

        let payload_encrypted =
            crate::crypto::encrypt(&self.encryption_key, &update.payload)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO pending_updates
             (id, contact_id, update_type, payload, payload_encrypted, created_at, retry_count, status, error_message, retry_at)
             VALUES (?1, ?2, ?3, X'', ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                update.id,
                update.contact_id,
                update.update_type,
                payload_encrypted,
                update.created_at as i64,
                update.retry_count as i32,
                status,
                error_msg,
                retry_at,
            ],
        )?;

        Ok(())
    }

    /// Gets pending updates for a contact.
    pub fn get_pending_updates(
        &self,
        contact_id: &str,
    ) -> Result<Vec<PendingUpdate>, StorageError> {
        let sql = format!(
            "SELECT {} FROM pending_updates WHERE contact_id = ?1 ORDER BY created_at",
            PENDING_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<PendingRow> = stmt
            .query_map(params![contact_id], row_to_pending_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_pending_row(r))
            .collect()
    }

    /// Gets all pending updates.
    pub fn get_all_pending_updates(&self) -> Result<Vec<PendingUpdate>, StorageError> {
        let sql = format!(
            "SELECT {} FROM pending_updates ORDER BY created_at",
            PENDING_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<PendingRow> = stmt
            .query_map([], row_to_pending_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_pending_row(r))
            .collect()
    }

    /// Marks an update as sent (removes it from the queue).
    pub fn mark_update_sent(&self, update_id: &str) -> Result<bool, StorageError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM pending_updates WHERE id = ?1",
            params![update_id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Gets a single pending update by ID.
    pub fn get_pending_update(
        &self,
        update_id: &str,
    ) -> Result<Option<PendingUpdate>, StorageError> {
        let sql = format!(
            "SELECT {} FROM pending_updates WHERE id = ?1",
            PENDING_SELECT
        );
        let result = self.conn.query_row(&sql, params![update_id], row_to_pending_row);

        match result {
            Ok(row) => Ok(Some(self.decrypt_pending_row(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Updates the status of a pending update.
    pub fn update_pending_status(
        &self,
        update_id: &str,
        status: UpdateStatus,
        retry_count: u32,
    ) -> Result<bool, StorageError> {
        let (status_str, error_msg, retry_at) = match &status {
            UpdateStatus::Pending => ("pending", None, None),
            UpdateStatus::Sending => ("sending", None, None),
            UpdateStatus::Failed { error, retry_at } => {
                ("failed", Some(error.as_str()), Some(*retry_at as i64))
            }
        };

        let rows_affected = self.conn.execute(
            "UPDATE pending_updates SET status = ?1, error_message = ?2, retry_at = ?3, retry_count = ?4
             WHERE id = ?5",
            params![status_str, error_msg, retry_at, retry_count as i32, update_id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Counts pending updates for a contact.
    pub fn count_pending_updates(&self, contact_id: &str) -> Result<usize, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pending_updates WHERE contact_id = ?1",
            params![contact_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Deletes a pending update by ID.
    pub fn delete_pending_update(&self, id: &str) -> Result<bool, StorageError> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM pending_updates WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }

    /// Counts all pending updates across all contacts.
    pub fn count_all_pending_updates(&self) -> Result<usize, StorageError> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM pending_updates", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Deletes all pending updates for a contact.
    ///
    /// Returns the number of deleted updates.
    pub fn delete_pending_updates_for_contact(
        &self,
        contact_id: &str,
    ) -> Result<usize, StorageError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM pending_updates WHERE contact_id = ?1",
            params![contact_id],
        )?;
        Ok(rows_affected)
    }

    /// Clears all pending updates.
    ///
    /// Returns the number of deleted updates.
    pub fn clear_all_pending_updates(&self) -> Result<usize, StorageError> {
        let rows_affected = self.conn.execute("DELETE FROM pending_updates", [])?;
        Ok(rows_affected)
    }

    /// Gets pending updates by status.
    pub fn get_pending_updates_by_status(
        &self,
        status: &str,
    ) -> Result<Vec<PendingUpdate>, StorageError> {
        let sql = format!(
            "SELECT {} FROM pending_updates WHERE status = ?1 ORDER BY created_at",
            PENDING_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<PendingRow> = stmt
            .query_map(params![status], row_to_pending_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_pending_row(r))
            .collect()
    }
}
