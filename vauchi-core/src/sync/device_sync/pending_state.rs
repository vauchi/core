// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{DeviceSyncError, MAX_SYNC_ITEMS_PER_BATCH, SyncItem, bytes_array_32};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tracks synchronization state with another device.
///
/// Each device maintains one InterDeviceSyncState per other linked device
/// to track what has been synced and what is pending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterDeviceSyncState {
    /// ID of the target device.
    #[serde(with = "bytes_array_32")]
    device_id: [u8; 32],
    /// Items pending sync to this device.
    pending_items: Vec<SyncItem>,
    /// Last sync version number.
    last_sync_version: u64,
    #[serde(default, skip_serializing_if = "all_legacy")]
    pending_sequences: Vec<Option<u64>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    received_sequences: HashMap<String, u64>,
}

impl InterDeviceSyncState {
    /// Creates a new sync state for the given device.
    pub fn new(device_id: [u8; 32]) -> Self {
        InterDeviceSyncState {
            device_id,
            pending_items: Vec::new(),
            last_sync_version: 0,
            pending_sequences: Vec::new(),
            received_sequences: HashMap::new(),
        }
    }

    /// Returns the target device ID.
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Returns pending items to sync.
    pub fn pending_items(&self) -> &[SyncItem] {
        &self.pending_items
    }

    /// Returns the last sync version.
    pub fn last_sync_version(&self) -> u64 {
        self.last_sync_version
    }

    /// Queues an item for sync to this device.
    pub fn queue_item(&mut self, item: SyncItem) {
        self.pending_items.push(item);
        self.pending_sequences.push(None);
    }

    /// Marks items as synced up to the given version.
    pub fn mark_synced(&mut self, version: u64) {
        let mut items = Vec::new();
        let mut sequences = Vec::new();
        for (item, sequence) in self
            .pending_items
            .drain(..)
            .zip(self.pending_sequences.drain(..))
        {
            if sequence.is_some_and(|sequence| sequence > version) {
                items.push(item);
                sequences.push(sequence);
            }
        }
        self.pending_items = items;
        self.pending_sequences = sequences;
        self.last_sync_version = self.last_sync_version.max(version);
    }

    /// Serializes the sync state to JSON.
    // INVARIANT: derived Serialize with string-keyed maps; serde_json fails
    // only on non-string map keys (vauchi/private#761).
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("InterDeviceSyncState serialization should not fail")
    }

    /// Deserializes sync state from JSON.
    ///
    /// Tolerant on the pending queue: an unknown or malformed pending
    /// item (written by a newer build) is skipped instead of failing the
    /// whole state restore — losing one future-variant item is strictly
    /// better than losing every pending item with it.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        #[derive(Deserialize)]
        struct TolerantState {
            #[serde(with = "bytes_array_32")]
            device_id: [u8; 32],
            pending_items: Vec<serde_json::Value>,
            last_sync_version: u64,
            #[serde(default)]
            pending_sequences: Option<Vec<Option<u64>>>,
            #[serde(default)]
            received_sequences: HashMap<String, u64>,
        }
        let raw: TolerantState = serde_json::from_str(json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
        if raw.pending_items.len() > MAX_SYNC_ITEMS_PER_BATCH {
            return Err(DeviceSyncError::Deserialization(format!(
                "pending queue exceeds {MAX_SYNC_ITEMS_PER_BATCH} items"
            )));
        }
        let sequences = raw
            .pending_sequences
            .unwrap_or_else(|| vec![None; raw.pending_items.len()]);
        if sequences.len() != raw.pending_items.len()
            || raw.received_sequences.len() > MAX_SYNC_ITEMS_PER_BATCH
            || raw
                .received_sequences
                .values()
                .any(|sequence| *sequence == 0)
        {
            return Err(DeviceSyncError::Deserialization(
                "invalid persisted device ordering state".into(),
            ));
        }
        super::ordering::validate_sequences(&sequences)?;
        let mut pending_items = Vec::new();
        let mut pending_sequences = Vec::new();
        for (value, sequence) in raw.pending_items.into_iter().zip(sequences) {
            if let Ok(item) = serde_json::from_value::<SyncItem>(value) {
                pending_items.push(item);
                pending_sequences.push(sequence);
            }
        }
        Ok(Self {
            device_id: raw.device_id,
            pending_items,
            pending_sequences,
            last_sync_version: raw.last_sync_version,
            received_sequences: raw.received_sequences,
        })
    }
    pub(crate) fn queue_ordered_item(&mut self, item: SyncItem, sequence: u64) {
        self.pending_items.push(item);
        self.pending_sequences.push(Some(sequence));
    }

    pub(crate) fn pending_sequences(&self) -> &[Option<u64>] {
        &self.pending_sequences
    }

    pub(crate) fn received_sequence(&self, key: &str) -> Option<u64> {
        self.received_sequences.get(key).copied()
    }

    pub(crate) fn remember_sequence(
        &mut self,
        key: String,
        sequence: u64,
    ) -> Result<(), DeviceSyncError> {
        if self.received_sequences.len() >= MAX_SYNC_ITEMS_PER_BATCH
            && !self.received_sequences.contains_key(&key)
        {
            return Err(DeviceSyncError::Deserialization(
                "device ordering key limit exceeded".into(),
            ));
        }
        self.received_sequences.insert(key, sequence);
        Ok(())
    }
}

fn all_legacy(sequences: &[Option<u64>]) -> bool {
    sequences.iter().all(Option::is_none)
}
