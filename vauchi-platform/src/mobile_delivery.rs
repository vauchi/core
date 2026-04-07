// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync, delivery status, retry queue, offline queue, multi-device delivery, and backup operations.

use std::sync::Arc;

use vauchi_core::{ContactCard, Identity, IdentityBackup, Vauchi, VauchiConfig};

use super::error::{MobileError, lock_or};
use super::types::{
    MobileDeliveryRecord, MobileDeliveryStatus, MobileDeliverySummary, MobileDeviceDeliveryRecord,
    MobileRetryEntry, MobileSyncResult, MobileSyncStatus,
};
use super::{IdentityData, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    // === Sync Operations ===

    /// Sync with relay server via OHTTP-encrypted HTTP.
    ///
    /// Creates a temporary `Vauchi` instance, connects, syncs, and maps the
    /// outcome to `MobileSyncResult`. All synchronous — no tokio runtime needed.
    pub fn sync(&self) -> Result<MobileSyncResult, MobileError> {
        use vauchi_core::api::VauchiSyncOutcome;

        *lock_or(&self.sync_status)? = MobileSyncStatus::Syncing;

        let result = (|| -> Result<MobileSyncResult, MobileError> {
            let config = VauchiConfig::with_storage_path(&self.storage_path)
                .with_relay_url(&self.relay_url)
                .with_storage_key(self.storage_key.clone());
            let mut vauchi = Vauchi::new(config)?;

            vauchi
                .connect()
                .map_err(|e| MobileError::NetworkError(format!("Connect: {e}")))?;

            let outcome = vauchi
                .sync()
                .map_err(|e| MobileError::SyncFailed(format!("{e}")))?;

            vauchi.disconnect();

            match outcome {
                VauchiSyncOutcome::Ok {
                    received,
                    sent,
                    acknowledged: _,
                    errors: _,
                    version_policy: _,
                } => Ok(MobileSyncResult {
                    contacts_added: 0,
                    cards_updated: received as u32,
                    updates_sent: sent as u32,
                    total: (received + sent) as u32,
                    has_changes: received > 0 || sent > 0,
                    updated_contact_names: vec![],
                }),
                VauchiSyncOutcome::TooSoon => Ok(MobileSyncResult {
                    contacts_added: 0,
                    cards_updated: 0,
                    updates_sent: 0,
                    total: 0,
                    has_changes: false,
                    updated_contact_names: vec![],
                }),
                VauchiSyncOutcome::NotConnected => {
                    Err(MobileError::NetworkError("Not connected".into()))
                }
                VauchiSyncOutcome::NoIdentity => Err(MobileError::Internal("No identity".into())),
            }
        })();

        match &result {
            Ok(_) => {
                let _ = lock_or(&self.sync_status).map(|mut g| *g = MobileSyncStatus::Idle);
            }
            Err(_) => {
                let _ = lock_or(&self.sync_status).map(|mut g| *g = MobileSyncStatus::Error);
            }
        }

        result
    }

    /// Get sync status.
    pub fn get_sync_status(&self) -> MobileSyncStatus {
        let Ok(guard) = self.sync_status.lock() else {
            return MobileSyncStatus::Error;
        };
        *guard
    }

    /// Get pending update count.
    pub fn pending_update_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let mut total = 0u32;
        for contact in contacts {
            let pending = storage.get_pending_updates(contact.id())?;
            total += pending.len() as u32;
        }
        Ok(total)
    }

    // === Delivery Privacy Settings ===

    /// Returns whether delivery receipts (ReceivedByRecipient ACKs) are enabled.
    pub fn is_delivery_receipts_enabled(&self) -> bool {
        let Ok(guard) = self.delivery_receipts_enabled.lock() else {
            return false;
        };
        *guard
    }

    /// Sets whether delivery receipts are enabled.
    pub fn set_delivery_receipts_enabled(&self, enabled: bool) {
        let Ok(mut guard) = self.delivery_receipts_enabled.lock() else {
            return;
        };
        *guard = enabled;
    }

    /// Returns whether presence suppression is enabled.
    pub fn is_suppress_presence_enabled(&self) -> bool {
        let Ok(guard) = self.suppress_presence.lock() else {
            return false;
        };
        *guard
    }

    /// Sets whether presence suppression is enabled.
    pub fn set_suppress_presence_enabled(&self, enabled: bool) {
        let Ok(mut guard) = self.suppress_presence.lock() else {
            return;
        };
        *guard = enabled;
    }

    // === Delivery Status Operations ===

    /// Get delivery record for a message.
    pub fn get_delivery_record(
        &self,
        message_id: String,
    ) -> Result<Option<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let record = storage.get_delivery_record(&message_id)?;
        Ok(record.as_ref().map(MobileDeliveryRecord::from))
    }

    /// Get all delivery records.
    pub fn get_all_delivery_records(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_all_delivery_records()?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Get all delivery records for a recipient.
    pub fn get_delivery_records_for_contact(
        &self,
        recipient_id: String,
    ) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_delivery_records_for_recipient(&recipient_id)?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Count failed deliveries.
    pub fn count_failed_deliveries(&self) -> Result<u32, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let storage = self.open_storage()?;
        let count = storage.count_deliveries_by_status(&DeliveryStatus::Failed {
            reason: String::new(),
        })?;
        Ok(count as u32)
    }

    /// Manually retry a failed delivery.
    ///
    /// Returns true if the retry entry was found and rescheduled.
    pub fn manual_retry(&self, message_id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        // Check if there's a retry entry for this message
        let entry = storage.get_retry_entry(&message_id)?;
        if entry.is_none() {
            return Ok(false);
        }

        // Reschedule for immediate retry
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        storage.update_retry_next_time(&message_id, now)?;
        Ok(true)
    }

    /// Get all pending (non-terminal) deliveries.
    pub fn get_pending_deliveries(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_pending_deliveries()?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Get delivery count by status.
    pub fn get_delivery_count_by_status(
        &self,
        status: MobileDeliveryStatus,
    ) -> Result<u32, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let core_status = match status {
            MobileDeliveryStatus::Queued => DeliveryStatus::Queued,
            MobileDeliveryStatus::Sent => DeliveryStatus::Sent,
            MobileDeliveryStatus::Stored => DeliveryStatus::Stored,
            MobileDeliveryStatus::Delivered => DeliveryStatus::Delivered,
            MobileDeliveryStatus::Expired => DeliveryStatus::Expired,
            MobileDeliveryStatus::Failed => DeliveryStatus::Failed {
                reason: String::new(),
            },
        };
        let storage = self.open_storage()?;
        let count = storage.count_deliveries_by_status(&core_status)?;
        Ok(count as u32)
    }

    // === Retry Queue Operations ===

    /// Get all retry entries that are due for retry.
    pub fn get_due_retries(&self) -> Result<Vec<MobileRetryEntry>, MobileError> {
        let storage = self.open_storage()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entries = storage.get_due_retries(now)?;
        Ok(entries.iter().map(MobileRetryEntry::from).collect())
    }

    /// Get all retry entries for a contact.
    pub fn get_retries_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileRetryEntry>, MobileError> {
        let storage = self.open_storage()?;
        let entries = storage.get_retry_entries_for_recipient(&contact_id)?;
        Ok(entries.iter().map(MobileRetryEntry::from).collect())
    }

    /// Get the total count of retry entries.
    pub fn get_retry_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_retry_entries()?;
        Ok(count as u32)
    }

    /// Delete a retry entry (after successful delivery or max attempts).
    pub fn delete_retry(&self, message_id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let deleted = storage.delete_retry_entry(&message_id)?;
        Ok(deleted)
    }

    /// Calculate the backoff time for a given retry attempt.
    ///
    /// Returns seconds until next retry: 2^attempt, max 3600 (1 hour).
    pub fn calculate_retry_backoff(&self, attempt: u32) -> u64 {
        use vauchi_core::storage::RetryQueue;
        let queue = RetryQueue::new();
        queue.backoff_seconds(attempt)
    }

    // === Offline Queue Operations ===

    /// Get total count of all pending updates across all contacts.
    pub fn get_total_pending_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_all_pending_updates()?;
        Ok(count as u32)
    }

    /// Check if the offline queue is full.
    ///
    /// Default max size is 1000 updates.
    pub fn is_offline_queue_full(&self) -> Result<bool, MobileError> {
        use vauchi_core::storage::OfflineQueue;
        let storage = self.open_storage()?;
        let queue = OfflineQueue::new();
        queue
            .is_full(&storage)
            .map_err(|e| MobileError::StorageError(e.to_string()))
    }

    /// Get remaining capacity in the offline queue.
    pub fn get_offline_queue_capacity(&self) -> Result<u32, MobileError> {
        use vauchi_core::storage::OfflineQueue;
        let storage = self.open_storage()?;
        let queue = OfflineQueue::new();
        let remaining = queue
            .remaining_capacity(&storage)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(remaining as u32)
    }

    /// Clear all pending updates for a contact.
    ///
    /// Returns the number of cleared updates.
    pub fn clear_pending_updates_for_contact(
        &self,
        contact_id: String,
    ) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.delete_pending_updates_for_contact(&contact_id)?;
        Ok(count as u32)
    }

    // === Multi-Device Delivery Operations ===

    /// Get delivery summary for a message (X of Y devices delivered).
    pub fn get_delivery_summary(
        &self,
        message_id: String,
    ) -> Result<MobileDeliverySummary, MobileError> {
        let storage = self.open_storage()?;
        let summary = storage.get_delivery_summary(&message_id)?;
        Ok(MobileDeliverySummary::from(&summary))
    }

    /// Get all device delivery records for a message.
    pub fn get_device_deliveries(
        &self,
        message_id: String,
    ) -> Result<Vec<MobileDeviceDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_device_deliveries_for_message(&message_id)?;
        Ok(records
            .iter()
            .map(MobileDeviceDeliveryRecord::from)
            .collect())
    }

    /// Get all pending device deliveries.
    pub fn get_pending_device_deliveries(
        &self,
    ) -> Result<Vec<MobileDeviceDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_pending_device_deliveries()?;
        Ok(records
            .iter()
            .map(MobileDeviceDeliveryRecord::from)
            .collect())
    }

    // === Backup Operations ===

    /// Export encrypted backup.
    pub fn export_backup(&self, password: String) -> Result<String, MobileError> {
        let identity = self.get_identity()?;

        let backup = identity
            .export_backup(&password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(backup.as_bytes());

        Ok(encoded)
    }

    /// Import backup.
    pub fn import_backup(&self, backup_data: String, password: String) -> Result<(), MobileError> {
        {
            let data = lock_or(&self.identity_data)?;
            if data.is_some() {
                return Err(MobileError::AlreadyInitialized);
            }
        }

        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&backup_data)
            .map_err(|_| MobileError::InvalidInput("Invalid base64".to_string()))?;

        let backup = IdentityBackup::new(bytes);
        let identity = Identity::import_backup(&backup, &password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let internal_backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let internal_backup_data = internal_backup.as_bytes().to_vec();
        let display_name = identity.display_name().to_string();

        let storage = self.open_storage()?;
        storage.save_identity(&internal_backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data: internal_backup_data,
            display_name: display_name.clone(),
        };
        *lock_or(&self.identity_data)? = Some(identity_data);

        if storage.load_own_card()?.is_none() {
            let card = ContactCard::new(&display_name);
            storage.save_own_card(&card)?;
        }

        Ok(())
    }
}

// Async sync method — runs sync on a blocking thread to prevent UI freeze.
// Feature-gated behind `async-sync` (default) which pulls in tokio.
#[cfg(feature = "async-sync")]
#[uniffi::export(async_runtime = "tokio")]
impl VauchiPlatform {
    /// Async version of sync for mobile UI threads.
    ///
    /// Delegates to the synchronous `sync()` via `spawn_blocking` so
    /// the core OHTTP HTTP calls don't block the async runtime.
    pub async fn sync_async(self: Arc<Self>) -> Result<MobileSyncResult, MobileError> {
        let platform = self.clone();
        tokio::task::spawn_blocking(move || platform.sync())
            .await
            .map_err(|e| MobileError::Internal(format!("Sync task panicked: {e}")))?
    }
}
