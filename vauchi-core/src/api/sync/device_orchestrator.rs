// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device Sync Orchestrator
//!
//! Orchestrates synchronization between devices belonging to the same identity.
//! Manages queuing of SyncItems, tracking sync state per device, and version vectors.

use std::collections::HashMap;

use crate::contact_card::ContactCard;
use crate::crypto::{HKDF, SymmetricKey, encryption};
use crate::identity::device::{DeviceInfo, DeviceRegistry};
use crate::storage::Storage;
use crate::sync::device_sync::{
    ContactDeviceRegistrySyncData, ContactExchangeLocation, DeviceLinkIntent, DeviceSyncError,
    DeviceSyncPayload, FieldStamp, GroupSyncData, InterDeviceSyncState, PlaceSyncData, SyncItem,
    TagSyncData, VersionVector,
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
    /// Last-write stamp per field for conflict resolution: `(timestamp,
    /// device_id)`, compared lexicographically (ADR-020 LWW + device-id
    /// tie-break). Key is the field identifier (e.g., "field:email" or
    /// "contact:abc123"). Persisted via `Storage::save_field_timestamps`.
    field_timestamps: HashMap<String, FieldStamp>,
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
            .sync()
            .list_device_sync_states()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        for state in stored_states {
            orchestrator.device_states.insert(*state.device_id(), state);
        }

        // Load version vector if exists
        if let Some(vector) = storage
            .sync()
            .load_version_vector()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
        {
            orchestrator.version_vector = vector;
        }

        // Load conflict-resolution field timestamps so the LWW gate in
        // `process_incoming` survives across sync cycles (G3). Without this
        // a reloaded orchestrator starts empty and an older incoming change
        // would overwrite a newer local one.
        orchestrator.field_timestamps = storage
            .sync()
            .load_field_timestamps()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        Ok(orchestrator)
    }

    /// Records a local change to be synced to other devices.
    ///
    /// Queues the SyncItem for all other linked devices and increments
    /// the local version vector.
    pub fn record_local_change(&mut self, item: SyncItem) -> Result<(), DeviceSyncError> {
        self.stage_local_change(item);

        // Persist device states + version vector atomically (#105).
        // A transaction ensures either all state is saved or none, preventing
        // inconsistency if the process crashes mid-write.
        self.storage
            .begin_transaction()
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

        let result = self.persist_staged_state();
        match result {
            Ok(()) => self
                .storage
                .commit()
                .map_err(|e| DeviceSyncError::Serialization(e.to_string())),
            Err(error) => {
                self.storage.rollback();
                Err(error)
            }
        }
    }

    /// Atomically persist an expanded signed owner registry and queue it for
    /// every other active device, including devices linked before the newest
    /// one existed.
    pub fn persist_device_registry_change(
        storage: &'a Storage,
        identity: &crate::identity::Identity,
        registry: &DeviceRegistry,
        timestamp: u64,
    ) -> Result<(), DeviceSyncError> {
        if !registry.verify(&identity.signing_keypair().public_key()) {
            return Err(DeviceSyncError::Deserialization(
                "owner device registry signature is invalid".to_string(),
            ));
        }
        if let Some(current) = storage
            .device()
            .load_device_registry()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            && registry.version() <= current.version()
        {
            return Err(DeviceSyncError::Deserialization(format!(
                "owner device registry version {} is not newer than {}",
                registry.version(),
                current.version()
            )));
        }

        let mut orchestrator = Self::load(
            storage,
            identity.create_device_info(timestamp),
            registry.clone(),
        )?;
        let item = SyncItem::DeviceRegistryChanged {
            registry_json: registry.to_json(),
            version: registry.version(),
        };

        storage
            .begin_transaction()
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        let result = (|| {
            storage
                .device()
                .save_device_registry(registry)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            orchestrator.stage_local_change(item);
            orchestrator.persist_staged_state()
        })();

        match result {
            Ok(()) => storage
                .commit()
                .map_err(|e| DeviceSyncError::Serialization(e.to_string())),
            Err(error) => {
                storage.rollback();
                Err(error)
            }
        }
    }

    fn stage_local_change(&mut self, item: SyncItem) {
        // Track the last-write stamp for conflict resolution: this device
        // originated the change, so stamp it with our device id (ADR-020).
        let key = Self::conflict_key(&item);
        let stamp = FieldStamp {
            timestamp: item.timestamp(),
            device_id: *self.current_device.device_id(),
        };
        self.field_timestamps.insert(key, stamp);

        // Increment our version
        self.version_vector
            .increment(self.current_device.device_id());

        // Queue item for all other devices
        for state in self.device_states.values_mut() {
            state.queue_item(item.clone());
        }
    }

    fn persist_staged_state(&self) -> Result<(), DeviceSyncError> {
        for state in self.device_states.values() {
            self.storage
                .sync()
                .save_device_sync_state(state)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }
        self.storage
            .sync()
            .save_version_vector(&self.version_vector)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        self.storage
            .sync()
            .save_field_timestamps(&self.field_timestamps)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))
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
                .sync()
                .save_device_sync_state(state)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }
        Ok(())
    }

    /// Creates a full sync payload for a newly linked device.
    ///
    /// This includes contacts, the owner's card, and signed peer-device
    /// registries. Ratchet sessions are never cloned (ADR-064).
    pub fn create_full_sync_payload(
        &self,
        _intent: DeviceLinkIntent,
    ) -> Result<DeviceSyncPayload, DeviceSyncError> {
        // Load contacts from storage
        let contacts = self
            .storage
            .contacts()
            .list_contacts()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        // Load own card from storage
        let own_card = self
            .storage
            .contacts()
            .load_own_card()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .unwrap_or_else(|| ContactCard::new(""));

        // Load owner-private tags (ADR-051)
        let tags: Vec<TagSyncData> = self
            .storage
            .tags()
            .list_tags()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .iter()
            .map(TagSyncData::from_tag)
            .collect();

        // Load owner-private groups and their ADR-054 presentation state.
        let groups: Vec<GroupSyncData> = self
            .storage
            .labels()
            .load_all_groups()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .iter()
            .map(GroupSyncData::from_group)
            .collect();

        // Load named places + per-contact exchange locations (ADR-051)
        let places: Vec<PlaceSyncData> = self
            .storage
            .places()
            .list_places()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .iter()
            .map(PlaceSyncData::from_place)
            .collect();
        let exchange_locations: Vec<ContactExchangeLocation> = self
            .storage
            .list_exchange_locations()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .iter()
            .map(|(id, loc)| ContactExchangeLocation::from_parts(id, loc))
            .collect();

        let contact_device_registries = self
            .storage
            .device()
            .list_contact_device_registries()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .into_iter()
            .map(|(contact_id, broadcast)| ContactDeviceRegistrySyncData {
                contact_id,
                broadcast_json: broadcast.to_json(),
            })
            .collect();

        // Get current version
        let version = self.version_vector.get(self.current_device.device_id());

        Ok(DeviceSyncPayload::new(&contacts, &own_card, version)
            .with_tags(tags)
            .with_groups(groups)
            .with_places(places)
            .with_exchange_locations(exchange_locations)
            .with_contact_device_registries(contact_device_registries))
    }

    /// Applies a full sync payload received during device linking.
    ///
    /// This replaces the local state with the received data.
    /// All writes are wrapped in a transaction for atomicity (#105).
    pub fn apply_full_sync(&mut self, payload: DeviceSyncPayload) -> Result<(), DeviceSyncError> {
        self.storage
            .begin_transaction()
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

        let result = (|| {
            // Save own card
            if !payload.own_card_json.is_empty() {
                let own_card: ContactCard = serde_json::from_str(&payload.own_card_json)
                    .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
                self.storage
                    .contacts()
                    .save_own_card(&own_card)
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Save exchanged contacts
            for contact_data in &payload.contacts {
                let contact = contact_data.to_contact()?;
                self.storage
                    .contacts()
                    .save_contact(&contact)
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Save imported contacts
            for imported_data in &payload.imported_contacts {
                let contact = imported_data.to_contact()?;
                self.storage
                    .contacts()
                    .save_contact(&contact)
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Retain signed peer topology, never a live ratchet chain. The
            // newly linked device will independently bootstrap its own pair
            // sessions when it sends or receives.
            for registry_data in &payload.contact_device_registries {
                let contact = self
                    .storage
                    .contacts()
                    .load_contact(&registry_data.contact_id)
                    .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
                    .ok_or_else(|| {
                        DeviceSyncError::Deserialization(format!(
                            "registry references missing contact {}",
                            registry_data.contact_id
                        ))
                    })?;
                let public_key = contact.public_key().ok_or_else(|| {
                    DeviceSyncError::Deserialization(
                        "device registry references imported contact".into(),
                    )
                })?;
                let broadcast =
                    crate::identity::RegistryBroadcast::from_json(&registry_data.broadcast_json)
                        .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
                let already_current = self
                    .storage
                    .device()
                    .load_contact_device_registry(&registry_data.contact_id)
                    .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
                    .is_some_and(|stored| stored.version() >= broadcast.version());
                if !already_current {
                    self.storage
                        .device()
                        .save_contact_device_registry(
                            &registry_data.contact_id,
                            &broadcast,
                            public_key,
                            u64::MAX,
                        )
                        .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
                }
            }

            // Restore owner-private tags (ADR-051), preserving their ids.
            for tag_data in &payload.tags {
                self.storage
                    .tags()
                    .save_tag(&tag_data.to_tag())
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Restore owner-private groups (ADR-054), preserving their stable
            // ids, membership, visibility, presentation, and timestamps.
            for group in &payload.groups {
                self.storage
                    .labels()
                    .save_group(&group.to_group())
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Restore named places (ADR-051), preserving their ids.
            for place_data in &payload.places {
                self.storage
                    .places()
                    .save_place(&place_data.to_place())
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Restore per-contact exchange locations (contacts saved above exist).
            for loc in &payload.exchange_locations {
                self.storage
                    .save_exchange_location(&loc.contact_id, &loc.location())
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            }

            // Update version vector to match received version
            self.version_vector
                .increment(self.current_device.device_id());

            self.storage
                .sync()
                .save_version_vector(&self.version_vector)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.storage
                    .commit()
                    .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
                Ok(())
            }
            Err(e) => {
                self.storage.rollback();
                Err(e)
            }
        }
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
            .sync()
            .delete_device_sync_state(device_id)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        Ok(())
    }

    /// Encrypts data for another device using ECDH + XChaCha20-Poly1305.
    ///
    /// Uses the current device's exchange key to perform ECDH with the target
    /// device's public key, derives an encryption key via HKDF, and encrypts
    /// the data with XChaCha20-Poly1305.
    pub fn encrypt_for_device(
        &self,
        target_public_key: &[u8; 32],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, DeviceSyncError> {
        let encryption_key = self.derive_shared_key(target_public_key)?;
        encryption::encrypt(&encryption_key, plaintext)
            .map_err(|e| DeviceSyncError::Encryption(e.to_string()))
    }

    /// Decrypts data from another device using ECDH + XChaCha20-Poly1305.
    ///
    /// Uses the current device's exchange key to perform ECDH with the sender
    /// device's public key, derives a decryption key via HKDF, and decrypts
    /// the data.
    pub fn decrypt_from_device(
        &self,
        sender_public_key: &[u8; 32],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, DeviceSyncError> {
        let decryption_key = self.derive_shared_key(sender_public_key)?;
        encryption::decrypt(&decryption_key, ciphertext)
            .map_err(|e| DeviceSyncError::Decryption(e.to_string()))
    }

    /// Derives a shared symmetric key from ECDH with another device.
    fn derive_shared_key(
        &self,
        their_public_key: &[u8; 32],
    ) -> Result<SymmetricKey, DeviceSyncError> {
        // ECDH: our_secret * their_public -> shared_secret
        let shared_secret = self
            .current_device
            .exchange_keypair()
            .diffie_hellman(their_public_key)?;

        // HKDF to derive encryption key
        let key_bytes = HKDF::derive_key(None, &*shared_secret, DEVICE_SYNC_INFO);
        Ok(SymmetricKey::from_bytes(*key_bytes))
    }

    /// Processes incoming sync items from another device.
    ///
    /// ## Conflict Resolution Strategy: Last-Write-Wins (LWW) (#193)
    ///
    /// Each sync item carries a Unix millisecond timestamp set by the originating device.
    /// Conflict is determined by the `conflict_key()` function, which maps items to a
    /// namespace:id string (e.g., `"field:email"`, `"contact:abc123"`).
    ///
    /// Rules:
    /// - **Newer wins:** If incoming timestamp > local timestamp for the same key, apply it.
    /// - **Tie → higher device id wins (ADR-020):** If timestamps are equal, the
    ///   item from the lexicographically-higher device id wins — deterministic and
    ///   identical on every device, so concurrent same-ms edits converge.
    /// - **Independent fields coexist:** Items with different conflict keys never conflict.
    ///   E.g., updating email and phone simultaneously on two devices both succeed.
    /// - **Cross-type conflicts:** ContactAdded and ContactRemoved share the same
    ///   `"contact:{id}"` key, so a remove-after-add with a newer timestamp wins.
    ///
    /// Returns the list of items that were applied.
    pub fn process_incoming(
        &mut self,
        items: Vec<SyncItem>,
        sender_device_id: &[u8; 32],
    ) -> Result<Vec<SyncItem>, DeviceSyncError> {
        let mut applied = Vec::new();

        for item in items {
            let key = Self::conflict_key(&item);
            // Every item in a batch was authored by the sending device, so
            // the sender's id is the originating device id for the tie-break.
            let incoming = FieldStamp {
                timestamp: item.timestamp(),
                device_id: *sender_device_id,
            };

            // LWW + device-id tie-break (ADR-020): apply iff the incoming
            // (timestamp, device_id) is lexicographically greater than the
            // local stamp. An equal stamp (same write echoed back) is rejected.
            let local = self.field_timestamps.get(&key).copied();
            if local.is_none_or(|local| incoming > local) {
                self.field_timestamps.insert(key, incoming);
                applied.push(item);
            }
            // Older-or-equal incoming is rejected (not added to applied).
        }

        // Persist the updated timestamps so the LWW gate survives a reload
        // (G3). Idempotent when nothing applied.
        self.storage
            .sync()
            .save_field_timestamps(&self.field_timestamps)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

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
            SyncItem::ContactCardUpdated { contact_id, .. } => {
                format!("contact_card:{}", contact_id)
            }
            SyncItem::DeviceRegistryChanged { .. } => "device_registry".to_string(),
            SyncItem::CardUpdated { field_label, .. } => format!("field:{}", field_label),
            SyncItem::CardFieldRemoved { field_label, .. } => format!("field:{}", field_label),
            SyncItem::VisibilityChanged { contact_id, .. } => format!("visibility:{}", contact_id),
            SyncItem::GroupChanged { group_data, .. } => {
                format!("group:{}", group_data.id)
            }
            SyncItem::GroupDeleted { group_id, .. } => format!("group:{}", group_id),
            SyncItem::TagChanged { tag_data, .. } => format!("tag:{}", tag_data.id),
            SyncItem::TagDeleted { tag_id, .. } => format!("tag:{}", tag_id),
            SyncItem::ContactTrustChanged { contact_id, .. } => {
                format!("trust:{}", contact_id)
            }
            SyncItem::DeletionScheduled { .. } => "deletion:scheduled".to_string(),
            SyncItem::DeletionCancelled { .. } => "deletion:cancelled".to_string(),
            SyncItem::PersonalNoteChanged { contact_id, .. } => {
                format!("personal_note:{}", contact_id)
            }
            SyncItem::ContactFieldNoteChanged {
                contact_id,
                field_id,
                ..
            } => {
                format!("field_note:{}:{}", contact_id, field_id)
            }
            SyncItem::ProposalTrustChanged { contact_id, .. } => {
                format!("proposal_trust:{}", contact_id)
            }
            SyncItem::ImportedContactAdded { contact_data, .. } => {
                format!("imported_contact:{}", contact_data.id)
            }
            SyncItem::ImportedContactUpdated { contact_data, .. } => {
                format!("imported_contact:{}", contact_data.id)
            }
            SyncItem::ImportedContactRemoved { contact_id, .. } => {
                format!("imported_contact:{}", contact_id)
            }
            SyncItem::ContactArchived { contact_id, .. } => {
                format!("archive:{}", contact_id)
            }
            SyncItem::ContactUnarchived { contact_id, .. } => {
                format!("archive:{}", contact_id)
            }
        }
    }

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

    /// Saves a sync checkpoint before sending items to a target device.
    ///
    /// This allows resuming an interrupted sync from the last sent item
    /// rather than starting over. Call before beginning to send items.
    pub fn save_checkpoint(
        &self,
        target_device_id: &[u8; 32],
        items: &[SyncItem],
        sent_count: usize,
    ) -> Result<(), DeviceSyncError> {
        self.storage
            .sync()
            .save_sync_checkpoint(target_device_id, items, sent_count)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))
    }

    /// Loads a sync checkpoint for a target device.
    ///
    /// Returns the stored items and how many were already sent,
    /// or `None` if no checkpoint exists. Use this when reconnecting
    /// to resume an interrupted sync.
    pub fn load_checkpoint(
        &self,
        target_device_id: &[u8; 32],
    ) -> Result<Option<(Vec<SyncItem>, usize)>, DeviceSyncError> {
        self.storage
            .sync()
            .load_sync_checkpoint(target_device_id)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }

    /// Clears a sync checkpoint after successful completion.
    ///
    /// Call this after all items have been successfully sent to the
    /// target device.
    pub fn clear_checkpoint(&self, target_device_id: &[u8; 32]) -> Result<(), DeviceSyncError> {
        self.storage
            .sync()
            .clear_sync_checkpoint(target_device_id)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))
    }

    /// Builds device sync envelopes for all devices with pending items.
    ///
    /// Wraps each pending sync message in a serialized `EncryptedUpdate` where
    /// `recipient_id` is the daily self-token. The relay delivers it to all
    /// connections that registered the matching token.
    ///
    /// Returns a list of serialized envelopes ready to send over the wire.
    ///
    /// SP-33 Task 4.3.
    pub fn build_outbound_envelopes(
        &self,
        identity: &crate::Identity,
    ) -> Result<Vec<Vec<u8>>, DeviceSyncError> {
        use crate::network::mailbox_token::{compute_self_token, current_day_epoch, token_hex};

        let devices = self.devices_with_pending();
        if devices.is_empty() {
            return Ok(Vec::new());
        }

        let day = current_day_epoch(self.storage.clock().unix_seconds());
        let self_token = compute_self_token(identity.master_seed(), day);
        let recipient_id = token_hex(&self_token);

        let mut envelopes = Vec::with_capacity(devices.len());

        for device_id in &devices {
            let sync_msg = self.create_sync_message(device_id)?;
            if sync_msg.items.is_empty() {
                continue;
            }

            // Find the device's public key from the registry
            let active = self.registry.active_devices();
            let device = active
                .iter()
                .find(|d| &d.device_id == device_id)
                .ok_or_else(|| {
                    DeviceSyncError::Encryption(format!(
                        "Device {} not found in registry",
                        hex::encode(device_id)
                    ))
                })?;

            // Serialize and encrypt payload
            let payload_bytes = serde_json::to_vec(&sync_msg.items)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            let ciphertext =
                self.encrypt_for_device(&device.exchange_public_key, &payload_bytes)?;

            // Build a simple envelope struct (recipient_id = self-token)
            let envelope = SyncEnvelope {
                recipient_id: recipient_id.clone(),
                ciphertext,
                target_device_id: *device_id,
                version: sync_msg.version,
            };

            let encoded = serde_json::to_vec(&envelope)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
            envelopes.push(encoded);
        }

        Ok(envelopes)
    }
}

/// Builds device sync envelopes from storage state.
///
/// Loads the device registry and orchestrator state from storage, then
/// builds all pending sync envelopes. Returns an empty vec if there are
/// no linked devices or nothing to sync.
///
/// All clients should call this instead of implementing their own sync
/// orchestration logic.
pub fn build_device_sync_envelopes(
    identity: &crate::Identity,
    storage: &Storage,
) -> Result<Vec<Vec<u8>>, DeviceSyncError> {
    let registry = match storage
        .device()
        .load_device_registry()
        .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
    {
        Some(r) if r.device_count() > 1 => r,
        _ => return Ok(Vec::new()),
    };

    let orchestrator = DeviceSyncOrchestrator::load(
        storage,
        identity.create_device_info(storage.clock().unix_seconds()),
        registry,
    )?;

    orchestrator.build_outbound_envelopes(identity)
}

/// Serialized device sync envelope for wire transmission.
///
/// Contains a self-token `recipient_id` for relay routing, the encrypted
/// payload, and metadata for the target device.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncEnvelope {
    /// Self-token hex (64 chars) — relay routes on this.
    pub recipient_id: String,
    /// Encrypted sync payload (XChaCha20-Poly1305).
    pub ciphertext: Vec<u8>,
    /// Target device ID (for the recipient to identify which device state to update).
    pub target_device_id: [u8; 32],
    /// Version number for deduplication.
    pub version: u64,
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
