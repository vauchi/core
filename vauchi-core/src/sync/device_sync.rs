// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-to-Device Sync Module
//!
//! Handles syncing data between devices belonging to the same identity.
//! Used during device linking and for ongoing inter-device synchronization.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

mod payload;

pub use payload::DeviceSyncPayload;

mod sync_data;

pub use sync_data::{
    ContactExchangeLocation, ContactSyncData, GroupSyncData, ImportedContactSyncData,
    PlaceSyncData, TagSyncData,
};

/// Why a device is being linked.
///
/// Both modes establish fresh per-device sessions. No mode copies a live
/// ratchet chain between devices (ADR-064).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLinkIntent {
    /// The new device joins alongside the existing ones.
    AddDevice,
    /// The new device takes over from this one; the source device is
    /// expected to decommission after the transfer.
    ReplaceDevice,
}

/// A contact's identity-signed active-device registry.
///
/// This is safe to copy between owner devices; each receiving owner device
/// uses it to bootstrap fresh pairwise sessions. Live ratchet state is never
/// copied for concurrent operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactDeviceRegistrySyncData {
    pub contact_id: String,
    pub broadcast_json: String,
}

/// Errors that can occur during device sync.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum DeviceSyncError {
    #[error("Serialization failed: {0}")]
    Serialization(String),

    #[error("Deserialization failed: {0}")]
    Deserialization(String),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("Send failed: {0}")]
    SendFailed(String),

    #[error("DH validation failed: {0}")]
    DhValidation(#[from] crate::crypto::DhError),
}

// ============================================================
// Phase 4: Inter-Device Sync Types
// ============================================================

/// Types of sync events between devices.
///
/// Each SyncItem represents an atomic change that needs to be
/// synchronized across all devices belonging to the same identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SyncItem {
    /// A new contact was added.
    ContactAdded {
        /// Full contact data for reconstruction.
        contact_data: ContactSyncData,
        /// Timestamp when the contact was added.
        timestamp: u64,
    },

    /// A contact was removed.
    ContactRemoved {
        /// ID of the removed contact.
        contact_id: String,
        /// Timestamp of removal.
        timestamp: u64,
    },

    /// A peer's contact card was updated on this identity's other device.
    ///
    /// This is deliberately distinct from [`SyncItem::ContactAdded`]: a peer
    /// card update may update an existing contact, but it must never recreate a
    /// contact the owner removed on this device.
    ContactCardUpdated {
        /// Existing exchanged contact whose peer card changed.
        contact_id: String,
        /// Complete verified peer card as JSON.
        card_json: String,
        /// Timestamp when this device accepted the peer update.
        timestamp: u64,
    },

    /// The identity's signed active-device registry expanded or changed.
    DeviceRegistryChanged {
        /// Complete identity-signed registry as JSON.
        registry_json: String,
        /// Signed monotonic registry version used for conflict ordering.
        version: u64,
    },

    /// A contact's verified signed registry was received (F4, ADR-064
    /// Amendment 2026-07-25). Siblings persist it so they can resolve the
    /// contact's device-scoped tokens. Control-plane only.
    ContactRegistryReceived {
        /// Existing exchanged contact whose registry this is.
        contact_id: String,
        /// The contact's identity-signed `RegistryBroadcast` as JSON —
        /// siblings re-verify against the contact's pinned key on apply.
        registry_json: String,
        /// The broadcast's signed monotonic version.
        version: u64,
        /// When this device accepted the registry.
        timestamp: u64,
    },

    /// This identity's F4 activation handshake state for a contact changed.
    /// Snapshots the tracker; siblings merge monotonically (a newer local
    /// push is never regressed — any sibling may fetch the ack for a push
    /// another device sent, since mailbox tokens are identity-scoped).
    ContactActivationChanged {
        /// Contact the handshake is with.
        contact_id: String,
        /// Outstanding push correlation nonce (32 bytes when present).
        push_nonce: Option<Vec<u8>>,
        /// Outstanding pushed registry version.
        pushed_version: Option<u64>,
        /// Our registry version the peer confirmed.
        our_version_acked: Option<u64>,
        /// The peer registry version this identity holds.
        peer_version_held: Option<u64>,
        /// When the state changed on the recording device.
        timestamp: u64,
    },

    /// Own contact card field was updated.
    CardUpdated {
        /// Field label that was updated.
        field_label: String,
        /// New field value.
        new_value: String,
        /// Timestamp of update.
        timestamp: u64,
    },

    /// Own contact card field was removed.
    CardFieldRemoved {
        /// Field label that was removed.
        field_label: String,
        /// Timestamp of removal.
        timestamp: u64,
    },

    /// Visibility setting for a contact changed.
    VisibilityChanged {
        /// Contact ID whose visibility changed.
        contact_id: String,
        /// Own-card field **id** the override targets — the key the resolver
        /// reads. The wire key stays `field_label` (frozen by the V1 sync
        /// format, `protocol_compatibility_tests`) so cross-version device
        /// sync does not break; it carried a label pre-2026-06-14, when the
        /// toggle wrote label-keyed rules no read path consulted (F1).
        #[serde(rename = "field_label")]
        field_id: String,
        /// New visibility state.
        is_visible: bool,
        /// Timestamp of change.
        timestamp: u64,
    },

    /// Complete owner-private group state changed on a linked device.
    GroupChanged {
        group_data: GroupSyncData,
        timestamp: u64,
    },

    /// An owner-private group was deleted on a linked device.
    GroupDeleted { group_id: String, timestamp: u64 },

    /// Complete owner-private tag state changed on a linked device.
    TagChanged {
        tag_data: TagSyncData,
        timestamp: u64,
    },

    /// An owner-private tag was deleted on a linked device.
    TagDeleted { tag_id: String, timestamp: u64 },

    /// A contact's recovery trust status changed.
    ContactTrustChanged {
        /// Contact ID whose trust status changed.
        contact_id: String,
        /// New recovery trust state.
        recovery_trusted: bool,
        /// Timestamp of change.
        timestamp: u64,
    },

    /// Identity deletion has been scheduled on another device.
    ///
    /// Propagated via device sync so all linked devices can show the
    /// deletion countdown and execute at the same time.
    DeletionScheduled {
        /// When the deletion was scheduled.
        scheduled_at: u64,
        /// When the deletion should execute (after grace period).
        execute_at: u64,
        /// Timestamp of this sync event.
        timestamp: u64,
    },

    /// Identity deletion has been cancelled on another device.
    DeletionCancelled {
        /// Timestamp of this sync event.
        timestamp: u64,
    },

    /// Personal note for a contact was created or updated.
    ///
    /// Notes are private ("your eyes only") — never shared with the contact.
    PersonalNoteChanged {
        /// Contact whose note was changed.
        contact_id: String,
        /// New note text (plain text; stored encrypted at rest).
        note: String,
        /// Timestamp of change (milliseconds since UNIX epoch).
        timestamp: u64,
    },

    /// Personal note for a contact was removed.
    ///
    /// This tombstone shares the note's conflict key so an older note update
    /// cannot restore owner-private data after it was deleted.
    PersonalNoteRemoved {
        /// Contact whose note was removed.
        contact_id: String,
        /// Timestamp of change (milliseconds since UNIX epoch).
        timestamp: u64,
    },

    /// Per-field private note for a contact's shared field was created or updated.
    ///
    /// Field notes are private — never shared with the contact.
    ContactFieldNoteChanged {
        /// Contact whose field note was changed.
        contact_id: String,
        /// ID of the specific field the note is attached to.
        field_id: String,
        /// New note text (plain text; stored encrypted at rest).
        note: String,
        /// Timestamp of change (milliseconds since UNIX epoch).
        timestamp: u64,
    },

    /// A contact's proposal-trust status changed.
    ///
    /// `proposal_trusted` controls whether inbound card-update proposals from
    /// this contact are accepted automatically.
    ProposalTrustChanged {
        /// Contact ID whose proposal-trust status changed.
        contact_id: String,
        /// New proposal-trust state.
        proposal_trusted: bool,
        /// Timestamp of change (milliseconds since UNIX epoch).
        timestamp: u64,
    },

    /// An imported contact was added on another device.
    ImportedContactAdded {
        /// Full imported contact data for reconstruction.
        contact_data: ImportedContactSyncData,
        /// Timestamp when the contact was imported.
        timestamp: u64,
    },

    /// An imported contact's card was updated on another device.
    ImportedContactUpdated {
        /// Updated imported contact data.
        contact_data: ImportedContactSyncData,
        /// Timestamp of the update.
        timestamp: u64,
    },

    /// An imported contact was removed on another device.
    ImportedContactRemoved {
        /// ID of the removed imported contact.
        contact_id: String,
        /// Timestamp of removal.
        timestamp: u64,
    },

    /// A contact was archived on another device.
    ContactArchived {
        /// ID of the archived contact.
        contact_id: String,
        /// Timestamp of archival.
        timestamp: u64,
    },

    /// A contact was unarchived on another device.
    ContactUnarchived {
        /// ID of the unarchived contact.
        contact_id: String,
        /// Timestamp of unarchival.
        timestamp: u64,
    },

    /// Own contact-card field with its stable identity.
    ///
    /// New linked-device writes use this form so per-contact visibility
    /// rules, which are keyed by field id, remain valid on every sibling.
    ///
    /// This is appended to preserve the declaration order of established
    /// variants for any future non-JSON storage codec.
    CardFieldSynced {
        /// Complete field, including its stable identifier.
        field: crate::contact_card::ContactField,
        /// Explicit own-card visibility for this field, if configured.
        ///
        /// `None` retains the privacy-first unruled state rather than the
        /// `VisibilityRules::get` compatibility fallback.
        field_visibility: Option<crate::visibility::FieldVisibility>,
        /// Timestamp of update.
        timestamp: u64,
    },
}

impl SyncItem {
    /// Returns the timestamp of this sync item.
    pub fn timestamp(&self) -> u64 {
        match self {
            SyncItem::ContactAdded { timestamp, .. } => *timestamp,
            SyncItem::ContactRemoved { timestamp, .. } => *timestamp,
            SyncItem::ContactCardUpdated { timestamp, .. } => *timestamp,
            SyncItem::DeviceRegistryChanged { version, .. } => *version,
            SyncItem::ContactRegistryReceived { timestamp, .. } => *timestamp,
            SyncItem::ContactActivationChanged { timestamp, .. } => *timestamp,
            SyncItem::CardUpdated { timestamp, .. } => *timestamp,
            SyncItem::CardFieldSynced { timestamp, .. } => *timestamp,
            SyncItem::CardFieldRemoved { timestamp, .. } => *timestamp,
            SyncItem::VisibilityChanged { timestamp, .. } => *timestamp,
            SyncItem::GroupChanged { timestamp, .. } => *timestamp,
            SyncItem::GroupDeleted { timestamp, .. } => *timestamp,
            SyncItem::TagChanged { timestamp, .. } => *timestamp,
            SyncItem::TagDeleted { timestamp, .. } => *timestamp,
            SyncItem::ContactTrustChanged { timestamp, .. } => *timestamp,
            SyncItem::DeletionScheduled { timestamp, .. } => *timestamp,
            SyncItem::DeletionCancelled { timestamp, .. } => *timestamp,
            SyncItem::PersonalNoteChanged { timestamp, .. } => *timestamp,
            SyncItem::PersonalNoteRemoved { timestamp, .. } => *timestamp,
            SyncItem::ContactFieldNoteChanged { timestamp, .. } => *timestamp,
            SyncItem::ProposalTrustChanged { timestamp, .. } => *timestamp,
            SyncItem::ImportedContactAdded { timestamp, .. } => *timestamp,
            SyncItem::ImportedContactUpdated { timestamp, .. } => *timestamp,
            SyncItem::ImportedContactRemoved { timestamp, .. } => *timestamp,
            SyncItem::ContactArchived { timestamp, .. } => *timestamp,
            SyncItem::ContactUnarchived { timestamp, .. } => *timestamp,
        }
    }

    /// Resolves conflict between two sync items using last-write-wins.
    ///
    /// The item with the later timestamp wins. When timestamps are equal,
    /// the device_id is used as a deterministic tie-breaker via lexicographic
    /// comparison — the item from the device with the higher device_id wins.
    /// This ensures all devices converge to the same result regardless of
    /// the order they process conflicting items.
    pub fn resolve_conflict(
        a: &SyncItem,
        b: &SyncItem,
        a_device_id: &[u8; 32],
        b_device_id: &[u8; 32],
    ) -> SyncItem {
        if a.timestamp() != b.timestamp() {
            // Different timestamps: later wins
            if a.timestamp() > b.timestamp() {
                a.clone()
            } else {
                b.clone()
            }
        } else {
            // Equal timestamps: use device_id as deterministic tie-breaker
            // Higher device_id (lexicographic) wins
            if a_device_id >= b_device_id {
                a.clone()
            } else {
                b.clone()
            }
        }
    }

    /// Serializes this item to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("SyncItem serialization should not fail")
    }

    /// Deserializes an item from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }
}

/// Upper bound on items per owner-sync batch — reject absurd arrays
/// before element decoding (DC-01).
const MAX_SYNC_ITEMS_PER_BATCH: usize = 10_000;

/// Result of tolerantly decoding a batch of [`SyncItem`]s.
///
/// Release A of the readers-before-writers rollout
/// (`backlog/2026-07-21-per-device-ratchet-registry-dormant` §Progress):
/// known items survive unknown or malformed siblings emitted by a newer
/// linked device, instead of the whole batch failing on the first one.
#[derive(Debug)]
pub struct DecodedSyncItems {
    /// Items this binary understands, in batch order.
    pub known: Vec<SyncItem>,
    /// Externally tagged variants this binary does not know.
    pub unknown_count: usize,
    /// Known variants whose fields failed to decode.
    pub malformed_count: usize,
}

impl DecodedSyncItems {
    /// True when at least one batch element could not be decoded.
    pub fn has_skipped(&self) -> bool {
        self.unknown_count > 0 || self.malformed_count > 0
    }
}

/// Decodes an owner-sync item batch, skipping (and counting) elements
/// this binary cannot represent. The outer value must be a JSON array
/// within [`MAX_SYNC_ITEMS_PER_BATCH`]; anything else fails closed.
pub fn decode_sync_items_tolerantly(bytes: &[u8]) -> Result<DecodedSyncItems, DeviceSyncError> {
    let values: Vec<serde_json::Value> = serde_json::from_slice(bytes)
        .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
    if values.len() > MAX_SYNC_ITEMS_PER_BATCH {
        return Err(DeviceSyncError::Deserialization(format!(
            "sync batch exceeds {MAX_SYNC_ITEMS_PER_BATCH} items"
        )));
    }
    let mut decoded = DecodedSyncItems {
        known: Vec::with_capacity(values.len()),
        unknown_count: 0,
        malformed_count: 0,
    };
    for value in values {
        match serde_json::from_value::<SyncItem>(value) {
            Ok(item) => decoded.known.push(item),
            // The unknown/malformed split is diagnostic only — both are
            // skipped. serde has no typed unknown-variant error, so the
            // stable message prefix is the only available classifier.
            Err(e) if e.to_string().starts_with("unknown variant") => decoded.unknown_count += 1,
            Err(_) => decoded.malformed_count += 1,
        }
    }
    Ok(decoded)
}

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
}

impl InterDeviceSyncState {
    /// Creates a new sync state for the given device.
    pub fn new(device_id: [u8; 32]) -> Self {
        InterDeviceSyncState {
            device_id,
            pending_items: Vec::new(),
            last_sync_version: 0,
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
    }

    /// Marks items as synced up to the given version.
    pub fn mark_synced(&mut self, version: u64) {
        self.pending_items.clear();
        self.last_sync_version = version;
    }

    /// Serializes the sync state to JSON.
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
        }
        let raw: TolerantState = serde_json::from_str(json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
        if raw.pending_items.len() > MAX_SYNC_ITEMS_PER_BATCH {
            return Err(DeviceSyncError::Deserialization(format!(
                "pending queue exceeds {MAX_SYNC_ITEMS_PER_BATCH} items"
            )));
        }
        let pending_items = raw
            .pending_items
            .into_iter()
            .filter_map(|value| serde_json::from_value::<SyncItem>(value).ok())
            .collect();
        Ok(InterDeviceSyncState {
            device_id: raw.device_id,
            pending_items,
            last_sync_version: raw.last_sync_version,
        })
    }
}

/// Conflict-resolution stamp for one field: the timestamp and the
/// originating device id of the current winning write.
///
/// Ordering is **lexicographic — timestamp first, then device id** (the
/// derived `Ord` compares fields in declaration order), implementing
/// ADR-020: last-write-wins, with the higher device id breaking exact
/// timestamp ties deterministically and identically on every device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FieldStamp {
    /// Unix-ms timestamp of the write.
    pub timestamp: u64,
    /// Device id that originated the write (ADR-020 tie-breaker).
    pub device_id: [u8; 32],
}

/// Version vector for causality tracking across devices.
///
/// Used to detect concurrent updates and determine if changes
/// happened before, after, or concurrently with other changes.
///
/// ## Integration Status (Tracker #34)
///
/// The `is_concurrent_with` and `dominates` methods are implemented and tested,
/// but the `DeviceSyncOrchestrator::process_incoming` method does NOT use them.
/// Conflict resolution currently relies on `field_timestamps` ([`FieldStamp`]
/// LWW + device-id tie-break, ADR-020) rather than vector clock causality.
/// The `field_timestamps` map is persisted to storage (table
/// `sync_field_timestamps`, migration v48).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionVector {
    /// Map of device ID to version number.
    #[serde(with = "version_map_serde")]
    versions: HashMap<[u8; 32], u64>,
}

impl VersionVector {
    /// Creates an empty version vector.
    pub fn new() -> Self {
        VersionVector {
            versions: HashMap::new(),
        }
    }

    /// Gets the version for a device.
    pub fn get(&self, device_id: &[u8; 32]) -> u64 {
        self.versions.get(device_id).copied().unwrap_or(0)
    }

    /// Increments the version for a device.
    pub fn increment(&mut self, device_id: &[u8; 32]) {
        let current = self.get(device_id);
        self.versions.insert(*device_id, current + 1);
    }

    /// Merges two version vectors, taking the max of each device's version.
    pub fn merge(a: &VersionVector, b: &VersionVector) -> VersionVector {
        let mut merged = a.clone();

        for (device_id, &version) in &b.versions {
            let current = merged.get(device_id);
            if version > current {
                merged.versions.insert(*device_id, version);
            }
        }

        merged
    }

    /// Checks if this vector is concurrent with another.
    ///
    /// Two vectors are concurrent if neither dominates the other
    /// (i.e., each has at least one version the other doesn't have).
    pub fn is_concurrent_with(&self, other: &VersionVector) -> bool {
        let self_dominates = self.dominates(other);
        let other_dominates = other.dominates(self);

        // Concurrent if neither dominates
        !self_dominates && !other_dominates
    }

    /// Checks if this vector dominates another.
    ///
    /// A dominates B if A[d] >= B[d] for all devices d,
    /// and A[d] > B[d] for at least one device.
    fn dominates(&self, other: &VersionVector) -> bool {
        let mut dominated = false;

        // Check all devices in other
        for (device_id, &other_ver) in &other.versions {
            let self_ver = self.get(device_id);
            if self_ver < other_ver {
                return false; // other has a higher version
            }
            if self_ver > other_ver {
                dominated = true;
            }
        }

        // Check devices only in self
        for (device_id, &self_ver) in &self.versions {
            if !other.versions.contains_key(device_id) && self_ver > 0 {
                dominated = true;
            }
        }

        dominated
    }

    /// Serializes the version vector to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("VersionVector serialization should not fail")
    }

    /// Deserializes version vector from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }
}

// ============================================================
// Phase 7: Timestamp Validation
// ============================================================

/// Maximum allowed clock drift into the future (5 minutes in seconds).
const MAX_FUTURE_DRIFT_SECS: u64 = 300;

/// Validates that a timestamp is reasonable.
///
/// A timestamp is valid if:
/// - It is not zero (zero indicates uninitialized or invalid data)
/// - It is not in the far future (more than 5 minutes ahead of current time)
///
/// This prevents accepting sync items with clearly bogus timestamps
/// that could arise from clock skew, replay attacks, or data corruption.
pub fn validate_timestamp(timestamp: u64, now: u64) -> bool {
    if timestamp == 0 {
        return false;
    }

    // Reject timestamps more than MAX_FUTURE_DRIFT_SECS in the future
    timestamp <= now + MAX_FUTURE_DRIFT_SECS
}

/// Serde helper for 32-byte arrays.
mod bytes_array_32 {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a 32-byte array to a base64-encoded string for device sync payloads.
    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    /// Deserializes a 32-byte array from a base64-encoded string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = BASE64.decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 32-byte array"))
    }
}

/// Serde helper for HashMap<[u8; 32], u64> using hex-encoded keys.
mod version_map_serde {
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};
    use std::collections::HashMap;
    use std::fmt;

    /// Serializes a version map with 32-byte device keys as hex-encoded strings.
    pub fn serialize<S>(map: &HashMap<[u8; 32], u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut ser_map = serializer.serialize_map(Some(map.len()))?;
        for (key, value) in map {
            ser_map.serialize_entry(&hex::encode(key), value)?;
        }
        ser_map.end()
    }

    /// Deserializes a version map from hex-encoded 32-byte device keys.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<[u8; 32], u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VersionMapVisitor;

        impl<'de> Visitor<'de> for VersionMapVisitor {
            type Value = HashMap<[u8; 32], u64>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map with hex-encoded 32-byte keys and u64 values")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut map = HashMap::new();
                while let Some((key, value)) = access.next_entry::<String, u64>()? {
                    let bytes = hex::decode(&key).map_err(serde::de::Error::custom)?;
                    let arr: [u8; 32] = bytes
                        .try_into()
                        .map_err(|_| serde::de::Error::custom("invalid key length"))?;
                    map.insert(arr, value);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(VersionMapVisitor)
    }
}
