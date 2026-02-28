// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OfflineManager: offline queuing and connectivity flush.
//!
//! Queues outbound updates when offline and returns them for sending
//! when connectivity is restored. Enforces queue capacity limits.

use crate::storage::{OfflineQueue, PendingUpdate, Storage, StorageError, UpdateStatus};

/// Manages offline queuing and flush of pending updates.
pub struct OfflineManager {
    offline_queue: OfflineQueue,
}

impl OfflineManager {
    /// Creates a new OfflineManager with default queue settings.
    pub fn new() -> Self {
        Self {
            offline_queue: OfflineQueue::new(),
        }
    }

    /// Creates a new OfflineManager with a custom offline queue.
    pub fn with_offline_queue(offline_queue: OfflineQueue) -> Self {
        Self { offline_queue }
    }

    /// Queues an update for sending.
    ///
    /// If `is_online` is true, the update is queued with `Sending` status
    /// so the caller knows to send it immediately.
    /// If `is_online` is false, the update is queued with `Pending` status
    /// for later flush when connectivity is restored.
    ///
    /// Returns an error if the queue is full.
    pub fn send_or_queue(
        &self,
        storage: &Storage,
        mut update: PendingUpdate,
        is_online: bool,
    ) -> Result<(), StorageError> {
        if self.offline_queue.is_full(storage)? {
            return Err(StorageError::QueueFull(
                "Offline queue is at capacity".to_string(),
            ));
        }

        if is_online {
            update.status = UpdateStatus::Sending;
        } else {
            update.status = UpdateStatus::Pending;
        }

        storage.queue_update(&update)
    }

    /// Returns all pending updates for sending (flush on reconnect).
    ///
    /// Updates are returned in insertion order. The caller is responsible
    /// for sending them and removing them from the queue on success.
    pub fn flush_queue(&self, storage: &Storage) -> Result<Vec<PendingUpdate>, StorageError> {
        storage.get_pending_updates_by_status("pending")
    }

    /// Returns the remaining capacity in the offline queue.
    pub fn remaining_capacity(&self, storage: &Storage) -> Result<usize, StorageError> {
        self.offline_queue.remaining_capacity(storage)
    }

    /// Returns true if the queue is full.
    pub fn is_full(&self, storage: &Storage) -> Result<bool, StorageError> {
        self.offline_queue.is_full(storage)
    }
}

impl Default for OfflineManager {
    fn default() -> Self {
        Self::new()
    }
}
