// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! DeliveryService: ACK→Storage bridge.
//!
//! Receives acknowledgment events from the transport layer and updates
//! delivery records and retry entries in storage accordingly.

use crate::storage::{
    DeliveryStatus, DeliverySummary, DeviceDeliveryStatus, RetryEntry, RetryQueue, Storage,
    StorageError,
};

/// ACK status received from the relay/transport layer.
///
/// This is a transport-agnostic representation of delivery acknowledgments,
/// decoupled from network feature flags.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeliveryAckStatus {
    /// Relay confirmed message storage.
    Stored,
    /// Recipient confirmed message receipt.
    Delivered,
    /// Delivery failed with a reason.
    Failed { reason: String },
}

#[cfg(feature = "network-rustls")]
impl DeliveryAckStatus {
    /// Converts from network `AckStatus` to delivery-layer `DeliveryAckStatus`.
    pub fn from_network_ack(status: crate::network::AckStatus, error: Option<&str>) -> Self {
        use crate::network::AckStatus;
        match status {
            AckStatus::Stored => DeliveryAckStatus::Stored,
            AckStatus::Delivered | AckStatus::ReceivedByRecipient => DeliveryAckStatus::Delivered,
            AckStatus::Failed => DeliveryAckStatus::Failed {
                reason: error.unwrap_or("unknown").to_string(),
            },
        }
    }
}

/// Bridges ACK events from the transport layer to delivery storage.
///
/// Handles the lifecycle of delivery records:
/// - Stored ACK → update delivery status to Stored
/// - Delivered ACK → update status to Delivered + remove retry entry
/// - Failed ACK → update status to Failed + schedule retry
pub struct DeliveryService {
    retry_queue: RetryQueue,
}

impl DeliveryService {
    /// Creates a new DeliveryService with default retry queue settings.
    pub fn new() -> Self {
        Self {
            retry_queue: RetryQueue::new(),
        }
    }

    /// Creates a new DeliveryService with a custom retry queue.
    pub fn with_retry_queue(retry_queue: RetryQueue) -> Self {
        Self { retry_queue }
    }

    /// Handles an ACK for a message, updating delivery storage accordingly.
    ///
    /// - `Stored`: Updates delivery record status to `Stored`
    /// - `Delivered`: Updates status to `Delivered` and removes any retry entry
    /// - `Failed`: Updates status to `Failed` and schedules a retry entry
    ///
    /// Returns an error if the message ID is not found in delivery records.
    pub fn handle_ack(
        &self,
        storage: &Storage,
        message_id: &str,
        status: DeliveryAckStatus,
        rng: &dyn crate::rng::SecureRng,
    ) -> Result<(), StorageError> {
        let record = storage.get_delivery_record(message_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("Delivery record not found: {}", message_id))
        })?;

        let now = storage.clock().unix_seconds();

        match status {
            DeliveryAckStatus::Stored => {
                storage.update_delivery_status(message_id, &DeliveryStatus::Stored, now)?;
            }
            DeliveryAckStatus::Delivered => {
                storage.update_delivery_status(message_id, &DeliveryStatus::Delivered, now)?;
                // Clean up any existing retry entry —
                // propagate so a stuck retry queue surfaces instead of
                // growing forever (was silently dropped before 2026-05-21).
                storage.delete_retry_entry(message_id)?;
            }
            DeliveryAckStatus::Failed { reason } => {
                storage.update_delivery_status(
                    message_id,
                    &DeliveryStatus::Failed { reason },
                    now,
                )?;

                // Schedule a retry entry
                let entry = RetryEntry {
                    message_id: message_id.to_string(),
                    recipient_id: record.recipient_id,
                    payload: vec![],
                    attempt: 0,
                    next_retry: self.retry_queue.next_retry_time_with_jitter(now, 0, rng),
                    created_at: now,
                    max_attempts: 10,
                };
                storage.create_retry_entry(&entry)?;
            }
        }

        Ok(())
    }

    /// Handles a per-device ACK, updating the device delivery record and
    /// recomputing the aggregate message delivery status.
    ///
    /// When all devices confirm delivery, the message-level status is
    /// automatically promoted to `Delivered`.
    ///
    /// Returns the updated `DeliverySummary` so callers can check progress.
    pub fn handle_device_ack(
        &self,
        storage: &Storage,
        message_id: &str,
        device_id: &str,
        status: DeviceDeliveryStatus,
    ) -> Result<DeliverySummary, StorageError> {
        // Verify the message-level record exists
        storage.get_delivery_record(message_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("Delivery record not found: {}", message_id))
        })?;

        let now = storage.clock().unix_seconds();

        // Update the individual device status
        storage.update_device_delivery_status(message_id, device_id, status, now)?;

        // Recompute aggregate
        let summary = storage.get_delivery_summary(message_id)?;

        // If all devices delivered, promote message-level status
        if summary.is_fully_delivered() {
            storage.update_delivery_status(message_id, &DeliveryStatus::Delivered, now)?;
        }

        Ok(summary)
    }

    /// Runs periodic cleanup tasks:
    /// 1. Marks records with `expires_at` in the past as `Expired`
    /// 2. Removes old terminal records (delivered/expired/failed) older than 30 days
    ///
    /// Returns a `CleanupResult` with counts of affected records.
    pub fn run_cleanup(&self, storage: &Storage) -> Result<CleanupResult, StorageError> {
        let now = storage.clock().unix_seconds();

        let expired = storage.expire_old_deliveries(now)?;
        let cleaned_up = storage.run_startup_maintenance()?;

        Ok(CleanupResult {
            expired,
            cleaned_up,
        })
    }
}

/// Result of a periodic cleanup run.
#[derive(Debug, Default)]
pub struct CleanupResult {
    /// Number of records marked as expired (TTL exceeded).
    pub expired: usize,
    /// Number of old terminal records removed.
    pub cleaned_up: usize,
}

impl Default for DeliveryService {
    fn default() -> Self {
        Self::new()
    }
}
