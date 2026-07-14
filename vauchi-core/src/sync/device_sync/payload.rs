// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Full-state payload transferred while linking an owner device.

use serde::{Deserialize, Serialize};

use crate::contact::Contact;
use crate::contact_card::ContactCard;

use super::{
    ContactDeviceRegistrySyncData, ContactExchangeLocation, ContactSyncData, DeviceSyncError,
    GroupSyncData, ImportedContactSyncData, PlaceSyncData, TagSyncData,
};

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
