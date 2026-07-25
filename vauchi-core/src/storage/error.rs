// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage error types.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// State of identity deletion (stored in the database).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeletionState {
    /// No deletion scheduled.
    None,
    /// Deletion scheduled with a grace period.
    Scheduled {
        /// When deletion was scheduled (Unix timestamp).
        scheduled_at: u64,
        /// When deletion can be executed (Unix timestamp).
        execute_at: u64,
    },
    /// Deletion has been executed.
    Executed {
        /// When deletion was executed (Unix timestamp).
        executed_at: u64,
    },
}

impl DeletionState {
    /// Returns the time remaining before scheduled deletion executes.
    ///
    /// `now` is the current Unix-epoch seconds — production callers route
    /// it through `Storage::clock().unix_seconds()`.
    ///
    /// Returns `None` for `None` or `Executed` states.
    /// Returns `Duration::ZERO` if the execution time has already passed.
    pub fn time_remaining(&self, now: u64) -> Option<std::time::Duration> {
        match self {
            DeletionState::Scheduled { execute_at, .. } => Some(std::time::Duration::from_secs(
                execute_at.saturating_sub(now),
            )),
            _ => None,
        }
    }
}

/// Storage error types.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(rusqlite::Error),

    #[error("Device storage is full. Free up space and try again.")]
    DiskFull,

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Queue full: {0}")]
    QueueFull(String),
}

impl From<rusqlite::Error> for StorageError {
    fn from(err: rusqlite::Error) -> Self {
        // SQLITE_FULL (code 13): database or disk is full
        if let rusqlite::Error::SqliteFailure(ref ffi_err, _) = err
            && ffi_err.extended_code == 13
        {
            return Self::DiskFull;
        }
        Self::Database(err)
    }
}

impl StorageError {
    /// Returns a user-friendly message suitable for display in the UI.
    pub fn user_message(&self) -> &str {
        match self {
            Self::DiskFull => {
                "Your device storage is full. Free up space in Settings and try again."
            }
            Self::QueueFull(_) => "Too many pending updates. Connect to the internet to sync.",
            _ => "A storage error occurred. Please try again.",
        }
    }
}

/// Pending update status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpdateStatus {
    Pending,
    Sending,
    Failed { error: String, retry_at: u64 },
}

/// A pending sync update.
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub id: String,
    pub contact_id: String,
    pub update_type: String,
    pub payload: Vec<u8>,
    pub created_at: u64,
    pub retry_count: u32,
    pub status: UpdateStatus,
    /// Target relay URL for this update. When set, the update should be
    /// sent to the contact's relay instead of the home relay.
    /// Populated from `Contact::relay_url()` when the update is queued.
    pub target_relay_url: Option<String>,
    /// Recipient device id for a per-device fan-out copy (F4, ADR-064
    /// Amendment 2026-07-25). `Some(device_id)` deposits at that device's
    /// device-scoped contact mailbox; `None` (legacy `[0;32]`, genesis,
    /// alerts, reciprocity) deposits at the identity-scoped mailbox.
    pub target_device_id: Option<[u8; 32]>,
}

/// Delivery status for tracking message delivery progression.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeliveryStatus {
    /// Message queued locally, not yet sent.
    Queued,
    /// Message sent to relay.
    Sent,
    /// Relay confirmed storage.
    Stored,
    /// Recipient confirmed receipt.
    Delivered,
    /// Message expired without delivery.
    Expired,
    /// Delivery failed.
    Failed { reason: String },
}

/// A record tracking delivery status of an outbound message.
#[derive(Debug, Clone)]
pub struct DeliveryRecord {
    /// Unique message ID (UUID).
    pub message_id: String,
    /// Recipient's contact ID.
    pub recipient_id: String,
    /// Current delivery status.
    pub status: DeliveryStatus,
    /// When the message was created.
    pub created_at: u64,
    /// When the status was last updated.
    pub updated_at: u64,
    /// When the message expires (optional).
    pub expires_at: Option<u64>,
}

/// An entry in the retry queue for failed message deliveries.
#[derive(Debug, Clone)]
pub struct RetryEntry {
    /// Unique message ID.
    pub message_id: String,
    /// Recipient's contact ID.
    pub recipient_id: String,
    /// The message payload to retry.
    pub payload: Vec<u8>,
    /// Current retry attempt (0 = first attempt).
    pub attempt: u32,
    /// Unix timestamp for next retry.
    pub next_retry: u64,
    /// When the entry was created.
    pub created_at: u64,
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
}

impl RetryEntry {
    /// Returns true if the maximum retry attempts have been exceeded.
    pub fn is_max_attempts_exceeded(&self) -> bool {
        self.attempt >= self.max_attempts
    }
}

/// Retry queue with exponential backoff calculation.
#[derive(Debug, Clone, Default)]
pub struct RetryQueue {
    /// Maximum backoff in seconds (default: 1 hour).
    max_backoff_secs: u64,
}

impl RetryQueue {
    /// Creates a new retry queue with default settings.
    pub fn new() -> Self {
        RetryQueue {
            max_backoff_secs: 3600, // 1 hour
        }
    }

    /// Creates a new retry queue with custom max backoff.
    pub fn with_max_backoff(max_backoff_secs: u64) -> Self {
        RetryQueue { max_backoff_secs }
    }

    /// Calculates the backoff time in seconds for a given attempt.
    ///
    /// Uses exponential backoff: 2^attempt seconds, capped at max_backoff_secs.
    pub fn backoff_seconds(&self, attempt: u32) -> u64 {
        let backoff = 2u64.saturating_pow(attempt);
        backoff.min(self.max_backoff_secs)
    }

    /// Calculates the backoff time with jitter to prevent thundering herd.
    ///
    /// Returns base backoff + random jitter in range [0, base * 0.25].
    pub fn backoff_seconds_with_jitter(
        &self,
        attempt: u32,
        rng: &dyn crate::rng::SecureRng,
    ) -> u64 {
        let base = self.backoff_seconds(attempt);
        let jitter_range = base / 4; // 25% of base
        if jitter_range == 0 {
            return base;
        }
        // Non-crypto RNG: jitter for backoff timing, not security-sensitive
        let jitter = rng.random_in_range_u64(0, jitter_range);
        base + jitter
    }

    /// Calculates the next retry timestamp.
    pub fn next_retry_time(&self, current_time: u64, attempt: u32) -> u64 {
        current_time + self.backoff_seconds(attempt)
    }

    /// Calculates the next retry timestamp with jitter.
    pub fn next_retry_time_with_jitter(
        &self,
        current_time: u64,
        attempt: u32,
        rng: &dyn crate::rng::SecureRng,
    ) -> u64 {
        current_time + self.backoff_seconds_with_jitter(attempt, rng)
    }
}

/// Offline queue configuration and helpers.
#[derive(Debug, Clone)]
pub struct OfflineQueue {
    /// Maximum number of pending updates to queue.
    max_queue_size: usize,
}

impl Default for OfflineQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Delivery status for a specific device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceDeliveryStatus {
    /// Message pending delivery to this device.
    Pending,
    /// Message stored at relay for this device.
    Stored,
    /// Message delivered to this device.
    Delivered,
    /// Delivery to this device failed.
    Failed,
}

/// Per-device delivery tracking record.
#[derive(Debug, Clone)]
pub struct DeviceDeliveryRecord {
    /// Message ID being tracked.
    pub message_id: String,
    /// Recipient's contact ID.
    pub recipient_id: String,
    /// Target device ID.
    pub device_id: String,
    /// Delivery status for this device.
    pub status: DeviceDeliveryStatus,
    /// When the status was last updated.
    pub updated_at: u64,
}

/// Summary of delivery status across all devices.
#[derive(Debug, Clone)]
pub struct DeliverySummary {
    /// Message ID.
    pub message_id: String,
    /// Total number of target devices.
    pub total_devices: usize,
    /// Number of devices that received the message.
    pub delivered_devices: usize,
    /// Number of devices still pending.
    pub pending_devices: usize,
    /// Number of devices where delivery failed.
    pub failed_devices: usize,
}

impl DeliverySummary {
    /// Returns true if all devices have received the message.
    pub fn is_fully_delivered(&self) -> bool {
        self.delivered_devices == self.total_devices && self.total_devices > 0
    }

    /// Returns the delivery progress as a fraction (0.0 to 1.0).
    pub fn progress(&self) -> f32 {
        if self.total_devices == 0 {
            return 0.0;
        }
        self.delivered_devices as f32 / self.total_devices as f32
    }
}

impl OfflineQueue {
    /// Default maximum queue size.
    pub const DEFAULT_MAX_SIZE: usize = 1000;

    /// Creates a new offline queue with default settings.
    pub fn new() -> Self {
        OfflineQueue {
            max_queue_size: Self::DEFAULT_MAX_SIZE,
        }
    }

    /// Creates a new offline queue with custom max size.
    pub fn with_max_size(max_size: usize) -> Self {
        OfflineQueue {
            max_queue_size: max_size,
        }
    }

    /// Returns the maximum queue size.
    pub fn max_queue_size(&self) -> usize {
        self.max_queue_size
    }

    /// Checks if the queue is full.
    pub fn is_full(&self, storage: &super::Storage) -> Result<bool, super::StorageError> {
        let count = storage.pending().count_all_pending_updates()?;
        Ok(count >= self.max_queue_size)
    }

    /// Checks if there's room to queue more updates.
    pub fn can_queue(&self, storage: &super::Storage) -> Result<bool, super::StorageError> {
        Ok(!self.is_full(storage)?)
    }

    /// Returns the remaining capacity in the queue.
    pub fn remaining_capacity(
        &self,
        storage: &super::Storage,
    ) -> Result<usize, super::StorageError> {
        let count = storage.pending().count_all_pending_updates()?;
        Ok(self.max_queue_size.saturating_sub(count))
    }
}
