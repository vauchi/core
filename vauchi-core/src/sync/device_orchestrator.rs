// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device Sync Orchestrator
//!
//! Orchestrates synchronization between devices belonging to the same identity.
//! Manages queuing of SyncItems, tracking sync state per device, and version vectors.

use std::collections::HashMap;

use crate::contact_card::ContactCard;
use crate::crypto::{encryption, SymmetricKey, HKDF};
use crate::identity::device::{DeviceInfo, DeviceRegistry};
use crate::storage::Storage;
use crate::sync::device_sync::{
    DeviceSyncError, DeviceSyncPayload, InterDeviceSyncState, SyncItem, VersionVector,
};

/// Domain separation for device-to-device encryption key derivation.
const DEVICE_SYNC_INFO: &[u8] = b"Vauchi_DeviceSync";

/// Orchestrates synchronization between devices of the same identity.
///
/// Each instance manages sync state for a single device, tracking what
/// needs to be synced to other linked devices.
pub struct DeviceSyncOrchestrator<'a> {
    /// Storage for persisting state.
    storage: &'a Storage,
    /// Current device info.
    current_device: DeviceInfo,
    /// Device registry (all linked devices).
    registry: DeviceRegistry,
    /// Per-device sync state (device_id -> sync state).
    device_states: HashMap<[u8; 32], InterDeviceSyncState>,
    /// Local version vector for causality tracking.
    version_vector: VersionVector,
    /// Timestamps of the last change to each field (for conflict resolution).
    /// Key is the field identifier (e.g., "field:email" or "contact:abc123").
    field_timestamps: HashMap<String, u64>,
}

impl<'a> DeviceSyncOrchestrator<'a> {
    /// Creates a new device sync orchestrator.
    pub fn new(storage: &'a Storage, current_device: DeviceInfo, registry: DeviceRegistry) -> Self {
        // Initialize sync states for all other devices
        let mut device_states = HashMap::new();
        for device in registry.active_devices() {
            if device.device_id != *current_device.device_id() {
                device_states.insert(
                    device.device_id,
                    InterDeviceSyncState::new(device.device_id),
                );
            }
        }

        DeviceSyncOrchestrator {
            storage,
            current_device,
            registry,
            device_states,
            version_vector: VersionVector::new(),
            field_timestamps: HashMap::new(),
        }
    }

    /// Creates an orchestrator and loads existing state from storage.
    pub fn load(
        storage: &'a Storage,
        current_device: DeviceInfo,
        registry: DeviceRegistry,
    ) -> Result<Self, DeviceSyncError> {
        let mut orchestrator = Self::new(storage, current_device, registry);

        // Load existing sync states from storage
        let stored_states = storage
            .list_device_sync_states()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        for state in stored_states {
            orchestrator.device_states.insert(*state.device_id(), state);
        }

        // Load version vector if exists
        if let Some(vector) = storage
            .load_version_vector()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
        {
            orchestrator.version_vector = vector;
        }

        Ok(orchestrator)
    }

    /// Records a local change to be synced to other devices.
    ///
    /// Queues the SyncItem for all other linked devices and increments
    /// the local version vector.
    pub fn record_local_change(&mut self, item: SyncItem) -> Result<(), DeviceSyncError> {
        // Track timestamp for conflict resolution
        let key = Self::conflict_key(&item);
        let timestamp = item.timestamp();
        self.field_timestamps.insert(key, timestamp);

        // Increment our version
        self.version_vector
            .increment(self.current_device.device_id());

        // Queue item for all other devices
        for state in self.device_states.values_mut() {
            state.queue_item(item.clone());
        }

        // Persist updated states
        for state in self.device_states.values() {
            self.storage
                .save_device_sync_state(state)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }

        // Persist version vector
        self.storage
            .save_version_vector(&self.version_vector)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

        Ok(())
    }

    /// Returns pending sync items for a specific device.
    pub fn pending_for_device(&self, device_id: &[u8; 32]) -> &[SyncItem] {
        self.device_states
            .get(device_id)
            .map(|s| s.pending_items())
            .unwrap_or(&[])
    }

    /// Returns all device IDs that have pending items.
    pub fn devices_with_pending(&self) -> Vec<[u8; 32]> {
        self.device_states
            .iter()
            .filter(|(_, state)| !state.pending_items().is_empty())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Marks items as synced to a device.
    pub fn mark_synced(
        &mut self,
        device_id: &[u8; 32],
        version: u64,
    ) -> Result<(), DeviceSyncError> {
        if let Some(state) = self.device_states.get_mut(device_id) {
            state.mark_synced(version);
            self.storage
                .save_device_sync_state(state)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }
        Ok(())
    }

    /// Creates a full sync payload for a newly linked device.
    ///
    /// This includes all contacts and the user's own contact card.
    pub fn create_full_sync_payload(&self) -> Result<DeviceSyncPayload, DeviceSyncError> {
        // Load contacts from storage
        let contacts = self
            .storage
            .list_contacts()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        // Load own card from storage
        let own_card = self
            .storage
            .load_own_card()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .unwrap_or_else(|| ContactCard::new(""));

        // Get current version
        let version = self.version_vector.get(self.current_device.device_id());

        Ok(DeviceSyncPayload::new(&contacts, &own_card, version))
    }

    /// Applies a full sync payload received during device linking.
    ///
    /// This replaces the local state with the received data.
    pub fn apply_full_sync(&mut self, payload: DeviceSyncPayload) -> Result<(), DeviceSyncError> {
        // Save own card
        if !payload.own_card_json.is_empty() {
            let own_card: ContactCard = serde_json::from_str(&payload.own_card_json)
                .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
            self.storage
                .save_own_card(&own_card)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }

        // Save contacts
        for contact_data in &payload.contacts {
            let contact = contact_data.to_contact()?;
            self.storage
                .save_contact(&contact)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }

        // Update version vector to match received version
        self.version_vector
            .increment(self.current_device.device_id());

        self.storage
            .save_version_vector(&self.version_vector)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

        Ok(())
    }

    /// Returns the current device info.
    pub fn current_device(&self) -> &DeviceInfo {
        &self.current_device
    }

    /// Returns the device registry.
    pub fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    /// Returns the local version vector.
    pub fn version_vector(&self) -> &VersionVector {
        &self.version_vector
    }

    /// Adds a new device to track (called after device linking).
    pub fn add_device(&mut self, device_id: [u8; 32]) {
        self.device_states
            .entry(device_id)
            .or_insert_with(|| InterDeviceSyncState::new(device_id));
    }

    /// Removes a device from tracking (called after device revocation).
    pub fn remove_device(&mut self, device_id: &[u8; 32]) -> Result<(), DeviceSyncError> {
        self.device_states.remove(device_id);
        self.storage
            .delete_device_sync_state(device_id)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        Ok(())
    }

    // ============================================================
    // Device-to-device encryption (Phase 4)
    // ============================================================

    /// Encrypts data for another device using ECDH + AES-GCM.
    ///
    /// Uses the current device's exchange key to perform ECDH with the target
    /// device's public key, derives an encryption key via HKDF, and encrypts
    /// the data with AES-256-GCM.
    pub fn encrypt_for_device(
        &self,
        target_public_key: &[u8; 32],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, DeviceSyncError> {
        let encryption_key = self.derive_shared_key(target_public_key);
        encryption::encrypt(&encryption_key, plaintext)
            .map_err(|e| DeviceSyncError::Encryption(e.to_string()))
    }

    /// Decrypts data from another device using ECDH + AES-GCM.
    ///
    /// Uses the current device's exchange key to perform ECDH with the sender
    /// device's public key, derives a decryption key via HKDF, and decrypts
    /// the data with AES-256-GCM.
    pub fn decrypt_from_device(
        &self,
        sender_public_key: &[u8; 32],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, DeviceSyncError> {
        let decryption_key = self.derive_shared_key(sender_public_key);
        encryption::decrypt(&decryption_key, ciphertext)
            .map_err(|e| DeviceSyncError::Decryption(e.to_string()))
    }

    /// Derives a shared symmetric key from ECDH with another device.
    fn derive_shared_key(&self, their_public_key: &[u8; 32]) -> SymmetricKey {
        // ECDH: our_secret * their_public -> shared_secret
        let shared_secret = self
            .current_device
            .exchange_keypair()
            .diffie_hellman(their_public_key);

        // HKDF to derive encryption key
        let key_bytes = HKDF::derive_key(None, &shared_secret, DEVICE_SYNC_INFO);
        SymmetricKey::from_bytes(key_bytes)
    }

    // ============================================================
    // Conflict Resolution (Phase 5)
    // ============================================================

    /// Processes incoming sync items from another device.
    ///
    /// Uses last-write-wins conflict resolution:
    /// - If incoming item has a newer timestamp than local, apply it
    /// - If incoming item has an older timestamp, reject it
    /// - Different fields/items don't conflict
    ///
    /// Returns the list of items that were applied.
    pub fn process_incoming(
        &mut self,
        items: Vec<SyncItem>,
    ) -> Result<Vec<SyncItem>, DeviceSyncError> {
        let mut applied = Vec::new();

        for item in items {
            let key = Self::conflict_key(&item);
            let incoming_timestamp = item.timestamp();

            // Check if we have a local timestamp for this key
            let local_timestamp = self.field_timestamps.get(&key).copied().unwrap_or(0);

            // Last-write-wins: only apply if incoming is newer
            if incoming_timestamp > local_timestamp {
                // Update our local timestamp
                self.field_timestamps.insert(key, incoming_timestamp);

                // Add to applied list
                applied.push(item);
            }
            // If incoming is older or equal, we reject it (don't add to applied)
        }

        Ok(applied)
    }

    /// Generates a conflict key for a SyncItem.
    ///
    /// Items with the same key are considered conflicting (only one can win).
    /// Items with different keys are independent and can both be applied.
    fn conflict_key(item: &SyncItem) -> String {
        match item {
            SyncItem::ContactAdded { contact_data, .. } => format!("contact:{}", contact_data.id),
            SyncItem::ContactRemoved { contact_id, .. } => format!("contact:{}", contact_id),
            SyncItem::CardUpdated { field_label, .. } => format!("field:{}", field_label),
            SyncItem::VisibilityChanged { contact_id, .. } => format!("visibility:{}", contact_id),
        }
    }

    // ============================================================
    // Bidirectional Sync (Phase 6)
    // ============================================================

    /// Creates a sync message containing all pending items for a target device.
    ///
    /// This is used when reconnecting to send all queued changes to another device.
    pub fn create_sync_message(
        &self,
        device_id: &[u8; 32],
    ) -> Result<PendingSyncMessage, DeviceSyncError> {
        let items = self.pending_for_device(device_id).to_vec();
        let version = self.version_vector.get(self.current_device.device_id());

        Ok(PendingSyncMessage {
            items,
            version,
            sender_device_id: *self.current_device.device_id(),
        })
    }
}

/// A message containing pending sync items to send to another device.
#[derive(Debug, Clone)]
pub struct PendingSyncMessage {
    /// The pending sync items.
    pub items: Vec<SyncItem>,
    /// Version number for deduplication.
    pub version: u64,
    /// The sender device ID.
    pub sender_device_id: [u8; 32],
}
