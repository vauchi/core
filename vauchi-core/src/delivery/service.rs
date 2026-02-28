// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! DeliveryService: ACK→Storage bridge.
//!
//! Receives acknowledgment events from the transport layer and updates
//! delivery records and retry entries in storage accordingly.

use crate::storage::{DeliveryStatus, RetryEntry, RetryQueue, Storage, StorageError};

/// ACK status received from the relay/transport layer.
///
/// This is a transport-agnostic representation of delivery acknowledgments,
/// decoupled from network feature flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryAckStatus {
    /// Relay confirmed message storage.
    Stored,
    /// Recipient confirmed message receipt.
    Delivered,
    /// Delivery failed with a reason.
    Failed { reason: String },
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
    ) -> Result<(), StorageError> {
        let record = storage.get_delivery_record(message_id)?.ok_or_else(|| {
            StorageError::NotFound(format!("Delivery record not found: {}", message_id))
        })?;

        let now = current_timestamp();

        match status {
            DeliveryAckStatus::Stored => {
                storage.update_delivery_status(message_id, &DeliveryStatus::Stored, now)?;
            }
            DeliveryAckStatus::Delivered => {
                storage.update_delivery_status(message_id, &DeliveryStatus::Delivered, now)?;
                // Clean up any existing retry entry
                let _ = storage.delete_retry_entry(message_id);
            }
            DeliveryAckStatus::Failed { reason } => {
                storage.update_delivery_status(
                    message_id,
                    &DeliveryStatus::Failed {
                        reason: reason.clone(),
                    },
                    now,
                )?;

                // Schedule a retry entry
                let entry = RetryEntry {
                    message_id: message_id.to_string(),
                    recipient_id: record.recipient_id,
                    payload: vec![],
                    attempt: 0,
                    next_retry: self.retry_queue.next_retry_time_with_jitter(now, 0),
                    created_at: now,
                    max_attempts: 10,
                };
                storage.create_retry_entry(&entry)?;
            }
        }

        Ok(())
    }
}

impl Default for DeliveryService {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
