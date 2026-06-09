// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`ActivityLogStore`](super::ActivityLogStore).

use super::Storage;

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

impl Storage {}
