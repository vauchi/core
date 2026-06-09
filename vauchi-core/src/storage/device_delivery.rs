// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`DeviceDeliveryStore`](super::DeviceDeliveryStore).

use super::error::{DeliverySummary, DeviceDeliveryRecord, DeviceDeliveryStatus};
use super::{Storage, StorageError};

impl Storage {
    /// Creates a new device delivery record.
    pub fn create_device_delivery(
        &self,
        record: &DeviceDeliveryRecord,
    ) -> Result<(), StorageError> {
        self.device_deliveries().create_device_delivery(record)
    }
    /// Gets a device delivery record.
    pub fn get_device_delivery(
        &self,
        message_id: &str,
        device_id: &str,
    ) -> Result<Option<DeviceDeliveryRecord>, StorageError> {
        self.device_deliveries()
            .get_device_delivery(message_id, device_id)
    }
    /// Gets all device delivery records for a message.
    pub fn get_device_deliveries_for_message(
        &self,
        message_id: &str,
    ) -> Result<Vec<DeviceDeliveryRecord>, StorageError> {
        self.device_deliveries()
            .get_device_deliveries_for_message(message_id)
    }
    /// Updates the status of a device delivery.
    pub fn update_device_delivery_status(
        &self,
        message_id: &str,
        device_id: &str,
        status: DeviceDeliveryStatus,
        updated_at: u64,
    ) -> Result<bool, StorageError> {
        self.device_deliveries()
            .update_device_delivery_status(message_id, device_id, status, updated_at)
    }
    /// Gets delivery summary for a message (X of Y devices delivered).
    pub fn get_delivery_summary(&self, message_id: &str) -> Result<DeliverySummary, StorageError> {
        self.device_deliveries().get_delivery_summary(message_id)
    }
    /// Deletes all device delivery records for a message.
    pub fn delete_device_deliveries_for_message(
        &self,
        message_id: &str,
    ) -> Result<usize, StorageError> {
        self.device_deliveries()
            .delete_device_deliveries_for_message(message_id)
    }
    /// Gets all pending device deliveries (not yet delivered).
    pub fn get_pending_device_deliveries(&self) -> Result<Vec<DeviceDeliveryRecord>, StorageError> {
        self.device_deliveries().get_pending_device_deliveries()
    }
    /// Counts device deliveries by status.
    pub fn count_device_deliveries_by_status(
        &self,
        status: DeviceDeliveryStatus,
    ) -> Result<usize, StorageError> {
        self.device_deliveries()
            .count_device_deliveries_by_status(status)
    }
}
