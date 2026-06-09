// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`ActivityLogStore`](super::ActivityLogStore).

use super::{Storage, StorageError};

/// A single row in the `activity_log` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityLogRow {
    /// Caller-generated deduplication key (PRIMARY KEY).
    pub event_key: String,
    /// Event category (e.g. "card_update", "exchange", "emergency").
    pub category: String,
    /// Associated contact ID, if any.
    pub contact_id: Option<String>,
    /// JSON-encoded event payload.
    pub payload: String,
    /// Unix timestamp (seconds) when the event occurred.
    pub created_at: u64,
}

impl Storage {
    /// Inserts an activity log entry.
    ///
    /// Uses `INSERT OR IGNORE` so duplicate `event_key` values are silently
    /// skipped. Returns `true` if the row was inserted, `false` if it was a
    /// duplicate.
    pub fn activity_log_insert(&self, row: &ActivityLogRow) -> Result<bool, StorageError> {
        self.activity_log().activity_log_insert(row)
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
        self.activity_log()
            .activity_log_query_recent(now_secs, max_age_secs)
    }
    /// Deletes activity log entries older than `max_age_secs` ago.
    ///
    /// Returns the number of rows deleted.
    pub fn activity_log_prune(
        &self,
        now_secs: u64,
        max_age_secs: u64,
    ) -> Result<usize, StorageError> {
        self.activity_log()
            .activity_log_prune(now_secs, max_age_secs)
    }
}
