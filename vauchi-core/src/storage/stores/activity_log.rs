// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ActivityLog domain persistence view (activity_log).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use rusqlite::{Connection, params};

use super::super::activity_log::ActivityLogRow;
use super::super::{Storage, StorageError};

/// Scoped persistence view for the activity_log domain.
pub struct ActivityLogStore<'a> {
    conn: &'a Connection,
}

impl Storage {
    /// Scoped persistence view for the activity_log domain.
    pub fn activity_log(&self) -> ActivityLogStore<'_> {
        ActivityLogStore { conn: &self.conn }
    }
}

impl ActivityLogStore<'_> {
    /// Inserts an activity log entry.
    ///
    /// Uses `INSERT OR IGNORE` so duplicate `event_key` values are silently
    /// skipped. Returns `true` if the row was inserted, `false` if it was a
    /// duplicate.
    pub fn activity_log_insert(&self, row: &ActivityLogRow) -> Result<bool, StorageError> {
        let changes = self.conn.execute(
            "INSERT OR IGNORE INTO activity_log
             (event_key, category, contact_id, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                row.event_key,
                row.category,
                row.contact_id,
                row.payload,
                row.created_at as i64,
            ],
        )?;
        Ok(changes > 0)
    }
    /// Returns activity log entries newer than `max_age_secs` ago.
    ///
    /// Results are ordered by `created_at DESC` (newest first). The cutoff is
    /// computed as `now - max_age_secs` where `now` is provided by the caller
    /// to keep tests deterministic.
    pub fn activity_log_query_recent(
        &self,
        now_secs: u64,
        max_age_secs: u64,
    ) -> Result<Vec<ActivityLogRow>, StorageError> {
        let cutoff = now_secs.saturating_sub(max_age_secs) as i64;

        let mut stmt = self.conn.prepare(
            "SELECT event_key, category, contact_id, payload, created_at
             FROM activity_log
             WHERE created_at >= ?1
             ORDER BY created_at DESC",
        )?;

        let rows = stmt
            .query_map(params![cutoff], |row| {
                Ok(ActivityLogRow {
                    event_key: row.get(0)?,
                    category: row.get(1)?,
                    contact_id: row.get(2)?,
                    payload: row.get(3)?,
                    created_at: row.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }
    /// Deletes activity log entries older than `max_age_secs` ago.
    ///
    /// Returns the number of rows deleted.
    pub fn activity_log_prune(
        &self,
        now_secs: u64,
        max_age_secs: u64,
    ) -> Result<usize, StorageError> {
        let cutoff = now_secs.saturating_sub(max_age_secs) as i64;

        let changes = self.conn.execute(
            "DELETE FROM activity_log WHERE created_at < ?1",
            params![cutoff],
        )?;

        Ok(changes)
    }
}
