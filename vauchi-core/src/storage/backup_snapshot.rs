// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage capture adapter for the pure backup representation.

use super::Storage;
use crate::backup::{
    BackupError,
    snapshot::{BackupSnapshot, VisibilityOverride},
};
use crate::identity::Identity;

impl BackupSnapshot {
    pub fn capture(storage: &Storage, identity: &Identity, now: u64) -> Result<Self, BackupError> {
        let map_error = |_| BackupError::Serialization("Could not read backup state".into());
        let mut contacts = storage.contacts().list_contacts().map_err(map_error)?;
        contacts.extend(
            storage
                .contacts()
                .list_archived_contacts()
                .map_err(map_error)?,
        );
        let groups = storage.labels().load_all_groups().map_err(map_error)?;
        let contact_ids: std::collections::HashSet<_> = contacts.iter().map(|c| c.id()).collect();
        let mut overrides = Vec::new();
        for (contact_id, fields) in storage
            .labels()
            .load_all_contact_overrides()
            .map_err(map_error)?
        {
            if contact_ids.contains(contact_id.as_str()) {
                overrides.extend(fields.into_iter().map(|(field_id, visible)| {
                    VisibilityOverride {
                        contact_id: contact_id.clone(),
                        field_id,
                        visible,
                    }
                }));
            }
        }
        Self::from_captured_state(
            identity,
            now,
            storage.contacts().load_own_card().map_err(map_error)?,
            contacts,
            groups,
            overrides,
        )
    }
}
