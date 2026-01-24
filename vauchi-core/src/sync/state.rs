//! Sync State Machine
//!
//! Manages the synchronization state for each contact and coordinates
//! update delivery with offline queuing and retry logic.

use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use super::delta::CardDelta;
use crate::contact_card::ContactCard;
use crate::storage::{PendingUpdate, Storage, StorageError, UpdateStatus};

/// Returns the current Unix timestamp in seconds.
/// Falls back to 0 if the system clock is before UNIX_EPOCH (should never happen).
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sync error types.
#[derive(Error, Debug)]
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

/// Synchronization state for a contact.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState {
    /// Fully synchronized with no pending updates.
    Synced {
        /// Timestamp of last successful sync.
        last_sync: u64,
    },

    /// Updates are pending for this contact.
    Pending {
        /// Number of updates in the queue.
        queued_count: usize,
        /// Timestamp of last sync attempt (if any).
        last_attempt: Option<u64>,
    },

    /// Currently syncing updates.
    Syncing,

    /// Sync failed, will retry.
    Failed {
        /// Error description.
        error: String,
        /// Timestamp when retry will be attempted.
        retry_at: u64,
    },
}

/// Manages synchronization operations for all contacts.
///
/// The SyncManager coordinates update delivery, handles offline queuing,
/// and tracks sync state per contact.
pub struct SyncManager<'a> {
    storage: &'a Storage,
}

impl<'a> SyncManager<'a> {
    /// Creates a new SyncManager with the given storage backend.
    pub fn new(storage: &'a Storage) -> Self {
        SyncManager { storage }
    }

    /// Queues a card update for a specific contact.
    ///
    /// Computes the delta between the old and new card states and queues
    /// it for delivery. Multiple updates to the same contact may be
    /// coalesced into a single update.
    pub fn queue_card_update(
        &self,
        contact_id: &str,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> Result<String, SyncError> {
        // Compute delta
        let delta = CardDelta::compute(old_card, new_card);

        if delta.changes.is_empty() {
            return Err(SyncError::NoChanges);
        }

        // Serialize delta
        let payload =
            serde_json::to_vec(&delta).map_err(|e| SyncError::Serialization(e.to_string()))?;

        let now = current_timestamp();

        let update_id = Uuid::new_v4().to_string();

        let update = PendingUpdate {
            id: update_id.clone(),
            contact_id: contact_id.to_string(),
            update_type: "card_update".to_string(),
            payload,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };

        self.storage.queue_update(&update)?;

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

        let now = current_timestamp();

        let update_id = Uuid::new_v4().to_string();

        let update = PendingUpdate {
            id: update_id.clone(),
            contact_id: contact_id.to_string(),
            update_type: "visibility_change".to_string(),
            payload,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };

        self.storage.queue_update(&update)?;

        Ok(update_id)
    }

    /// Gets pending updates for a specific contact.
    pub fn get_pending(&self, contact_id: &str) -> Result<Vec<PendingUpdate>, SyncError> {
        Ok(self.storage.get_pending_updates(contact_id)?)
    }

    /// Gets all pending updates across all contacts.
    pub fn get_all_pending(&self) -> Result<Vec<PendingUpdate>, SyncError> {
        Ok(self.storage.get_all_pending_updates()?)
    }

    /// Marks an update as successfully delivered.
    ///
    /// Also updates the contact's last sync timestamp.
    pub fn mark_delivered(&self, update_id: &str) -> Result<bool, SyncError> {
        // Get the update first to find the contact_id
        if let Some(update) = self.storage.get_pending_update(update_id)? {
            let contact_id = update.contact_id.clone();

            // Delete the update
            let deleted = self.storage.mark_update_sent(update_id)?;

            if deleted {
                // Update the contact's last sync timestamp
                let now = current_timestamp();
                self.storage.set_contact_last_sync(&contact_id, now)?;
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
        // Exponential backoff: 30s, 1m, 2m, 4m, 8m, ...
        let base_delay_secs = 30u64;
        let delay = base_delay_secs * (1 << retry_count.min(6));

        let now = current_timestamp();

        let retry_at = now + delay;

        Ok(self.storage.update_pending_status(
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
        let pending = self.storage.get_pending_updates(contact_id)?;

        if pending.is_empty() {
            // Get actual last sync time from storage
            let last_sync = self.storage.get_contact_last_sync(contact_id)?.unwrap_or(0);
            return Ok(SyncState::Synced { last_sync });
        }

        // Check if any update is currently being sent
        let is_syncing = pending
            .iter()
            .any(|u| matches!(u.status, UpdateStatus::Sending));
        if is_syncing {
            return Ok(SyncState::Syncing);
        }

        // Check for failed updates
        let failed = pending
            .iter()
            .find(|u| matches!(u.status, UpdateStatus::Failed { .. }));
        if let Some(f) = failed {
            if let UpdateStatus::Failed { error, retry_at } = &f.status {
                return Ok(SyncState::Failed {
                    error: error.clone(),
                    retry_at: *retry_at,
                });
            }
        }

        // Has pending updates
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
        let all_pending = self.storage.get_all_pending_updates()?;

        // Group by contact_id
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
        let pending = self.storage.get_pending_updates(contact_id)?;

        // Only coalesce if there are multiple card_update entries
        let card_updates: Vec<_> = pending
            .iter()
            .filter(|u| u.update_type == "card_update")
            .collect();

        if card_updates.len() < 2 {
            return Ok(None);
        }

        // Parse and merge all deltas
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

        // Create merged delta
        let now = current_timestamp();

        let merged_delta = CardDelta {
            version: highest_version,
            timestamp: now,
            changes: merged_changes,
            signature: [0u8; 64], // Will be signed before transmission
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
        };

        // Remove old updates and add merged one
        for update in card_updates {
            self.storage.mark_update_sent(&update.id)?;
        }
        self.storage.queue_update(&merged_update)?;

        Ok(Some(merged_id))
    }

    /// Gets updates that are ready for retry (past their retry_at time).
    pub fn get_ready_for_retry(&self) -> Result<Vec<PendingUpdate>, SyncError> {
        let now = current_timestamp();

        let all_pending = self.storage.get_all_pending_updates()?;

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

        // Check for syncing
        let is_syncing = updates
            .iter()
            .any(|u| matches!(u.status, UpdateStatus::Sending));
        if is_syncing {
            return SyncState::Syncing;
        }

        // Check for failed
        let failed = updates
            .iter()
            .find(|u| matches!(u.status, UpdateStatus::Failed { .. }));
        if let Some(f) = failed {
            if let UpdateStatus::Failed { error, retry_at } = &f.status {
                return SyncState::Failed {
                    error: error.clone(),
                    retry_at: *retry_at,
                };
            }
        }

        // Pending
        SyncState::Pending {
            queued_count: updates.len(),
            last_attempt: None,
        }
    }
}
