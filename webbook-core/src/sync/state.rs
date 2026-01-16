//! Sync State Machine
//!
//! Manages the synchronization state for each contact and coordinates
//! update delivery with offline queuing and retry logic.

use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::contact_card::ContactCard;
use crate::storage::{Storage, StorageError, PendingUpdate, UpdateStatus};
use super::delta::CardDelta;

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
        let payload = serde_json::to_vec(&delta)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

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

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

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
    pub fn mark_delivered(&self, update_id: &str) -> Result<bool, SyncError> {
        Ok(self.storage.mark_update_sent(update_id)?)
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

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

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
            // Check last sync time from contact
            // For now, return a default synced state
            return Ok(SyncState::Synced { last_sync: 0 });
        }

        // Check if any update is currently being sent
        let is_syncing = pending.iter().any(|u| matches!(u.status, UpdateStatus::Sending));
        if is_syncing {
            return Ok(SyncState::Syncing);
        }

        // Check for failed updates
        let failed = pending.iter().find(|u| matches!(u.status, UpdateStatus::Failed { .. }));
        if let Some(f) = failed {
            if let UpdateStatus::Failed { error, retry_at } = &f.status {
                return Ok(SyncState::Failed {
                    error: error.clone(),
                    retry_at: *retry_at,
                });
            }
        }

        // Has pending updates
        let last_attempt = pending.iter()
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
            by_contact.entry(update.contact_id.clone())
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
        let card_updates: Vec<_> = pending.iter()
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let all_pending = self.storage.get_all_pending_updates()?;

        Ok(all_pending.into_iter()
            .filter(|u| {
                match &u.status {
                    UpdateStatus::Pending => true,
                    UpdateStatus::Failed { retry_at, .. } => *retry_at <= now,
                    UpdateStatus::Sending => false,
                }
            })
            .collect())
    }

    fn compute_state_from_updates(&self, updates: &[&PendingUpdate]) -> SyncState {
        if updates.is_empty() {
            return SyncState::Synced { last_sync: 0 };
        }

        // Check for syncing
        let is_syncing = updates.iter().any(|u| matches!(u.status, UpdateStatus::Sending));
        if is_syncing {
            return SyncState::Syncing;
        }

        // Check for failed
        let failed = updates.iter().find(|u| matches!(u.status, UpdateStatus::Failed { .. }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::{ContactCard, ContactField, FieldType};
    use crate::crypto::SymmetricKey;

    fn create_test_storage() -> Storage {
        let key = SymmetricKey::generate();
        Storage::in_memory(key).unwrap()
    }

    #[test]
    fn test_sync_queue_card_update() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let mut old_card = ContactCard::new("Alice");
        let _ = old_card.add_field(ContactField::new(FieldType::Email, "email", "old@example.com"));

        let mut new_card = ContactCard::new("Alice");
        let _ = new_card.add_field(ContactField::new(FieldType::Email, "email", "new@example.com"));

        let update_id = manager.queue_card_update("contact-1", &old_card, &new_card).unwrap();
        assert!(!update_id.is_empty());

        let pending = manager.get_pending("contact-1").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].update_type, "card_update");
    }

    #[test]
    fn test_sync_no_changes() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let card = ContactCard::new("Alice");

        let result = manager.queue_card_update("contact-1", &card, &card);
        assert!(matches!(result, Err(SyncError::NoChanges)));
    }

    #[test]
    fn test_sync_queue_visibility_change() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let update_id = manager.queue_visibility_change(
            "contact-1",
            vec!["email".to_string(), "phone".to_string()],
        ).unwrap();

        assert!(!update_id.is_empty());

        let pending = manager.get_pending("contact-1").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].update_type, "visibility_change");
    }

    #[test]
    fn test_sync_mark_delivered() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let mut old_card = ContactCard::new("Alice");
        let _ = old_card.add_field(ContactField::new(FieldType::Email, "email", "old@example.com"));

        let mut new_card = ContactCard::new("Alice");
        let _ = new_card.add_field(ContactField::new(FieldType::Email, "email", "new@example.com"));

        let update_id = manager.queue_card_update("contact-1", &old_card, &new_card).unwrap();

        assert_eq!(manager.get_pending("contact-1").unwrap().len(), 1);

        manager.mark_delivered(&update_id).unwrap();

        assert_eq!(manager.get_pending("contact-1").unwrap().len(), 0);
    }

    #[test]
    fn test_sync_mark_failed_with_backoff() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let mut old_card = ContactCard::new("Alice");
        let _ = old_card.add_field(ContactField::new(FieldType::Email, "email", "old@example.com"));

        let mut new_card = ContactCard::new("Alice");
        let _ = new_card.add_field(ContactField::new(FieldType::Email, "email", "new@example.com"));

        let update_id = manager.queue_card_update("contact-1", &old_card, &new_card).unwrap();

        manager.mark_failed(&update_id, "Connection refused", 0).unwrap();

        let pending = manager.get_pending("contact-1").unwrap();
        assert!(matches!(pending[0].status, UpdateStatus::Failed { .. }));
    }

    #[test]
    fn test_sync_state_pending() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let mut old_card = ContactCard::new("Alice");
        let _ = old_card.add_field(ContactField::new(FieldType::Email, "email", "old@example.com"));

        let mut new_card = ContactCard::new("Alice");
        let _ = new_card.add_field(ContactField::new(FieldType::Email, "email", "new@example.com"));

        manager.queue_card_update("contact-1", &old_card, &new_card).unwrap();

        let state = manager.get_sync_state("contact-1").unwrap();
        assert!(matches!(state, SyncState::Pending { queued_count: 1, .. }));
    }

    #[test]
    fn test_sync_state_synced() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let state = manager.get_sync_state("contact-1").unwrap();
        assert!(matches!(state, SyncState::Synced { .. }));
    }

    #[test]
    fn test_sync_state_failed() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let mut old_card = ContactCard::new("Alice");
        let _ = old_card.add_field(ContactField::new(FieldType::Email, "email", "old@example.com"));

        let mut new_card = ContactCard::new("Alice");
        let _ = new_card.add_field(ContactField::new(FieldType::Email, "email", "new@example.com"));

        let update_id = manager.queue_card_update("contact-1", &old_card, &new_card).unwrap();
        manager.mark_failed(&update_id, "Network error", 0).unwrap();

        let state = manager.get_sync_state("contact-1").unwrap();
        assert!(matches!(state, SyncState::Failed { .. }));
    }

    #[test]
    fn test_sync_coalesce_updates() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        // Queue multiple updates
        let card1 = ContactCard::new("Alice");
        let mut card2 = ContactCard::new("Alice");
        let _ = card2.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));
        let mut card3 = ContactCard::new("Alice");
        let _ = card3.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));
        let _ = card3.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

        manager.queue_card_update("contact-1", &card1, &card2).unwrap();
        manager.queue_card_update("contact-1", &card2, &card3).unwrap();

        assert_eq!(manager.get_pending("contact-1").unwrap().len(), 2);

        // Coalesce
        let merged_id = manager.coalesce_updates("contact-1").unwrap();
        assert!(merged_id.is_some());

        // Should now have only one update
        assert_eq!(manager.get_pending("contact-1").unwrap().len(), 1);
    }

    #[test]
    fn test_sync_status_multiple_contacts() {
        let storage = create_test_storage();
        let manager = SyncManager::new(&storage);

        let card1 = ContactCard::new("Alice");
        let mut card2 = ContactCard::new("Alice");
        let _ = card2.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));

        manager.queue_card_update("contact-1", &card1, &card2).unwrap();
        manager.queue_card_update("contact-2", &card1, &card2).unwrap();

        let status = manager.sync_status().unwrap();

        assert_eq!(status.len(), 2);
        assert!(status.contains_key("contact-1"));
        assert!(status.contains_key("contact-2"));
    }
}
