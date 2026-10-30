// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Durable per-origin journal positions and per-conflict-key receive floors.

use super::*;
use crate::sync::device_sync::ordering;

impl DeviceSyncOrchestrator<'_> {
    pub(crate) fn pending_batches(
        &self,
        target: &[u8; 32],
    ) -> Result<Vec<Vec<u8>>, DeviceSyncError> {
        let Some(state) = self.device_states.get(target) else {
            return Ok(Vec::new());
        };
        ordering::batches(
            state.pending_items(),
            state.pending_sequences(),
            self.current_device.device_id(),
            target,
        )
    }

    pub(crate) fn decode_incoming_batch(
        &self,
        bytes: &[u8],
        sender: &[u8; 32],
    ) -> Result<Vec<(SyncItem, Option<u64>)>, DeviceSyncError> {
        ordering::decode(bytes, sender, self.current_device.device_id())
    }

    pub(crate) fn apply_incoming_batch(
        &mut self,
        items: Vec<(SyncItem, Option<u64>)>,
        sender: &[u8; 32],
        apply: impl FnOnce(Vec<SyncItem>) -> Result<usize, DeviceSyncError>,
    ) -> Result<usize, DeviceSyncError> {
        self.receive_transaction(items, sender, apply)
    }

    pub(super) fn process_ordered_items(
        &mut self,
        items: Vec<(SyncItem, Option<u64>)>,
        sender: &[u8; 32],
    ) -> Result<Vec<SyncItem>, DeviceSyncError> {
        self.receive_transaction(items, sender, Ok)
    }

    fn receive_transaction<T>(
        &mut self,
        items: Vec<(SyncItem, Option<u64>)>,
        sender: &[u8; 32],
        apply: impl FnOnce(Vec<SyncItem>) -> Result<T, DeviceSyncError>,
    ) -> Result<T, DeviceSyncError> {
        self.state_transaction(|current| {
            let accepted = current.filter_ordered_items(items, sender)?;
            if let Some(state) = current.device_states.get(sender) {
                current
                    .storage
                    .sync()
                    .save_device_sync_state(state)
                    .map_err(|error| DeviceSyncError::Serialization(error.to_string()))?;
            }
            current
                .storage
                .sync()
                .save_field_timestamps(&current.field_timestamps)
                .map_err(|error| DeviceSyncError::Serialization(error.to_string()))?;
            apply(accepted)
        })
    }

    pub(super) fn state_transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, DeviceSyncError>,
    ) -> Result<T, DeviceSyncError> {
        self.storage
            .begin_transaction()
            .map_err(|error| DeviceSyncError::Serialization(error.to_string()))?;
        let states = self.device_states.clone();
        let stamps = self.field_timestamps.clone();
        let vector = self.version_vector.clone();
        let result = (|| {
            // The write lock precedes reload so cached handles cannot reuse sequences or erase newer floors.
            self.refresh_persisted_state()?;
            let result = operation(self)?;
            self.storage
                .commit()
                .map_err(|error| DeviceSyncError::Serialization(error.to_string()))?;
            Ok(result)
        })();
        if result.is_err() {
            self.storage.rollback();
            self.device_states = states;
            self.field_timestamps = stamps;
            self.version_vector = vector;
        }
        result
    }

    fn filter_ordered_items(
        &mut self,
        items: Vec<(SyncItem, Option<u64>)>,
        sender: &[u8; 32],
    ) -> Result<Vec<SyncItem>, DeviceSyncError> {
        let state = self
            .device_states
            .entry(*sender)
            .or_insert_with(|| InterDeviceSyncState::new(*sender));
        let mut applied = Vec::new();
        for (item, sequence) in items {
            let key = Self::conflict_key(&item);
            let floor = state.received_sequence(&key);
            if floor.is_some_and(|floor| sequence.is_none_or(|sequence| sequence <= floor)) {
                continue;
            }
            if let Some(sequence) = sequence {
                state.remember_sequence(key.clone(), sequence)?;
            }
            let incoming = FieldStamp {
                timestamp: item.timestamp(),
                device_id: *sender,
            };
            let local = self.field_timestamps.get(&key).copied();
            if local.is_none_or(|local| {
                incoming >= local || (sequence.is_some() && local.device_id == *sender)
            }) {
                self.field_timestamps.insert(key, incoming);
                applied.push(item);
            }
        }
        Ok(applied)
    }
}
