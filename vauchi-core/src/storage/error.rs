//! Storage error types.

use thiserror::Error;

/// Storage error types.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),
}

/// Pending update status.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// Delivery status for tracking message delivery progression.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// Calculates the next retry timestamp.
    pub fn next_retry_time(&self, current_time: u64, attempt: u32) -> u64 {
        current_time + self.backoff_seconds(attempt)
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
        let count = storage.count_all_pending_updates()?;
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
        let count = storage.count_all_pending_updates()?;
        Ok(self.max_queue_size.saturating_sub(count))
    }
}
