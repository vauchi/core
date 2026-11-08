// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The activity log: what happened, as rows in a profile's own database.
//!
//! [`ActivityLogWriter`] converts [`VauchiEvent`]s into rows and writes them;
//! it is stateless — callers pass storage and events in. Each relevant event
//! maps to an [`ActivityLogEntry`], serialised as JSON and inserted with
//! `INSERT OR IGNORE` (deduplication via `event_key`); entries older than 7
//! days are pruned, and only the newly inserted entries are returned.

use serde::{Deserialize, Serialize};

#[cfg(feature = "network-rustls")]
use crate::api::VauchiEvent;
#[cfg(feature = "network-rustls")]
use crate::storage::{ActivityLogRow, Storage, StorageError};
use crate::types::EventOrigin;

/// Retention window: 7 days in seconds.
pub const RETENTION_SECS: u64 = 7 * 24 * 3600;

/// Stateless writer that converts `VauchiEvent`s into activity log rows.
#[cfg(feature = "network-rustls")]
pub struct ActivityLogWriter;

#[cfg(feature = "network-rustls")]
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

                let was_inserted = storage.activity_log().activity_log_insert(&row)?;
                if was_inserted {
                    inserted.push((event_key, entry));
                }
            }
        }

        storage
            .activity_log()
            .activity_log_prune(now_secs, RETENTION_SECS)?;

        Ok(inserted)
    }

    /// Maps a single `VauchiEvent` to an `(event_key, ActivityLogEntry)` pair.
    ///
    /// Returns `None` for event variants that are not tracked in the activity
    /// log.
    fn event_to_entry(event: &VauchiEvent, now_secs: u64) -> Option<(String, ActivityLogEntry)> {
        match event {
            VauchiEvent::ContactAdded {
                contact_id, origin, ..
            } => {
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

            // Alert keys use the signed nonce, never local time: surfacing is
            // at-least-once from durable facts, so a time-based key would log
            // every re-dispatch as a new entry (delivery-axis findings,
            // 2026-07-21-per-device-ratchet-registry-dormant).
            VauchiEvent::EmergencyAlertReceived {
                contact_id,
                alert_nonce,
                ..
            } => {
                let key = format!("alert:{contact_id}:{}", hex::encode(alert_nonce));
                let entry = ActivityLogEntry::EmergencyAlertReceived {
                    contact_id: contact_id.clone(),
                };
                Some((key, entry))
            }
            VauchiEvent::OwnCardUpdated { changed_fields } => {
                let fields_tag = changed_fields.join(",");
                let key = format!("own_card_updated:{fields_tag}:{now_secs}");
                let entry = ActivityLogEntry::OwnCardUpdated {
                    changed_fields: changed_fields.clone(),
                };
                Some((key, entry))
            }
            VauchiEvent::ContactRemoved { contact_id } => {
                let key = format!("contact_removed:{contact_id}:{now_secs}");
                let entry = ActivityLogEntry::ContactRemoved {
                    contact_id: contact_id.clone(),
                };
                Some((key, entry))
            }

            _ => None,
        }
    }
}

/// Activity log entry types stored in the payload column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(tag = "type")]
pub enum ActivityLogEntry {
    CardUpdateReceived {
        contact_id: String,
        changed_fields: Vec<String>,
    },
    CardUpdateDelivered {
        contact_id: String,
    },
    CardUpdatePending {
        contact_id: String,
    },
    CardUpdateFailed {
        contact_id: String,
        reason: String,
    },
    ContactAdded {
        contact_id: String,
        origin: EventOrigin,
    },
    EmergencyAlertReceived {
        contact_id: String,
    },
    OwnCardUpdated {
        changed_fields: Vec<String>,
    },
    ContactRemoved {
        contact_id: String,
    },
}

impl ActivityLogEntry {
    /// Returns the category string for the DB column.
    pub fn category_str(&self) -> &'static str {
        match self {
            Self::CardUpdateReceived { .. } => "card_update_received",
            Self::CardUpdateDelivered { .. } => "card_update_delivered",
            Self::CardUpdatePending { .. } => "card_update_pending",
            Self::CardUpdateFailed { .. } => "card_update_failed",
            Self::ContactAdded { .. } => "contact_added",
            Self::EmergencyAlertReceived { .. } => "emergency_alert_received",
            Self::OwnCardUpdated { .. } => "own_card_updated",
            Self::ContactRemoved { .. } => "contact_removed",
        }
    }

    /// Returns the contact_id from any variant.
    ///
    /// For `OwnCardUpdated`, returns "me".
    pub fn contact_id(&self) -> &str {
        match self {
            Self::CardUpdateReceived { contact_id, .. }
            | Self::CardUpdateDelivered { contact_id }
            | Self::CardUpdatePending { contact_id }
            | Self::CardUpdateFailed { contact_id, .. }
            | Self::ContactAdded { contact_id, .. }
            | Self::EmergencyAlertReceived { contact_id }
            | Self::ContactRemoved { contact_id } => contact_id,
            Self::OwnCardUpdated { .. } => "me",
        }
    }
}
