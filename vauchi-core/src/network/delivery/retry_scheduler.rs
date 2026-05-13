// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! RetryScheduler: periodic retry processing.
//!
//! Processes due retry entries, increments attempts with exponential backoff,
//! and removes entries that exceed their maximum attempt count.

use crate::storage::{DeliveryStatus, RetryQueue, Storage, StorageError};

/// Result of a retry scheduler tick.
#[derive(Debug, Default)]
pub struct RetryTickResult {
    /// Number of due retry entries found.
    pub due: usize,
    /// Number of entries rescheduled with incremented attempt.
    pub rescheduled: usize,
    /// Number of entries removed due to exceeding max attempts.
    pub expired: usize,
    /// Message IDs that were rescheduled and are ready for resend.
    pub ready_ids: Vec<String>,
}

/// Processes due retry entries on a periodic tick.
pub struct RetryScheduler {
    retry_queue: RetryQueue,
}

impl RetryScheduler {
    /// Creates a new RetryScheduler with default retry queue settings.
    pub fn new() -> Self {
        Self {
            retry_queue: RetryQueue::new(),
        }
    }

    /// Creates a new RetryScheduler with a custom retry queue.
    pub fn with_retry_queue(retry_queue: RetryQueue) -> Self {
        Self { retry_queue }
    }

    /// Processes due retry entries.
    ///
    /// For each entry where `next_retry <= now`:
    /// - If `attempt >= max_attempts`: removes the entry and marks delivery as permanently failed
    /// - Otherwise: increments attempt, reschedules with backoff, and adds to ready_ids
    ///
    /// Returns a `RetryTickResult` with counts and ready message IDs.
    pub fn tick(
        &self,
        storage: &Storage,
        rng: &dyn crate::rng::SecureRng,
    ) -> Result<RetryTickResult, StorageError> {
        let now = storage.clock().unix_seconds();
        let due_entries = storage.get_due_retries(now)?;

        let mut result = RetryTickResult {
            due: due_entries.len(),
            ..RetryTickResult::default()
        };

        for entry in due_entries {
            if entry.is_max_attempts_exceeded() {
                // Remove permanently failed entry
                storage.delete_retry_entry(&entry.message_id)?;
                // Update delivery status to permanent failure
                let _ = storage.update_delivery_status(
                    &entry.message_id,
                    &DeliveryStatus::Failed {
                        reason: "max retries exceeded".to_string(),
                    },
                    now,
                );
                result.expired += 1;
            } else {
                // Reschedule with exponential backoff + jitter
                let next_retry =
                    self.retry_queue
                        .next_retry_time_with_jitter(now, entry.attempt + 1, rng);
                storage.increment_retry_attempt(&entry.message_id, next_retry)?;
                result.ready_ids.push(entry.message_id);
                result.rescheduled += 1;
            }
        }

        Ok(result)
    }
}

impl Default for RetryScheduler {
    fn default() -> Self {
        Self::new()
    }
}
