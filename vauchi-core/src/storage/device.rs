// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device, sync, and replay-nonce storage forwarders.
//!
//! Device-info / device-registry persistence now lives in
//! [`DeviceStore`](super::DeviceStore) and sync state in
//! [`SyncStore`](super::SyncStore); the methods below forward to them while
//! call sites migrate (Phase 2 of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`). Replay-nonce defense
//! (ADR-029) still lives here pending its own exchange/security store.

use super::{Storage, StorageError};
use crate::identity::device::DeviceRegistry;
use crate::sync::device_sync::{FieldStamp, InterDeviceSyncState, VersionVector};

impl Storage {
    /// Forwards to [`DeviceStore::save_device_info`].
    pub fn save_device_info(
        &self,
        device_id: &[u8; 32],
        device_index: u32,
        device_name: &str,
        created_at: u64,
    ) -> Result<(), StorageError> {
        self.device()
            .save_device_info(device_id, device_index, device_name, created_at)
    }

    /// Forwards to [`DeviceStore::load_device_info`].
    #[allow(clippy::type_complexity)]
    pub fn load_device_info(&self) -> Result<Option<([u8; 32], u32, String, u64)>, StorageError> {
        self.device().load_device_info()
    }

    /// Forwards to [`DeviceStore::has_device_info`].
    pub fn has_device_info(&self) -> Result<bool, StorageError> {
        self.device().has_device_info()
    }

    /// Forwards to [`DeviceStore::save_device_registry`].
    pub fn save_device_registry(&self, registry: &DeviceRegistry) -> Result<(), StorageError> {
        self.device().save_device_registry(registry)
    }

    /// Forwards to [`DeviceStore::load_device_registry`].
    pub fn load_device_registry(&self) -> Result<Option<DeviceRegistry>, StorageError> {
        self.device().load_device_registry()
    }

    /// Forwards to [`DeviceStore::load_device_registry_json`].
    pub fn load_device_registry_json(&self) -> Result<Option<String>, StorageError> {
        self.device().load_device_registry_json()
    }

    /// Forwards to [`DeviceStore::has_device_registry`].
    pub fn has_device_registry(&self) -> Result<bool, StorageError> {
        self.device().has_device_registry()
    }

    /// Forwards to [`SyncStore::save_device_sync_state`].
    pub fn save_device_sync_state(&self, state: &InterDeviceSyncState) -> Result<(), StorageError> {
        self.sync().save_device_sync_state(state)
    }

    /// Forwards to [`SyncStore::load_device_sync_state`].
    pub fn load_device_sync_state(
        &self,
        device_id: &[u8; 32],
    ) -> Result<Option<InterDeviceSyncState>, StorageError> {
        self.sync().load_device_sync_state(device_id)
    }

    /// Forwards to [`SyncStore::list_device_sync_states`].
    pub fn list_device_sync_states(&self) -> Result<Vec<InterDeviceSyncState>, StorageError> {
        self.sync().list_device_sync_states()
    }

    /// Forwards to [`SyncStore::delete_device_sync_state`].
    pub fn delete_device_sync_state(&self, device_id: &[u8; 32]) -> Result<bool, StorageError> {
        self.sync().delete_device_sync_state(device_id)
    }

    /// Forwards to [`SyncStore::save_version_vector`].
    pub fn save_version_vector(&self, vector: &VersionVector) -> Result<(), StorageError> {
        self.sync().save_version_vector(vector)
    }

    /// Forwards to [`SyncStore::load_version_vector`].
    pub fn load_version_vector(&self) -> Result<Option<VersionVector>, StorageError> {
        self.sync().load_version_vector()
    }

    /// Forwards to [`SyncStore::save_field_timestamps`].
    pub fn save_field_timestamps(
        &self,
        timestamps: &std::collections::HashMap<String, FieldStamp>,
    ) -> Result<(), StorageError> {
        self.sync().save_field_timestamps(timestamps)
    }

    /// Forwards to [`SyncStore::load_field_timestamps`].
    pub fn load_field_timestamps(
        &self,
    ) -> Result<std::collections::HashMap<String, FieldStamp>, StorageError> {
        self.sync().load_field_timestamps()
    }

    /// Wipes all device-specific data from storage.
    ///
    /// Delegates the `device_info` row to [`DeviceStore::clear_device_info`] and
    /// the sync-owned tables to [`SyncStore::wipe_for_device_reset`]. Used
    /// during identity deletion or device unlinking.
    pub fn wipe_device_data(&self) -> Result<(), StorageError> {
        self.device().clear_device_info()?;
        self.sync().wipe_for_device_reset()
    }

    /// Forwards to [`SyncStore::save_sync_checkpoint`].
    pub fn save_sync_checkpoint(
        &self,
        target_device_id: &[u8; 32],
        items: &[crate::sync::device_sync::SyncItem],
        sent_count: usize,
    ) -> Result<(), StorageError> {
        self.sync()
            .save_sync_checkpoint(target_device_id, items, sent_count)
    }

    /// Forwards to [`SyncStore::load_sync_checkpoint`].
    pub fn load_sync_checkpoint(
        &self,
        target_device_id: &[u8; 32],
    ) -> Result<Option<(Vec<crate::sync::device_sync::SyncItem>, usize)>, StorageError> {
        self.sync().load_sync_checkpoint(target_device_id)
    }

    /// Forwards to [`SyncStore::clear_sync_checkpoint`].
    pub fn clear_sync_checkpoint(&self, target_device_id: &[u8; 32]) -> Result<(), StorageError> {
        self.sync().clear_sync_checkpoint(target_device_id)
    }

    /// Forwards to [`SyncStore::save_batch_checkpoint`].
    pub fn save_batch_checkpoint(
        &self,
        batch_id: &str,
        total_items: usize,
        processed_items: usize,
        state_json: &str,
    ) -> Result<(), StorageError> {
        self.sync()
            .save_batch_checkpoint(batch_id, total_items, processed_items, state_json)
    }

    /// Forwards to [`SyncStore::load_batch_checkpoint`].
    pub fn load_batch_checkpoint(
        &self,
        batch_id: &str,
    ) -> Result<Option<(usize, usize, String)>, StorageError> {
        self.sync().load_batch_checkpoint(batch_id)
    }

    /// Forwards to [`SyncStore::update_batch_checkpoint`].
    pub fn update_batch_checkpoint(
        &self,
        batch_id: &str,
        processed_items: usize,
        state_json: &str,
    ) -> Result<(), StorageError> {
        self.sync()
            .update_batch_checkpoint(batch_id, processed_items, state_json)
    }

    /// Forwards to [`SyncStore::clear_batch_checkpoint`].
    pub fn clear_batch_checkpoint(&self, batch_id: &str) -> Result<(), StorageError> {
        self.sync().clear_batch_checkpoint(batch_id)
    }

    // === Replay Nonce Operations (V3 replay_nonces) ===

    /// Forwards to [`ReplayStore::save_replay_nonce`].
    pub fn save_replay_nonce(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.replay()
            .save_replay_nonce(contact_id, nonce, timestamp)
    }

    /// Forwards to [`ReplayStore::test_insert_malformed_replay_nonce`].
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert_malformed_replay_nonce(
        &self,
        contact_id: &str,
        bad_nonce: &[u8],
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.replay()
            .test_insert_malformed_replay_nonce(contact_id, bad_nonce, timestamp)
    }

    /// Forwards to [`ReplayStore::is_replay_nonce`].
    pub fn is_replay_nonce(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
    ) -> Result<bool, StorageError> {
        self.replay().is_replay_nonce(contact_id, nonce)
    }

    /// Forwards to [`ReplayStore::load_replay_nonces`].
    pub fn load_replay_nonces(
        &self,
        contact_id: &str,
    ) -> Result<Vec<([u8; 32], u64)>, StorageError> {
        self.replay().load_replay_nonces(contact_id)
    }

    /// Forwards to [`ReplayStore::cleanup_replay_nonces`].
    pub fn cleanup_replay_nonces(&self, cutoff: u64) -> Result<usize, StorageError> {
        self.replay().cleanup_replay_nonces(cutoff)
    }
}
