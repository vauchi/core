// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pending update storage operations.

use rusqlite::params;

use super::error::{PendingUpdate, UpdateStatus};
use super::{Storage, StorageError};

impl Storage {
    // === Pending Updates Operations ===

    /// Queues a pending update for a contact.
    pub fn queue_update(&self, update: &PendingUpdate) -> Result<(), StorageError> {
        let (status, error_msg, retry_at) = match &update.status {
            UpdateStatus::Pending => ("pending", None, None),
            UpdateStatus::Sending => ("sending", None, None),
            UpdateStatus::Failed { error, retry_at } => {
                ("failed", Some(error.as_str()), Some(*retry_at as i64))
            }
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO pending_updates
             (id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                update.id,
                update.contact_id,
                update.update_type,
                update.payload,
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
        let mut stmt = self.conn.prepare(
            "SELECT id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at
             FROM pending_updates WHERE contact_id = ?1 ORDER BY created_at"
        )?;

        let rows = stmt.query_map(params![contact_id], |row| {
            let status_str: String = row.get(6)?;
            let error_msg: Option<String> = row.get(7)?;
            let retry_at: Option<i64> = row.get(8)?;

            let status = match status_str.as_str() {
                "pending" => UpdateStatus::Pending,
                "sending" => UpdateStatus::Sending,
                "failed" => UpdateStatus::Failed {
                    error: error_msg.unwrap_or_default(),
                    retry_at: retry_at.unwrap_or(0) as u64,
                },
                _ => UpdateStatus::Pending,
            };

            Ok(PendingUpdate {
                id: row.get(0)?,
                contact_id: row.get(1)?,
                update_type: row.get(2)?,
                payload: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                retry_count: row.get::<_, i32>(5)? as u32,
                status,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Gets all pending updates.
    pub fn get_all_pending_updates(&self) -> Result<Vec<PendingUpdate>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at
             FROM pending_updates ORDER BY created_at"
        )?;

        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(6)?;
            let error_msg: Option<String> = row.get(7)?;
            let retry_at: Option<i64> = row.get(8)?;

            let status = match status_str.as_str() {
                "pending" => UpdateStatus::Pending,
                "sending" => UpdateStatus::Sending,
                "failed" => UpdateStatus::Failed {
                    error: error_msg.unwrap_or_default(),
                    retry_at: retry_at.unwrap_or(0) as u64,
                },
                _ => UpdateStatus::Pending,
            };

            Ok(PendingUpdate {
                id: row.get(0)?,
                contact_id: row.get(1)?,
                update_type: row.get(2)?,
                payload: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                retry_count: row.get::<_, i32>(5)? as u32,
                status,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
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
        let result = self.conn.query_row(
            "SELECT id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at
             FROM pending_updates WHERE id = ?1",
            params![update_id],
            |row| {
                let status_str: String = row.get(6)?;
                let error_msg: Option<String> = row.get(7)?;
                let retry_at: Option<i64> = row.get(8)?;

                let status = match status_str.as_str() {
                    "pending" => UpdateStatus::Pending,
                    "sending" => UpdateStatus::Sending,
                    "failed" => UpdateStatus::Failed {
                        error: error_msg.unwrap_or_default(),
                        retry_at: retry_at.unwrap_or(0) as u64,
                    },
                    _ => UpdateStatus::Pending,
                };

                Ok(PendingUpdate {
                    id: row.get(0)?,
                    contact_id: row.get(1)?,
                    update_type: row.get(2)?,
                    payload: row.get(3)?,
                    created_at: row.get::<_, i64>(4)? as u64,
                    retry_count: row.get::<_, i32>(5)? as u32,
                    status,
                })
            },
        );

        match result {
            Ok(update) => Ok(Some(update)),
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
        let mut stmt = self.conn.prepare(
            "SELECT id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at
             FROM pending_updates WHERE status = ?1 ORDER BY created_at"
        )?;

        let rows = stmt.query_map(params![status], |row| {
            let status_str: String = row.get(6)?;
            let error_msg: Option<String> = row.get(7)?;
            let retry_at: Option<i64> = row.get(8)?;

            let status = match status_str.as_str() {
                "pending" => UpdateStatus::Pending,
                "sending" => UpdateStatus::Sending,
                "failed" => UpdateStatus::Failed {
                    error: error_msg.unwrap_or_default(),
                    retry_at: retry_at.unwrap_or(0) as u64,
                },
                _ => UpdateStatus::Pending,
            };

            Ok(PendingUpdate {
                id: row.get(0)?,
                contact_id: row.get(1)?,
                update_type: row.get(2)?,
                payload: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                retry_count: row.get::<_, i32>(5)? as u32,
                status,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }
}
