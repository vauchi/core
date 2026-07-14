// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-to-Device Sync Module
//!
//! Handles syncing data between devices belonging to the same identity.
//! Used during device linking and for ongoing inter-device synchronization.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::contact::{Contact, Group, ImportSource};
use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::identifiers::IdentityKey;

/// Serializable contact data for device sync.
///
/// Contains all information needed to reconstruct a contact on a new device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct ContactSyncData {
    /// Contact's unique ID (public key fingerprint).
    pub id: String,
    /// Contact's Ed25519 public key.
    #[serde(with = "crate::identifiers::wire_identity_key_base64")]
    pub public_key: IdentityKey,
    /// Contact's display name.
    pub display_name: String,
    /// Contact's card as JSON.
    pub card_json: String,
    /// Shared symmetric key bytes.
    #[serde(with = "bytes_array_32")]
    pub shared_key: [u8; 32],
    /// Exchange timestamp.
    pub exchange_timestamp: u64,
    /// Whether fingerprint was verified.
    pub fingerprint_verified: bool,
    /// Visibility rules as JSON.
    pub visibility_rules_json: String,
    /// Whether this contact is trusted for recovery.
    #[serde(default)]
    pub recovery_trusted: bool,
}

impl ContactSyncData {
    /// Creates sync data from a contact.
    ///
    /// Only exchanged contacts can be synced (imported contacts have no crypto
    /// fields). Panics if called on an imported contact — callers should filter.
    pub fn from_contact(contact: &Contact) -> Self {
        // All synced contacts are exchanged. Imported contacts are not synced.
        let ex = contact
            .kind()
            .exchanged_data()
            .expect("Only exchanged contacts can be synced");

        let card_json =
            serde_json::to_string(contact.card()).expect("Card serialization should not fail");
        let visibility_rules_json = serde_json::to_string(&ex.visibility_rules)
            .expect("Visibility rules serialization should not fail");

        ContactSyncData {
            id: contact.id().to_string(),
            public_key: IdentityKey::from(ex.public_key),
            display_name: contact.display_name().to_string(),
            card_json,
            shared_key: *ex.shared_key.as_bytes(),
            exchange_timestamp: ex.exchange_timestamp,
            fingerprint_verified: ex.fingerprint_verified,
            visibility_rules_json,
            recovery_trusted: ex.recovery_trusted,
        }
    }

    /// Converts sync data back to a contact.
    pub fn to_contact(&self) -> Result<Contact, DeviceSyncError> {
        let card: ContactCard = serde_json::from_str(&self.card_json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        let visibility_rules = serde_json::from_str(&self.visibility_rules_json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        // Peer-supplied bytes — trust boundary. `try_from_bytes` rejects the
        // all-zeros degenerate key per the contract on encryption.rs:95-100.
        let shared_key = SymmetricKey::try_from_bytes(self.shared_key)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        let mut contact = Contact::from_sync_data(
            self.public_key.into_bytes(),
            card,
            shared_key,
            self.exchange_timestamp,
            self.fingerprint_verified,
            visibility_rules,
        );
        // All synced contacts are exchanged, so this always succeeds —
        // best-effort: if the invariant is ever violated (e.g. blocked
        // contact synced), the flag falls back to default
        #[allow(clippy::let_underscore_must_use)]
        let _ = contact.set_recovery_trusted(self.recovery_trusted);
        Ok(contact)
    }
}

/// Serializable imported contact data for device sync.
///
/// Contains all information needed to reconstruct an imported contact on a new
/// device. Unlike [`ContactSyncData`], this has no crypto fields — imported
/// contacts are identified by UUID, not public-key fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedContactSyncData {
    /// UUID v4 identifier for the imported contact.
    pub id: String,
    /// Contact's display name.
    pub display_name: String,
    /// ContactCard serialized as JSON.
    pub card_json: String,
    /// ImportSource serialized as JSON.
    pub source: String,
    /// Unix timestamp (seconds) when the contact was imported.
    pub imported_at: u64,
    /// Original vCard UID, if present — used for re-import dedup.
    pub original_uid: Option<String>,
    /// Whether this contact is hidden from the main contact list.
    #[serde(default)]
    pub hidden: bool,
    /// Whether this contact is blocked.
    #[serde(default)]
    pub blocked: bool,
    /// Whether this contact is a favorite.
    #[serde(default)]
    pub favorite: bool,
}

impl ImportedContactSyncData {
    /// Creates sync data from an imported contact.
    ///
    /// Returns `None` if `contact` is not an imported contact.
    pub fn from_contact(contact: &Contact) -> Option<Self> {
        let imported = contact.kind().imported_data()?;
        let card_json = serde_json::to_string(contact.card()).ok()?;
        let source = serde_json::to_string(&imported.source).ok()?;
        Some(Self {
            id: contact.id().to_string(),
            display_name: contact.display_name().to_string(),
            card_json,
            source,
            imported_at: imported.imported_at,
            original_uid: imported.original_uid.clone(),
            hidden: contact.is_hidden(),
            blocked: contact.is_blocked(),
            favorite: contact.is_favorite(),
        })
    }

    /// Converts sync data back to an imported contact.
    pub fn to_contact(&self) -> Result<Contact, DeviceSyncError> {
        let card: ContactCard = serde_json::from_str(&self.card_json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
        let source: ImportSource = serde_json::from_str(&self.source)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
        let mut contact = Contact::from_import_stored(
            self.id.clone(),
            card,
            source,
            self.imported_at,
            self.original_uid.clone(),
        );
        if self.hidden {
            contact.hide();
        }
        if self.blocked {
            contact.block();
        }
        if self.favorite {
            contact.set_favorite(true);
        }
        Ok(contact)
    }
}

/// Serializable form of an owner-private `Tag` for device sync (ADR-051).
///
/// Tags are owner-private and never leave the owner's devices; the device-sync
/// payload itself is encrypted device-to-device, so the name travels in
/// plaintext inside that envelope (like `card_json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSyncData {
    /// Stable tag id (preserved across devices).
    pub id: String,
    /// Tag name.
    pub name: String,
    /// Contact ids carrying this tag.
    pub contact_ids: Vec<String>,
    /// Creation timestamp.
    pub created_at: u64,
}

impl TagSyncData {
    /// Builds sync data from a tag.
    pub fn from_tag(tag: &crate::contact::Tag) -> Self {
        let mut contact_ids: Vec<String> = tag.contact_ids.iter().cloned().collect();
        contact_ids.sort();
        TagSyncData {
            id: tag.id.clone(),
            name: tag.name.clone(),
            contact_ids,
            created_at: tag.created_at,
        }
    }

    /// Reconstructs a tag from sync data.
    pub fn to_tag(&self) -> crate::contact::Tag {
        crate::contact::Tag {
            id: self.id.clone(),
            name: self.name.clone(),
            contact_ids: self.contact_ids.iter().cloned().collect(),
            created_at: self.created_at,
        }
    }
}

/// Serializable owner-private group state for linked-device sync (ADR-054).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupSyncData {
    pub id: String,
    pub name: String,
    pub contact_ids: Vec<String>,
    pub visible_fields: Vec<String>,
    pub display_name_override: Option<String>,
    pub bio_override: Option<String>,
    pub avatar_override: Option<Vec<u8>>,
    pub created_at: u64,
    pub modified_at: u64,
}

impl GroupSyncData {
    pub fn from_group(group: &Group) -> Self {
        let mut contact_ids: Vec<String> = group.contacts().iter().cloned().collect();
        contact_ids.sort();
        let mut visible_fields: Vec<String> = group.visible_fields().iter().cloned().collect();
        visible_fields.sort();
        GroupSyncData {
            id: group.id().to_string(),
            name: group.name().to_string(),
            contact_ids,
            visible_fields,
            display_name_override: group.display_name_override().map(str::to_string),
            bio_override: group.bio_override().map(str::to_string),
            avatar_override: group.avatar_override().map(<[u8]>::to_vec),
            created_at: group.created_at(),
            modified_at: group.modified_at(),
        }
    }

    pub fn to_group(&self) -> Group {
        Group::from_storage(
            self.id.clone(),
            self.name.clone(),
            self.contact_ids.iter().cloned().collect(),
            self.visible_fields.iter().cloned().collect(),
            self.display_name_override.clone(),
            self.bio_override.clone(),
            self.avatar_override.clone(),
            self.created_at,
            self.modified_at,
        )
    }
}

/// Serializable form of an owner-private named [`Place`] for device sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceSyncData {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub created_at: u64,
}

impl PlaceSyncData {
    pub fn from_place(place: &crate::contact::place::Place) -> Self {
        PlaceSyncData {
            id: place.id.clone(),
            name: place.name.clone(),
            latitude: place.latitude,
            longitude: place.longitude,
            created_at: place.created_at,
        }
    }

    pub fn to_place(&self) -> crate::contact::place::Place {
        crate::contact::place::Place {
            id: self.id.clone(),
            name: self.name.clone(),
            latitude: self.latitude,
            longitude: self.longitude,
            created_at: self.created_at,
        }
    }
}

/// A contact's exchange location for device sync, keyed by `contact_id`
/// (kind-agnostic: applies to exchanged and imported contacts alike).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactExchangeLocation {
    pub contact_id: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub place_id: Option<String>,
}

impl ContactExchangeLocation {
    pub fn from_parts(contact_id: &str, loc: &crate::contact::place::ExchangeLocation) -> Self {
        ContactExchangeLocation {
            contact_id: contact_id.to_string(),
            latitude: loc.latitude,
            longitude: loc.longitude,
            place_id: loc.place_id.clone(),
        }
    }

    pub fn location(&self) -> crate::contact::place::ExchangeLocation {
        crate::contact::place::ExchangeLocation {
            latitude: self.latitude,
            longitude: self.longitude,
            place_id: self.place_id.clone(),
        }
    }
}

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

/// Payload for syncing all contacts during device linking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncPayload {
    /// Exchanged contacts to sync (have crypto fields).
    pub contacts: Vec<ContactSyncData>,
    /// Imported contacts to sync (no crypto fields).
    #[serde(default)]
    pub imported_contacts: Vec<ImportedContactSyncData>,
    /// User's own contact card.
    pub own_card_json: String,
    /// Owner-private tags (ADR-051). `#[serde(default)]` for back-compat with
    /// payloads from older devices that predate tags.
    #[serde(default)]
    pub tags: Vec<TagSyncData>,
    /// Owner-private groups and presentation overrides (ADR-054).
    /// `#[serde(default)]` preserves compatibility with payloads created
    /// before linked-device group convergence was implemented.
    #[serde(default)]
    pub groups: Vec<GroupSyncData>,
    /// Owner-private named places (ADR-051). `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub places: Vec<PlaceSyncData>,
    /// Per-contact exchange locations (ADR-051). `#[serde(default)]` back-compat.
    #[serde(default)]
    pub exchange_locations: Vec<ContactExchangeLocation>,
    /// Verified peer topology needed to establish this device's own sessions.
    #[serde(default)]
    pub contact_device_registries: Vec<ContactDeviceRegistrySyncData>,
    /// Version number for conflict resolution.
    pub version: u64,
}

impl DeviceSyncPayload {
    /// Creates an empty sync payload.
    pub fn empty() -> Self {
        DeviceSyncPayload {
            contacts: Vec::new(),
            imported_contacts: Vec::new(),
            own_card_json: String::new(),
            tags: Vec::new(),
            groups: Vec::new(),
            places: Vec::new(),
            exchange_locations: Vec::new(),
            contact_device_registries: Vec::new(),
            version: 0,
        }
    }

    /// Creates a sync payload from contacts and card.
    ///
    /// Separates exchanged contacts (with crypto fields) from imported
    /// contacts (no crypto fields) to avoid panics on mixed lists.
    pub fn new(contacts: &[Contact], own_card: &ContactCard, version: u64) -> Self {
        let mut exchanged = Vec::new();
        let mut imported = Vec::new();
        for contact in contacts {
            if contact.is_exchanged() {
                exchanged.push(ContactSyncData::from_contact(contact));
            } else if let Some(sync_data) = ImportedContactSyncData::from_contact(contact) {
                imported.push(sync_data);
            }
        }

        let own_card_json =
            serde_json::to_string(own_card).expect("Card serialization should not fail");

        DeviceSyncPayload {
            contacts: exchanged,
            imported_contacts: imported,
            own_card_json,
            tags: Vec::new(),
            groups: Vec::new(),
            places: Vec::new(),
            exchange_locations: Vec::new(),
            contact_device_registries: Vec::new(),
            version,
        }
    }

    /// Attaches owner-private tags to this payload (builder style).
    #[must_use]
    pub fn with_tags(mut self, tags: Vec<TagSyncData>) -> Self {
        self.tags = tags;
        self
    }

    /// Attaches owner-private groups to this payload, preserving stable ids,
    /// membership, field visibility, presentation overrides, and timestamps.
    #[must_use]
    pub fn with_groups(mut self, groups: Vec<GroupSyncData>) -> Self {
        self.groups = groups;
        self
    }

    /// Attaches named places to this payload (builder style).
    #[must_use]
    pub fn with_places(mut self, places: Vec<PlaceSyncData>) -> Self {
        self.places = places;
        self
    }

    /// Attaches per-contact exchange locations to this payload (builder style).
    #[must_use]
    pub fn with_exchange_locations(mut self, locs: Vec<ContactExchangeLocation>) -> Self {
        self.exchange_locations = locs;
        self
    }

    /// Attaches signed peer registries to the encrypted owner-device payload.
    #[must_use]
    pub fn with_contact_device_registries(
        mut self,
        registries: Vec<ContactDeviceRegistrySyncData>,
    ) -> Self {
        self.contact_device_registries = registries;
        self
    }

    /// Serializes the payload to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("DeviceSyncPayload serialization should not fail")
    }

    /// Deserializes a payload from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }

    /// Returns the total number of contacts (exchanged + imported).
    pub fn contact_count(&self) -> usize {
        self.contacts.len() + self.imported_contacts.len()
    }
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

    /// Own contact card field was updated.
    CardUpdated {
        /// Field label that was updated.
        field_label: String,
        /// New field value.
        new_value: String,
        /// Timestamp of update.
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

    /// A visibility label was created, updated, or deleted.
    LabelChange {
        /// The label's unique ID.
        label_id: String,
        /// The label's display name.
        label_name: String,
        /// Contact IDs assigned to this label.
        contacts: Vec<String>,
        /// Field IDs visible to contacts in this label.
        visible_fields: Vec<String>,
        /// Whether the label was deleted.
        is_deleted: bool,
        /// Timestamp of the change.
        timestamp: u64,
    },

    /// Complete owner-private group state changed on a linked device.
    GroupChanged {
        group_data: GroupSyncData,
        timestamp: u64,
    },

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
}

impl SyncItem {
    /// Returns the timestamp of this sync item.
    pub fn timestamp(&self) -> u64 {
        match self {
            SyncItem::ContactAdded { timestamp, .. } => *timestamp,
            SyncItem::ContactRemoved { timestamp, .. } => *timestamp,
            SyncItem::CardUpdated { timestamp, .. } => *timestamp,
            SyncItem::VisibilityChanged { timestamp, .. } => *timestamp,
            SyncItem::LabelChange { timestamp, .. } => *timestamp,
            SyncItem::GroupChanged { timestamp, .. } => *timestamp,
            SyncItem::ContactTrustChanged { timestamp, .. } => *timestamp,
            SyncItem::DeletionScheduled { timestamp, .. } => *timestamp,
            SyncItem::DeletionCancelled { timestamp, .. } => *timestamp,
            SyncItem::PersonalNoteChanged { timestamp, .. } => *timestamp,
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
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
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
