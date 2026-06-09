// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`DeliveryStore`](super::DeliveryStore).

use super::error::{DeliveryRecord, DeliveryStatus};
use super::{Storage, StorageError};

impl Storage {
    /// Creates a new delivery record.
    pub fn create_delivery_record(&self, record: &DeliveryRecord) -> Result<(), StorageError> {
        self.deliveries().create_delivery_record(record)
    }
    /// Gets a delivery record by message ID.
    pub fn get_delivery_record(
        &self,
        message_id: &str,
    ) -> Result<Option<DeliveryRecord>, StorageError> {
        self.deliveries().get_delivery_record(message_id)
    }
    /// Gets all delivery records for a recipient.
    pub fn get_delivery_records_for_recipient(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<DeliveryRecord>, StorageError> {
        self.deliveries()
            .get_delivery_records_for_recipient(recipient_id)
    }
    /// Gets all delivery records.
    pub fn get_all_delivery_records(&self) -> Result<Vec<DeliveryRecord>, StorageError> {
        self.deliveries().get_all_delivery_records()
    }
    /// Gets all delivery records with a specific status.
    pub fn get_delivery_records_by_status(
        &self,
        status: &DeliveryStatus,
    ) -> Result<Vec<DeliveryRecord>, StorageError> {
        self.deliveries().get_delivery_records_by_status(status)
    }
    /// Updates the status of a delivery record.
    pub fn update_delivery_status(
        &self,
        message_id: &str,
        status: &DeliveryStatus,
        updated_at: u64,
    ) -> Result<bool, StorageError> {
        self.deliveries()
            .update_delivery_status(message_id, status, updated_at)
    }
    /// Deletes a delivery record.
    pub fn delete_delivery_record(&self, message_id: &str) -> Result<bool, StorageError> {
        self.deliveries().delete_delivery_record(message_id)
    }
    /// Gets pending (non-terminal) delivery records that haven't been fully delivered.
    pub fn get_pending_deliveries(&self) -> Result<Vec<DeliveryRecord>, StorageError> {
        self.deliveries().get_pending_deliveries()
    }
    /// Marks expired delivery records as expired.
    pub fn expire_old_deliveries(&self, now: u64) -> Result<usize, StorageError> {
        self.deliveries().expire_old_deliveries(now)
    }
    /// Runs startup maintenance: cleans terminal delivery records older than 30 days (T2-12).
    ///
    /// Called automatically on `Storage::open()`. Deletes records with status
    /// 'delivered', 'expired', or 'failed' whose `updated_at` is more than
    /// 30 days in the past. Returns the number of rows deleted.
    pub fn run_startup_maintenance(&self) -> Result<usize, StorageError> {
        self.deliveries().run_startup_maintenance()
    }
    /// Deletes terminal delivery records older than `cutoff` timestamp (#124/#158).
    ///
    /// Removes records with status 'delivered', 'expired', or 'failed' whose
    /// `updated_at` is before `cutoff`. Returns the number of rows deleted.
    pub fn cleanup_old_deliveries(&self, cutoff: u64) -> Result<usize, StorageError> {
        self.deliveries().cleanup_old_deliveries(cutoff)
    }
    /// Extends the TTL of a delivery record by adding additional seconds to `expires_at`.
    ///
    /// Returns `true` if the record was found and updated, `false` if the
    /// message ID was not found or has no expiry set.
    pub fn extend_delivery_ttl(
        &self,
        message_id: &str,
        additional_secs: u64,
    ) -> Result<bool, StorageError> {
        self.deliveries()
            .extend_delivery_ttl(message_id, additional_secs)
    }
    /// Counts delivery records by status.
    pub fn count_deliveries_by_status(
        &self,
        status: &DeliveryStatus,
    ) -> Result<usize, StorageError> {
        self.deliveries().count_deliveries_by_status(status)
    }
}
