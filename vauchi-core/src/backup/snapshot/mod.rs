// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Private password-backup state. The public legacy v3 DTOs remain unchanged.

mod codec;
mod contacts;
mod validation;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::card::ContactCard;
use crate::contact::{Contact, Group};
use crate::identity::Identity;
use crate::sync::GroupSyncData;

use super::{BackupError, FullBackupEnvelope, IdentitySection};
use contacts::SnapshotContact;

pub(crate) use codec::{SNAPSHOT_VERSION, decode, encode};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupSnapshot {
    pub version: u32,
    pub created_at: u64,
    pub identity: IdentitySection,
    #[serde(with = "super::full_backup::backup_contact_card")]
    pub own_card: Option<ContactCard>,
    pub contacts: Vec<SnapshotContact>,
    pub groups: Vec<GroupSyncData>,
    pub overrides: Vec<VisibilityOverride>,
}

impl Drop for BackupSnapshot {
    fn drop(&mut self) {
        self.identity.master_seed_b64.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VisibilityOverride {
    pub contact_id: String,
    pub field_id: String,
    pub visible: bool,
}

impl BackupSnapshot {
    pub(crate) fn from_captured_state(
        identity: &Identity,
        now: u64,
        own_card: Option<ContactCard>,
        contacts: Vec<Contact>,
        groups: Vec<Group>,
        overrides: Vec<VisibilityOverride>,
    ) -> Result<Self, BackupError> {
        let mut snapshot = Self {
            version: SNAPSHOT_VERSION.into(),
            created_at: now,
            identity: IdentitySection {
                display_name: identity.display_name().into(),
                master_seed_b64: BASE64.encode(identity.master_seed()),
                device_index: identity.device_index(),
                device_name: identity.device_info().device_name().into(),
            },
            own_card,
            contacts: contacts
                .iter()
                .map(SnapshotContact::capture)
                .collect::<Result<_, _>>()?,
            groups: groups.iter().map(GroupSyncData::from_group).collect(),
            overrides,
        };
        snapshot.normalize_references()?;
        Ok(snapshot)
    }

    pub fn from_legacy(envelope: FullBackupEnvelope) -> Result<Self, BackupError> {
        if envelope.version != 3 {
            return Err(BackupError::Deserialization(
                "Invalid legacy envelope version".into(),
            ));
        }
        let groups = envelope
            .sections
            .labels
            .into_iter()
            .map(|label| GroupSyncData {
                id: label.label_id,
                name: label.name,
                contact_ids: label.contacts,
                visible_fields: Vec::new(),
                display_name_override: None,
                bio_override: None,
                avatar_override: None,
                created_at: envelope.created_at,
                modified_at: envelope.created_at,
            })
            .collect();
        let contacts = envelope
            .sections
            .contacts
            .into_iter()
            .map(|value| {
                let entry = serde_json::from_value(value)
                    .map_err(|_| BackupError::Deserialization("Invalid legacy contact".into()))?;
                SnapshotContact::from_legacy(entry)
            })
            .collect::<Result<_, _>>()?;
        let mut snapshot = Self {
            version: 3,
            created_at: envelope.created_at,
            identity: envelope.sections.identity,
            own_card: envelope.sections.own_card,
            contacts,
            groups,
            overrides: Vec::new(),
        };
        snapshot.normalize_references()?;
        Ok(snapshot)
    }

    // Deletions leave inert label rows (#64); normalize only local capture and
    // legacy conversion. Authenticated v6 input must pass strict validation.
    fn normalize_references(&mut self) -> Result<(), BackupError> {
        let contacts = self.restore_contacts()?;
        let ids: std::collections::HashSet<_> =
            contacts.iter().map(|contact| contact.id()).collect();
        let fields: std::collections::HashSet<_> = self
            .own_card
            .iter()
            .flat_map(|card| card.fields().iter().map(|field| field.id()))
            .collect();
        for group in &mut self.groups {
            group.contact_ids.retain(|id| ids.contains(id.as_str()));
            group
                .visible_fields
                .retain(|id| fields.contains(id.as_str()));
        }
        self.overrides.retain(|item| {
            ids.contains(item.contact_id.as_str()) && fields.contains(item.field_id.as_str())
        });
        Ok(())
    }

    pub fn restore_contacts(&self) -> Result<Vec<Contact>, BackupError> {
        self.contacts.iter().map(SnapshotContact::restore).collect()
    }

    pub fn restore_groups(&self) -> Vec<Group> {
        self.groups.iter().map(GroupSyncData::to_group).collect()
    }
}
