// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`PendingStore`](super::PendingStore).

use super::error::{PendingUpdate, UpdateStatus};
use super::{Storage, StorageError};

impl Storage {
    /// Queues a pending update for a contact (payload encrypted).
    pub fn queue_update(&self, update: &PendingUpdate) -> Result<(), StorageError> {
        self.pending().queue_update(update)
    }
    /// Gets pending updates for a contact.
    pub fn get_pending_updates(
        &self,
        contact_id: &str,
    ) -> Result<Vec<PendingUpdate>, StorageError> {
        self.pending().get_pending_updates(contact_id)
    }
    /// Gets all pending updates.
    pub fn get_all_pending_updates(&self) -> Result<Vec<PendingUpdate>, StorageError> {
        self.pending().get_all_pending_updates()
    }
    /// Marks an update as sent (removes it from the queue).
    pub fn mark_update_sent(&self, update_id: &str) -> Result<bool, StorageError> {
        self.pending().mark_update_sent(update_id)
    }
    /// Gets a single pending update by ID.
    pub fn get_pending_update(
        &self,
        update_id: &str,
    ) -> Result<Option<PendingUpdate>, StorageError> {
        self.pending().get_pending_update(update_id)
    }
    /// Updates the status of a pending update.
    pub fn update_pending_status(
        &self,
        update_id: &str,
        status: UpdateStatus,
        retry_count: u32,
    ) -> Result<bool, StorageError> {
        self.pending()
            .update_pending_status(update_id, status, retry_count)
    }
    /// Counts pending updates for a contact.
    pub fn count_pending_updates(&self, contact_id: &str) -> Result<usize, StorageError> {
        self.pending().count_pending_updates(contact_id)
    }
    /// Deletes a pending update by ID.
    pub fn delete_pending_update(&self, id: &str) -> Result<bool, StorageError> {
        self.pending().delete_pending_update(id)
    }
    /// Counts all pending updates across all contacts.
    pub fn count_all_pending_updates(&self) -> Result<usize, StorageError> {
        self.pending().count_all_pending_updates()
    }
    /// Deletes all pending updates for a contact.
    ///
    /// Returns the number of deleted updates.
    pub fn delete_pending_updates_for_contact(
        &self,
        contact_id: &str,
    ) -> Result<usize, StorageError> {
        self.pending()
            .delete_pending_updates_for_contact(contact_id)
    }
    /// Clears all pending updates.
    ///
    /// Returns the number of deleted updates.
    pub fn clear_all_pending_updates(&self) -> Result<usize, StorageError> {
        self.pending().clear_all_pending_updates()
    }
    /// Gets all pending updates grouped by target relay URL.
    ///
    /// Returns a `BTreeMap` where keys are `Option<String>`:
    /// - `Some(url)` → updates targeted at a specific relay
    /// - `None` → updates for the home relay (no specific target)
    ///
    /// Each group is ordered by `created_at` (ascending).
    pub fn get_pending_updates_grouped_by_relay(
        &self,
    ) -> Result<std::collections::BTreeMap<Option<String>, Vec<PendingUpdate>>, StorageError> {
        self.pending().get_pending_updates_grouped_by_relay()
    }
    /// Gets pending updates by status.
    pub fn get_pending_updates_by_status(
        &self,
        status: &str,
    ) -> Result<Vec<PendingUpdate>, StorageError> {
        self.pending().get_pending_updates_by_status(status)
    }
}
