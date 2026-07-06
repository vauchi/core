// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Manager
//!
//! Coordinates update delivery with offline queuing and retry logic,
//! tracking sync state per contact.

use std::collections::HashMap;

use thiserror::Error;
use uuid::Uuid;

use crate::contact_card::ContactCard;
use crate::storage::{PendingUpdate, Storage, StorageError, UpdateStatus};
use crate::sync::SyncState;
use crate::sync::delta::CardDelta;

/// Sync error types.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SyncError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Contact not found: {0}")]
    ContactNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("No changes to sync")]
    NoChanges,
}

/// Manages synchronization operations for all contacts.
///
/// The SyncManager coordinates update delivery, handles offline queuing,
/// and tracks sync state per contact. Also tracks the last applied delta
/// version per contact for downgrade detection.
pub struct SyncManager<'a> {
    storage: &'a Storage,
    /// Tracks the last applied delta version per contact ID.
    /// Used to detect downgrades (receiving an older version than expected).
    last_applied_versions: HashMap<String, u32>,
}

impl<'a> SyncManager<'a> {
    /// Creates a new SyncManager with the given storage backend.
    pub fn new(storage: &'a Storage) -> Self {
        SyncManager {
            storage,
            last_applied_versions: HashMap::new(),
        }
    }

    /// Checks if a delta version represents a downgrade for a contact.
    ///
    /// Returns `true` if the `delta_version` is less than the last applied
    /// version for the given contact, indicating a potential downgrade attack
    /// or stale update. Returns `false` if the version is acceptable (equal
    /// or newer) or if no version has been recorded for the contact.
    pub fn check_downgrade(&self, contact_id: &str, delta_version: u32) -> bool {
        if let Some(&last_version) = self.last_applied_versions.get(contact_id) {
            delta_version < last_version
        } else {
            false
        }
    }

    /// Records that a delta version was applied for a contact.
    ///
    /// Updates the last applied version tracker. Only updates if the new
    /// version is greater than or equal to the currently tracked version.
    pub fn record_applied_version(&mut self, contact_id: &str, version: u32) {
        let entry = self
            .last_applied_versions
            .entry(contact_id.to_string())
            .or_insert(0);
        if version >= *entry {
            *entry = version;
        }
    }

    /// Returns the last applied version for a contact, if any.
    pub fn last_applied_version(&self, contact_id: &str) -> Option<u32> {
        self.last_applied_versions.get(contact_id).copied()
    }

    /// Queues a card update for a specific contact.
    ///
    /// Computes the delta between the old and new card states and queues
    /// it for delivery. Multiple updates to the same contact may be
    /// coalesced into a single update.
    pub fn queue_card_update(
        &mut self,
        contact_id: &str,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> Result<String, SyncError> {
        let mut delta = CardDelta::compute(old_card, new_card, self.storage.clock().unix_seconds());

        if delta.changes.is_empty() {
            return Err(SyncError::NoChanges);
        }

        // Auto-increment version per contact (#42)
        let next_version = self
            .last_applied_versions
            .get(contact_id)
            .copied()
            .unwrap_or(0)
            + 1;
        delta.set_version(next_version);
        self.record_applied_version(contact_id, next_version);

        let payload =
            serde_json::to_vec(&delta).map_err(|e| SyncError::Serialization(e.to_string()))?;

        let now = self.storage.clock().unix_seconds();

        let update_id = Uuid::new_v4().to_string(); // TODO(PFC): ambient UUID despite self.rng — see 2026-07-06-core-pfc-violations C5

        let update = PendingUpdate {
            id: update_id.clone(),
            contact_id: contact_id.to_string(),
            update_type: "card_update".to_string(),
            payload,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        };

        self.storage.pending().queue_update(&update)?;

        Ok(update_id)
    }

    /// Queues a visibility change update for a contact.
    pub fn queue_visibility_change(
        &self,
        contact_id: &str,
        visible_fields: Vec<String>,
    ) -> Result<String, SyncError> {
        let payload = serde_json::to_vec(&visible_fields)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;

        let now = self.storage.clock().unix_seconds();

        let update_id = Uuid::new_v4().to_string(); // TODO(PFC): ambient UUID despite self.rng — see 2026-07-06-core-pfc-violations C5

        let update = PendingUpdate {
            id: update_id.clone(),
            contact_id: contact_id.to_string(),
            update_type: "visibility_change".to_string(),
            payload,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        };

        self.storage.pending().queue_update(&update)?;

        Ok(update_id)
    }

    /// Gets pending updates for a specific contact.
    pub fn get_pending(&self, contact_id: &str) -> Result<Vec<PendingUpdate>, SyncError> {
        Ok(self.storage.pending().get_pending_updates(contact_id)?)
    }

    /// Gets all pending updates across all contacts.
    pub fn get_all_pending(&self) -> Result<Vec<PendingUpdate>, SyncError> {
        Ok(self.storage.pending().get_all_pending_updates()?)
    }

    /// Marks an update as successfully delivered.
    ///
    /// Also updates the contact's last sync timestamp.
    pub fn mark_delivered(&self, update_id: &str) -> Result<bool, SyncError> {
        if let Some(update) = self.storage.pending().get_pending_update(update_id)? {
            let contact_id = update.contact_id;

            let deleted = self.storage.pending().mark_update_sent(update_id)?;

            if deleted {
                let now = self.storage.clock().unix_seconds();
                self.storage
                    .sync()
                    .set_contact_last_sync(&contact_id, now)?;
            }

            Ok(deleted)
        } else {
            Ok(false)
        }
    }

    /// Marks an update as failed with retry scheduling.
    pub fn mark_failed(
        &self,
        update_id: &str,
        error: &str,
        retry_count: u32,
    ) -> Result<bool, SyncError> {
        // Exponential backoff: 2s, 4s, 8s, 16s, 32s, ..., max 300s
        let base_delay_secs = 2u64;
        let delay = (base_delay_secs * (1 << retry_count.min(10))).min(300);

        let now = self.storage.clock().unix_seconds();

        let retry_at = now + delay;

        Ok(self.storage.pending().update_pending_status(
            update_id,
            UpdateStatus::Failed {
                error: error.to_string(),
                retry_at,
            },
            retry_count,
        )?)
    }

    /// Gets the sync state for a specific contact.
    pub fn get_sync_state(&self, contact_id: &str) -> Result<SyncState, SyncError> {
        let pending = self.storage.pending().get_pending_updates(contact_id)?;

        if pending.is_empty() {
            let last_sync = self
                .storage
                .sync()
                .get_contact_last_sync(contact_id)?
                .unwrap_or(0);
            return Ok(SyncState::Synced { last_sync });
        }

        let is_syncing = pending
            .iter()
            .any(|u| matches!(u.status, UpdateStatus::Sending));
        if is_syncing {
            return Ok(SyncState::Syncing);
        }

        let failed = pending
            .iter()
            .find(|u| matches!(u.status, UpdateStatus::Failed { .. }));
        if let Some(f) = failed
            && let UpdateStatus::Failed { error, retry_at } = &f.status
        {
            return Ok(SyncState::Failed {
                error: error.clone(),
                retry_at: *retry_at,
            });
        }

        let last_attempt = pending
            .iter()
            .filter_map(|u| {
                if u.retry_count > 0 {
                    Some(u.created_at) // Approximate last attempt
                } else {
                    None
                }
            })
            .max();

        Ok(SyncState::Pending {
            queued_count: pending.len(),
            last_attempt,
        })
    }

    /// Gets the sync status for all contacts with pending updates.
    pub fn sync_status(&self) -> Result<HashMap<String, SyncState>, SyncError> {
        let all_pending = self.storage.pending().get_all_pending_updates()?;

        let mut by_contact: HashMap<String, Vec<&PendingUpdate>> = HashMap::new();
        for update in &all_pending {
            by_contact
                .entry(update.contact_id.clone())
                .or_default()
                .push(update);
        }

        let mut status_map = HashMap::new();
        for (contact_id, updates) in by_contact {
            let state = self.compute_state_from_updates(&updates);
            status_map.insert(contact_id, state);
        }

        Ok(status_map)
    }

    /// Coalesces multiple pending updates for a contact into a single update.
    ///
    /// This reduces network traffic by combining multiple small updates
    /// into one larger update before transmission.
    pub fn coalesce_updates(&self, contact_id: &str) -> Result<Option<String>, SyncError> {
        let pending = self.storage.pending().get_pending_updates(contact_id)?;

        let card_updates: Vec<_> = pending
            .iter()
            .filter(|u| u.update_type == "card_update")
            .collect();

        if card_updates.len() < 2 {
            return Ok(None);
        }

        let mut merged_changes = Vec::new();
        let mut highest_version = 0u32;

        for update in &card_updates {
            if let Ok(delta) = serde_json::from_slice::<CardDelta>(&update.payload) {
                highest_version = highest_version.max(delta.version);
                merged_changes.extend(delta.changes);
            }
        }

        if merged_changes.is_empty() {
            return Ok(None);
        }

        let now = self.storage.clock().unix_seconds();

        // Generate random nonce for replay detection
        let nonce: [u8; 32] = crate::crypto::random_bytes();

        let merged_delta = CardDelta {
            version: highest_version,
            timestamp: now,
            changes: merged_changes,
            nonce,
            signature: [0u8; 64],     // Will be signed before transmission
            validation_summary: None, // Coalesced deltas re-compute summaries before send
        };

        let payload = serde_json::to_vec(&merged_delta)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;

        let merged_id = Uuid::new_v4().to_string();
        let merged_update = PendingUpdate {
            id: merged_id.clone(),
            contact_id: contact_id.to_string(),
            update_type: "card_update".to_string(),
            payload,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        };

        for update in card_updates {
            self.storage.pending().mark_update_sent(&update.id)?;
        }
        self.storage.pending().queue_update(&merged_update)?;

        Ok(Some(merged_id))
    }

    /// Gets ready-to-send updates grouped by target relay URL.
    ///
    /// Filters to only pending or retry-ready updates, then groups by
    /// `target_relay_url`. This enables the sync controller to dispatch
    /// each group to the correct relay connection.
    pub fn get_ready_grouped_by_relay(
        &self,
    ) -> Result<std::collections::BTreeMap<Option<String>, Vec<PendingUpdate>>, SyncError> {
        let ready = self.get_ready_for_retry()?;
        let mut grouped: std::collections::BTreeMap<Option<String>, Vec<PendingUpdate>> =
            std::collections::BTreeMap::new();
        for update in ready {
            grouped
                .entry(update.target_relay_url.clone())
                .or_default()
                .push(update);
        }
        Ok(grouped)
    }

    /// Gets updates that are ready for retry (past their retry_at time).
    pub fn get_ready_for_retry(&self) -> Result<Vec<PendingUpdate>, SyncError> {
        let now = self.storage.clock().unix_seconds();

        let all_pending = self.storage.pending().get_all_pending_updates()?;

        Ok(all_pending
            .into_iter()
            .filter(|u| match &u.status {
                UpdateStatus::Pending => true,
                UpdateStatus::Failed { retry_at, .. } => *retry_at <= now,
                UpdateStatus::Sending => false,
            })
            .collect())
    }

    fn compute_state_from_updates(&self, updates: &[&PendingUpdate]) -> SyncState {
        if updates.is_empty() {
            return SyncState::Synced { last_sync: 0 };
        }

        let is_syncing = updates
            .iter()
            .any(|u| matches!(u.status, UpdateStatus::Sending));
        if is_syncing {
            return SyncState::Syncing;
        }

        let failed = updates
            .iter()
            .find(|u| matches!(u.status, UpdateStatus::Failed { .. }));
        if let Some(f) = failed
            && let UpdateStatus::Failed { error, retry_at } = &f.status
        {
            return SyncState::Failed {
                error: error.clone(),
                retry_at: *retry_at,
            };
        }

        SyncState::Pending {
            queued_count: updates.len(),
            last_attempt: None,
        }
    }
}
