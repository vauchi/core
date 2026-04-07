// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Converts [`VauchiEvent`]s into activity log rows and writes them to storage.
//!
//! `ActivityLogWriter` is stateless — callers pass storage and events in. The
//! writer maps each relevant event to an [`ActivityLogEntry`], serialises the
//! entry as JSON, inserts it with `INSERT OR IGNORE` (deduplication via
//! `event_key`), prunes entries older than 7 days, and returns only the
//! newly inserted entries.

use vauchi_core::Storage;
use vauchi_core::VauchiEvent;
use vauchi_core::storage::{ActivityLogRow, StorageError};

use crate::notification_types::ActivityLogEntry;

/// Retention window: 7 days in seconds.
const RETENTION_SECS: u64 = 7 * 24 * 3600;

/// Stateless writer that converts `VauchiEvent`s into activity log rows.
pub struct ActivityLogWriter;

impl ActivityLogWriter {
    /// Process `events`, write log entries, prune old entries.
    ///
    /// Returns the `(event_key, ActivityLogEntry)` pairs for entries that were
    /// newly inserted. Duplicate keys (already in the log) are silently skipped
    /// and not included in the return value.
    ///
    /// `now_secs` is the caller-supplied Unix timestamp (seconds) used for
    /// `created_at` and for computing the prune cutoff.  Passing an explicit
    /// value keeps tests deterministic (no `SystemTime::now` in tests).
    pub fn write(
        storage: &Storage,
        events: &[VauchiEvent],
        now_secs: u64,
    ) -> Result<Vec<(String, ActivityLogEntry)>, StorageError> {
        let mut inserted = Vec::new();

        for event in events {
            if let Some((event_key, entry)) = Self::event_to_entry(event, now_secs) {
                let payload = serde_json::to_string(&entry)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                let row = ActivityLogRow {
                    event_key: event_key.clone(),
                    category: entry.category_str().to_owned(),
                    contact_id: Some(entry.contact_id().to_owned()),
                    payload,
                    created_at: now_secs,
                };

                let was_inserted = storage.activity_log_insert(&row)?;
                if was_inserted {
                    inserted.push((event_key, entry));
                }
            }
        }

        storage.activity_log_prune(now_secs, RETENTION_SECS)?;

        Ok(inserted)
    }

    /// Maps a single `VauchiEvent` to an `(event_key, ActivityLogEntry)` pair.
    ///
    /// Returns `None` for event variants that are not tracked in the activity
    /// log.
    fn event_to_entry(event: &VauchiEvent, now_secs: u64) -> Option<(String, ActivityLogEntry)> {
        match event {
            VauchiEvent::ContactAdded { contact_id, origin } => {
                let key = format!("contact_added:{contact_id}");
                let entry = ActivityLogEntry::ContactAdded {
                    contact_id: contact_id.clone(),
                    origin: *origin,
                };
                Some((key, entry))
            }

            VauchiEvent::IncomingUpdate { contact_id } => {
                let key = format!("card_received:{contact_id}:{now_secs}");
                let entry = ActivityLogEntry::CardUpdateReceived {
                    contact_id: contact_id.clone(),
                    changed_fields: vec![],
                };
                Some((key, entry))
            }

            VauchiEvent::MessageDelivered {
                contact_id,
                message_id,
            } => {
                let key = format!("card_delivered:{contact_id}:{message_id}");
                let entry = ActivityLogEntry::CardUpdateDelivered {
                    contact_id: contact_id.clone(),
                };
                Some((key, entry))
            }

            VauchiEvent::MessageFailed { contact_id, error } => {
                let key = format!("card_failed:{contact_id}:{now_secs}");
                let entry = ActivityLogEntry::CardUpdateFailed {
                    contact_id: contact_id.clone(),
                    reason: error.clone(),
                };
                Some((key, entry))
            }

            VauchiEvent::EmergencyAlertReceived { contact_id, .. } => {
                let key = format!("emergency:{contact_id}:{now_secs}");
                let entry = ActivityLogEntry::EmergencyAlertReceived {
                    contact_id: contact_id.clone(),
                };
                Some((key, entry))
            }

            _ => None,
        }
    }
}
