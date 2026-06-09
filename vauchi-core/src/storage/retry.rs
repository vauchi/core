// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`RetryStore`](super::RetryStore).

use super::error::RetryEntry;
use super::{Storage, StorageError};

impl Storage {
    /// Creates a new retry entry (payload encrypted).
    pub fn create_retry_entry(&self, entry: &RetryEntry) -> Result<(), StorageError> {
        self.retries().create_retry_entry(entry)
    }
    /// Gets a retry entry by message ID.
    pub fn get_retry_entry(&self, message_id: &str) -> Result<Option<RetryEntry>, StorageError> {
        self.retries().get_retry_entry(message_id)
    }
    /// Gets all retry entries that are due for retry (next_retry <= now).
    pub fn get_due_retries(&self, now: u64) -> Result<Vec<RetryEntry>, StorageError> {
        self.retries().get_due_retries(now)
    }
    /// Gets all retry entries for a recipient.
    pub fn get_retry_entries_for_recipient(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<RetryEntry>, StorageError> {
        self.retries().get_retry_entries_for_recipient(recipient_id)
    }
    /// Gets all retry entries.
    pub fn get_all_retry_entries(&self) -> Result<Vec<RetryEntry>, StorageError> {
        self.retries().get_all_retry_entries()
    }
    /// Increments the retry attempt count and updates the next retry time.
    pub fn increment_retry_attempt(
        &self,
        message_id: &str,
        next_retry: u64,
    ) -> Result<bool, StorageError> {
        self.retries()
            .increment_retry_attempt(message_id, next_retry)
    }
    /// Deletes a retry entry.
    pub fn delete_retry_entry(&self, message_id: &str) -> Result<bool, StorageError> {
        self.retries().delete_retry_entry(message_id)
    }
    /// Counts the total number of retry entries.
    pub fn count_retry_entries(&self) -> Result<usize, StorageError> {
        self.retries().count_retry_entries()
    }
    /// Updates the next retry time for an entry (for manual retry).
    pub fn update_retry_next_time(
        &self,
        message_id: &str,
        next_retry: u64,
    ) -> Result<bool, StorageError> {
        self.retries()
            .update_retry_next_time(message_id, next_retry)
    }
}
