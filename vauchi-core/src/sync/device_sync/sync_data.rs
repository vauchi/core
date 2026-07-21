// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serializable wire data carried by owner-device sync payloads and items.
//!
//! Pure data types: reconstruction inputs for contacts, imports, tags,
//! groups, places, and exchange locations. Split from `device_sync.rs`
//! (VRS04 file-size seam); behavior stays with the item/state machinery
//! there.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::DeviceSyncError;
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
    #[serde(with = "crate::sync::device_sync::bytes_array_32")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
